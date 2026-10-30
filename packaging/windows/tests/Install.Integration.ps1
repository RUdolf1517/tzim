[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$installerPath = Join-Path $repositoryRoot 'packaging\windows\install.ps1'
$artifactRoot = if ([string]::IsNullOrWhiteSpace($env:GITHUB_WORKSPACE)) {
    Join-Path $env:TEMP 'tzim-windows-installer-artifacts'
}
else {
    Join-Path $env:GITHUB_WORKSPACE 'artifacts'
}
[System.IO.Directory]::CreateDirectory($artifactRoot) | Out-Null
Start-Transcript -Path (Join-Path $artifactRoot 'windows-installer-transcript.txt') -Force | Out-Null

$initialUserPath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
$initialMachinePath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::Machine)
$initialProcessPath = $env:Path
$testBase = Join-Path ([Environment]::GetEnvironmentVariable('LOCALAPPDATA')) ('Programs\tzim installer test - 東京 - ' + [guid]::NewGuid().ToString('N'))
$latestRoot = Join-Path $testBase 'latest install path space 日本語'
$upgradeRoot = Join-Path $testBase 'upgrade install path space Łódź'
$manualRoot = Join-Path $testBase 'manual ZIP fallback path space 日本語'
$fixtures = Join-Path $artifactRoot 'release-fixtures'
$installerFunctionsLoaded = $false

function Assert-Equal {
    param($Expected, $Actual, [string]$Message)
    if (-not [object]::Equals($Expected, $Actual)) {
        throw "$Message Expected=[$Expected], Actual=[$Actual]"
    }
}

function Get-UserPathMatchCount {
    param([string]$InstallPath)
    $raw = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $foundEntries = @(([string]$raw).Split([char]';') | Where-Object {
        Test-TzimPathEntry -Entry $_ -InstallPath $InstallPath
    })
    return $foundEntries.Count
}

function Assert-InstalledVersion {
    param([string]$InstallPath, [string]$ExpectedVersion)
    $actual = Get-TzimArchiveVersion -ExecutablePath (Join-Path $InstallPath 'tzim.exe')
    Assert-Equal -Expected $ExpectedVersion -Actual $actual -Message "installed executable at $InstallPath"
}

function Assert-NewProcessResolvesInstall {
    param([string]$InstallPath, [string]$ExpectedVersion)

    $previousProcessPath = $env:Path
    $machinePath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::Machine)
    $userPath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $pathParts = @($machinePath, $userPath) | Where-Object { -not [string]::IsNullOrEmpty($_) }
    $env:Path = [string]::Join(';', $pathParts)
    try {
        $expectedExecutable = Join-Path $InstallPath 'tzim.exe'
        $expectedLiteral = "'" + $expectedExecutable.Replace("'", "''") + "'"
        $childCommand = '$resolved = Get-Command -Name tzim -CommandType Application -ErrorAction Stop; ' +
            'if ([string]::Equals($resolved.Source, ' + $expectedLiteral + ', [System.StringComparison]::OrdinalIgnoreCase)) { "PATH_OK" } else { "PATH_MISMATCH" }; ' +
            '& $resolved.Source --version'
        $powershell = Join-Path $PSHOME 'powershell.exe'
        $output = @(& $powershell -NoProfile -NonInteractive -Command $childCommand 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $env:Path = $previousProcessPath
    }

    $lines = @($output | ForEach-Object { [string]$_ })
    if ($exitCode -ne 0) {
        throw "A fresh PowerShell process could not resolve tzim from the persisted user PATH: $($lines -join '; ')"
    }
    Assert-Equal -Expected 'PATH_OK' -Actual $lines[0] -Message 'fresh process resolved the installed tzim.exe through user PATH'
    Assert-Equal -Expected "tzim $ExpectedVersion" -Actual $lines[-1] -Message 'fresh process reported the installed version'
    Write-Host "PASS new PowerShell process resolved tzim $ExpectedVersion from the persisted user PATH"
}

function New-AssetDownloader {
    param(
        [Parameter(Mandatory)][string]$Mode,
        [Parameter(Mandatory)][string]$ArchivePath,
        [Parameter(Mandatory)][string]$ChecksumsPath,
        [string]$AlternateArchivePath,
        [string]$AlternateChecksumsPath,
        [string]$FakeArchivePath,
        [string]$FakeChecksumsPath
    )

    $config = [pscustomobject]@{
        Mode = $Mode
        ArchivePath = $ArchivePath
        ChecksumsPath = $ChecksumsPath
        AlternateArchivePath = $AlternateArchivePath
        AlternateChecksumsPath = $AlternateChecksumsPath
        FakeArchivePath = $FakeArchivePath
        FakeChecksumsPath = $FakeChecksumsPath
    }
    $body = {
        param($Uri, $Destination)
        $assetName = [System.IO.Path]::GetFileName(([System.Uri]$Uri).AbsolutePath)
        if ($assetName -eq 'SHA256SUMS') {
            switch ($config.Mode) {
                'malformed-checksums' {
                    [System.IO.File]::WriteAllText($Destination, "this is not a checksum file`n")
                    return
                }
                'version-check' {
                    Copy-Item -LiteralPath $config.AlternateChecksumsPath -Destination $Destination -Force | Out-Null
                    return
                }
                'extraction-failure' {
                    Copy-Item -LiteralPath $config.FakeChecksumsPath -Destination $Destination -Force | Out-Null
                    return
                }
                default {
                    Copy-Item -LiteralPath $config.ChecksumsPath -Destination $Destination -Force | Out-Null
                    return
                }
            }
        }
        if ($assetName -ne 'tzim-x86_64-pc-windows-msvc.zip') {
            throw "Unexpected asset requested by installer test: $Uri"
        }
        if ($config.Mode -eq 'missing-asset') {
            throw 'simulated HTTP 404: release archive missing'
        }
        if ($config.Mode -eq 'version-check') {
            Copy-Item -LiteralPath $config.AlternateArchivePath -Destination $Destination -Force | Out-Null
            return
        }
        if ($config.Mode -eq 'extraction-failure') {
            Copy-Item -LiteralPath $config.FakeArchivePath -Destination $Destination -Force | Out-Null
            return
        }

        Copy-Item -LiteralPath $config.ArchivePath -Destination $Destination -Force | Out-Null
        if ($config.Mode -eq 'wrong-sha') {
            $bytes = [System.IO.File]::ReadAllBytes($Destination)
            $last = $bytes.Length - 1
            $bytes[$last] = [byte]($bytes[$last] -bxor 0xff)
            [System.IO.File]::WriteAllBytes($Destination, $bytes)
        }
        elseif ($config.Mode -eq 'truncated-download') {
            $bytes = [System.IO.File]::ReadAllBytes($Destination)
            $length = [int][Math]::Max(1, [Math]::Floor($bytes.Length / 2))
            $truncated = [System.Array]::CreateInstance([System.Byte], $length)
            [System.Array]::Copy($bytes, $truncated, $length)
            [System.IO.File]::WriteAllBytes($Destination, $truncated)
        }
    }.GetNewClosure()
    return $body
}

function Assert-FailedInstallPreservesPrevious {
    param(
        [Parameter(Mandatory)][string]$CaseName,
        [Parameter(Mandatory)][string]$ExpectedErrorPattern,
        [Parameter(Mandatory)][scriptblock]$DownloadAsset
    )

    $exePath = Join-Path $upgradeRoot 'tzim.exe'
    $beforeHash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    $beforePath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $beforeProcessPath = $env:Path
    $failed = $false
    $failureMessage = ''
    try {
        Invoke-TzimInstallCore -Version $script:latestVersion -InstallRoot $upgradeRoot `
            -ResolveLatest { $script:latestTag } -DownloadAsset $DownloadAsset
    }
    catch {
        $failed = $true
        $failureMessage = $_.Exception.Message
        Write-Host "EXPECTED FAILURE [$CaseName]: $failureMessage"
    }
    if (-not $failed) {
        throw "Failure injection '$CaseName' unexpectedly installed a replacement."
    }
    if (-not [System.Text.RegularExpressions.Regex]::IsMatch(
        $failureMessage,
        $ExpectedErrorPattern,
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )) {
        throw "Failure injection '$CaseName' failed for an unexpected reason: $failureMessage"
    }

    Assert-InstalledVersion -InstallPath $upgradeRoot -ExpectedVersion '0.14.0'
    $afterHash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    Assert-Equal -Expected $beforeHash -Actual $afterHash -Message "$CaseName preserved the previous executable hash"
    Assert-Equal -Expected $beforePath -Actual ([Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)) -Message "$CaseName preserved user PATH"
    Assert-Equal -Expected $beforeProcessPath -Actual $env:Path -Message "$CaseName preserved current-process PATH"
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $upgradeRoot) -Message "$CaseName retained the single installer-owned PATH entry"
    $parent = [System.IO.Directory]::GetParent($upgradeRoot).FullName
    $leftovers = @(Get-ChildItem -LiteralPath $parent -Force | Where-Object { $_.Name -like '.tzim-work-*' -or $_.Name -like '.tzim-stage-*' })
    Assert-Equal -Expected 0 -Actual $leftovers.Count -Message "$CaseName cleaned staging directories"
    Write-Host "PASS failure rollback: $CaseName"
}

function Assert-InjectedPromotionFailureRollsBack {
    param([Parameter(Mandatory)][scriptblock]$DownloadAsset)

    $exePath = Join-Path $upgradeRoot 'tzim.exe'
    $beforeHash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    $beforePath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $beforeProcessPath = $env:Path
    $moveCounter = [pscustomobject]@{ Count = 0 }
    $failSecondMove = {
        param($Source, $Destination)
        $moveCounter.Count = $moveCounter.Count + 1
        if ($moveCounter.Count -eq 2) {
            throw 'simulated stage promotion failure after the previous install moved to backup'
        }
        [System.IO.Directory]::Move($Source, $Destination)
    }.GetNewClosure()

    $failed = $false
    $failureMessage = ''
    try {
        Invoke-TzimInstallCore -Version $script:latestVersion -InstallRoot $upgradeRoot `
            -ResolveLatest { $script:latestTag } -DownloadAsset $DownloadAsset -MoveDirectory $failSecondMove
    }
    catch {
        $failed = $true
        $failureMessage = $_.Exception.Message
        Write-Host "EXPECTED FAILURE [replacement-promotion-rollback]: $failureMessage"
    }
    if (-not $failed -or $failureMessage -notmatch 'simulated stage promotion failure') {
        throw "Injected replacement failure did not reach the intended promotion step: $failureMessage"
    }
    Assert-Equal -Expected 2 -Actual $moveCounter.Count -Message 'promotion failure moved old install once and rejected new stage'
    Assert-InstalledVersion -InstallPath $upgradeRoot -ExpectedVersion '0.14.0'
    $afterHash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    Assert-Equal -Expected $beforeHash -Actual $afterHash -Message 'promotion failure restored the previous executable bytes'
    Assert-Equal -Expected $beforePath -Actual ([Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)) -Message 'promotion failure preserved user PATH'
    Assert-Equal -Expected $beforeProcessPath -Actual $env:Path -Message 'promotion failure preserved current-process PATH'
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $upgradeRoot) -Message 'promotion failure retained a single PATH entry'
    $parent = [System.IO.Directory]::GetParent($upgradeRoot).FullName
    $leftovers = @(Get-ChildItem -LiteralPath $parent -Force | Where-Object {
        $_.Name -like '.tzim-work-*' -or $_.Name -like '.tzim-stage-*' -or $_.Name -like '.tzim-backup-*'
    })
    Assert-Equal -Expected 0 -Actual $leftovers.Count -Message 'promotion failure restored backup and cleaned staging'
    Write-Host 'PASS post-backup replacement promotion failure rolled back to the prior install'
}

function Assert-LockedReplacementFails {
    param([Parameter(Mandatory)][scriptblock]$DownloadAsset)

    $exePath = Join-Path $upgradeRoot 'tzim.exe'
    $beforeHash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    $beforePath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $beforeProcessPath = $env:Path
    $lock = [System.IO.File]::Open(
        $exePath,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::None
    )
    try {
        $failed = $false
        $failureMessage = ''
        try {
            Invoke-TzimInstallCore -Version $script:latestVersion -InstallRoot $upgradeRoot `
                -ResolveLatest { $script:latestTag } -DownloadAsset $DownloadAsset
        }
        catch {
            $failed = $true
            $failureMessage = $_.Exception.Message
            Write-Host "EXPECTED FAILURE [replacement-failure-executable-in-use]: $failureMessage"
        }
        if (-not $failed) {
            throw 'Replacement unexpectedly succeeded while tzim.exe was exclusively open.'
        }
        if ($failureMessage -notmatch 'Cannot replace') {
            throw "Locked replacement failed for an unexpected reason: $failureMessage"
        }
        Assert-Equal -Expected $beforePath -Actual ([Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)) -Message 'locked replacement preserved user PATH'
        Assert-Equal -Expected $beforeProcessPath -Actual $env:Path -Message 'locked replacement preserved current-process PATH'
        Assert-Equal -Expected $true -Actual ([System.IO.Directory]::Exists($upgradeRoot)) -Message 'locked replacement kept the original install directory'
    }
    finally {
        $lock.Dispose()
    }

    Assert-InstalledVersion -InstallPath $upgradeRoot -ExpectedVersion '0.14.0'
    $afterHash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
    Assert-Equal -Expected $beforeHash -Actual $afterHash -Message 'locked replacement preserved the previous executable hash'
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $upgradeRoot) -Message 'locked replacement retained one PATH entry'
    Write-Host 'PASS failure rollback: replacement-failure-executable-in-use'
}

try {
    . $installerPath
    $installerFunctionsLoaded = $true

    $originalProcessorArchitecture = $env:PROCESSOR_ARCHITECTURE
    $originalWowArchitecture = $env:PROCESSOR_ARCHITEW6432
    try {
        $env:PROCESSOR_ARCHITECTURE = 'ARM64'
        $env:PROCESSOR_ARCHITEW6432 = 'ARM64'
        $architectureFailure = ''
        try {
            Assert-TzimX64Architecture
        }
        catch {
            $architectureFailure = $_.Exception.Message
        }
        if ($architectureFailure -notmatch 'Unsupported Windows architecture.*x64') {
            throw "Unsupported-architecture guard failed unexpectedly: $architectureFailure"
        }
    }
    finally {
        if ([string]::IsNullOrEmpty($originalProcessorArchitecture)) {
            Remove-Item Env:PROCESSOR_ARCHITECTURE -ErrorAction SilentlyContinue
        }
        else {
            $env:PROCESSOR_ARCHITECTURE = $originalProcessorArchitecture
        }
        if ([string]::IsNullOrEmpty($originalWowArchitecture)) {
            Remove-Item Env:PROCESSOR_ARCHITEW6432 -ErrorAction SilentlyContinue
        }
        else {
            $env:PROCESSOR_ARCHITEW6432 = $originalWowArchitecture
        }
    }
    Write-Host 'PASS unsupported Windows architecture is rejected explicitly'

    $latestTag = Get-TzimLatestReleaseTag
    $latestVersion = $latestTag.Substring(1)
    [System.IO.Directory]::CreateDirectory($fixtures) | Out-Null

    $releaseTags = @('v0.14.0', 'v0.15.0', $latestTag) | Select-Object -Unique
    foreach ($tag in $releaseTags) {
        $baseUri = 'https://github.com/RUdolf1517/tzim/releases/download/' + $tag + '/'
        $archivePath = Join-Path $fixtures ($tag + '-tzim-x86_64-pc-windows-msvc.zip')
        $checksumsPath = Join-Path $fixtures ($tag + '-SHA256SUMS')
        Save-TzimUrl -Uri ($baseUri + 'tzim-x86_64-pc-windows-msvc.zip') -Destination $archivePath
        Save-TzimUrl -Uri ($baseUri + 'SHA256SUMS') -Destination $checksumsPath
        $expected = Get-TzimReleaseChecksum -ChecksumPath $checksumsPath -AssetName 'tzim-x86_64-pc-windows-msvc.zip'
        $actual = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
        Assert-Equal -Expected $expected -Actual $actual -Message "real $tag Windows release asset checksum"
        Test-TzimArchiveEntries -ArchivePath $archivePath
        Write-Host "Verified real release inputs: $tag, archive SHA-256 $actual"
    }

    $archive015 = Join-Path $fixtures 'v0.15.0-tzim-x86_64-pc-windows-msvc.zip'
    $checksums015 = Join-Path $fixtures 'v0.15.0-SHA256SUMS'
    $archive014 = Join-Path $fixtures 'v0.14.0-tzim-x86_64-pc-windows-msvc.zip'
    $checksums014 = Join-Path $fixtures 'v0.14.0-SHA256SUMS'
    $archiveLatest = Join-Path $fixtures ($latestTag + '-tzim-x86_64-pc-windows-msvc.zip')
    $checksumsLatest = Join-Path $fixtures ($latestTag + '-SHA256SUMS')

    # Exercise the documented manual ZIP fallback against the same real inputs.
    $manualChecksumMatches = @(Select-String -Path $checksums015 `
        -Pattern '^([0-9a-fA-F]{64})  \./tzim-x86_64-pc-windows-msvc\.zip$')
    Assert-Equal -Expected 1 -Actual $manualChecksumMatches.Count -Message 'documented manual ZIP checksum selector'
    $manualExpectedHash = $manualChecksumMatches[0].Matches[0].Groups[1].Value
    $manualActualHash = (Get-FileHash -LiteralPath $archive015 -Algorithm SHA256).Hash
    Assert-Equal -Expected $manualExpectedHash -Actual $manualActualHash -Message 'documented manual ZIP SHA-256 check'
    New-Item -ItemType Directory -Force -Path $manualRoot | Out-Null
    Expand-Archive -LiteralPath $archive015 -DestinationPath $manualRoot -Force
    Assert-InstalledVersion -InstallPath $manualRoot -ExpectedVersion '0.15.0'
    Remove-Item -LiteralPath $manualRoot -Recurse -Force
    Write-Host 'PASS documented fixed-tag manual ZIP checksum verification, extraction, and version check'

    # Resolve the actual latest tag once, then prove the installer calls its
    # resolver once and both asset requests stay pinned to that fixed tag.
    $resolutionCounter = [pscustomobject]@{ Count = 0 }
    $downloadUris = New-Object 'System.Collections.Generic.List[string]'
    $latestBaseUri = 'https://github.com/RUdolf1517/tzim/releases/download/' + $latestTag + '/'
    $resolveLatest = {
        $resolutionCounter.Count = $resolutionCounter.Count + 1
        $latestTag
    }.GetNewClosure()
    $localLatestDownload = {
        param($Uri, $Destination)
        $downloadUris.Add($Uri)
        if ($Uri -ceq ($latestBaseUri + 'tzim-x86_64-pc-windows-msvc.zip')) {
            Copy-Item -LiteralPath $archiveLatest -Destination $Destination -Force | Out-Null
        }
        elseif ($Uri -ceq ($latestBaseUri + 'SHA256SUMS')) {
            Copy-Item -LiteralPath $checksumsLatest -Destination $Destination -Force | Out-Null
        }
        else {
            throw "latest install did not use a fixed, expected release URL: $Uri"
        }
    }.GetNewClosure()

    # Record a pre-existing user PATH entry: the installer must not duplicate
    # or claim it, and uninstall must leave it untouched.
    $preexistingLatestPath = if ([string]::IsNullOrEmpty($initialUserPath)) { $latestRoot } else { $initialUserPath + ';' + $latestRoot }
    [Environment]::SetEnvironmentVariable('Path', $preexistingLatestPath, [EnvironmentVariableTarget]::User)
    $env:Path = if ([string]::IsNullOrEmpty($initialProcessPath)) { $latestRoot } else { $initialProcessPath + ';' + $latestRoot }

    Invoke-TzimInstallCore -Version 'latest' -InstallRoot $latestRoot `
        -ResolveLatest $resolveLatest -DownloadAsset $localLatestDownload
    Assert-Equal -Expected 1 -Actual $resolutionCounter.Count -Message 'latest resolver callback invocation count'
    Assert-Equal -Expected 2 -Actual $downloadUris.Count -Message 'fixed release asset request count'
    Assert-InstalledVersion -InstallPath $latestRoot -ExpectedVersion $latestVersion
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $latestRoot) -Message 'latest user PATH remains unique'
    Assert-Equal -Expected $preexistingLatestPath -Actual ([Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)) -Message 'latest did not rewrite an existing user PATH entry'
    $latestMarker = Get-Content -LiteralPath (Join-Path $latestRoot '.tzim-installer.json') -Raw | ConvertFrom-Json
    Assert-Equal -Expected $false -Actual ([bool]$latestMarker.path_entry_owned) -Message 'pre-existing PATH ownership marker'

    # The public script wrapper accepts an explicit version and is idempotent.
    & $installerPath -Version $latestVersion -InstallRoot $latestRoot
    Assert-InstalledVersion -InstallPath $latestRoot -ExpectedVersion $latestVersion
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $latestRoot) -Message 'repeat install did not duplicate PATH'
    $sentinel = Join-Path $latestRoot 'user-owned-keep.txt'
    [System.IO.File]::WriteAllText($sentinel, 'not installed by the tzim installer')
    & $installerPath -Uninstall -InstallRoot $latestRoot
    Assert-Equal -Expected $true -Actual (Test-Path -LiteralPath $sentinel -PathType Leaf) -Message 'uninstall preserved an unrelated file'
    Assert-Equal -Expected $preexistingLatestPath -Actual ([Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)) -Message 'uninstall preserved a pre-existing PATH entry'
    Assert-Equal -Expected $false -Actual (Test-Path -LiteralPath (Join-Path $latestRoot 'tzim.exe')) -Message 'uninstall removed owned executable'
    Assert-Equal -Expected $false -Actual (Test-Path -LiteralPath (Join-Path $latestRoot 'LICENSE')) -Message 'uninstall removed owned license'
    Assert-Equal -Expected $false -Actual (Test-Path -LiteralPath (Join-Path $latestRoot '.tzim-installer.json')) -Message 'uninstall removed ownership marker'
    Remove-Item -LiteralPath $testBase -Recurse -Force
    [Environment]::SetEnvironmentVariable('Path', $initialUserPath, [EnvironmentVariableTarget]::User)
    $env:Path = $initialProcessPath
    Write-Host 'PASS latest install, explicit-version rerun, pre-existing PATH ownership, and selective uninstall'

    # A real upgrade from the older published Windows binary to the current latest release.
    & $installerPath -Version '0.14.0' -InstallRoot $upgradeRoot
    Assert-InstalledVersion -InstallPath $upgradeRoot -ExpectedVersion '0.14.0'
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $upgradeRoot) -Message 'fresh install added exactly one user PATH entry'
    Assert-NewProcessResolvesInstall -InstallPath $upgradeRoot -ExpectedVersion '0.14.0'

    $fakeArchive = Join-Path $fixtures 'not-a-zip.zip'
    $fakeChecksums = Join-Path $fixtures 'not-a-zip-SHA256SUMS'
    [System.IO.File]::WriteAllBytes($fakeArchive, [System.Text.Encoding]::ASCII.GetBytes('this is deliberately not a ZIP archive'))
    $fakeHash = (Get-FileHash -LiteralPath $fakeArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText($fakeChecksums, "$fakeHash  ./tzim-x86_64-pc-windows-msvc.zip`n")

    $failureCases = @(
        @{ Name = 'wrong-sha'; Expected = 'SHA-256 mismatch' },
        @{ Name = 'truncated-download'; Expected = 'SHA-256 mismatch' },
        @{ Name = 'missing-asset'; Expected = 'simulated HTTP 404' },
        @{ Name = 'malformed-checksums'; Expected = 'Malformed SHA256SUMS line' },
        @{ Name = 'extraction-failure'; Expected = 'Central Directory|ZIP archive' },
        @{ Name = 'version-check'; Expected = 'Version check failed' }
    )
    foreach ($failureCase in $failureCases) {
        $downloader = New-AssetDownloader -Mode $failureCase.Name `
            -ArchivePath $archiveLatest -ChecksumsPath $checksumsLatest `
            -AlternateArchivePath $archive014 -AlternateChecksumsPath $checksums014 `
            -FakeArchivePath $fakeArchive -FakeChecksumsPath $fakeChecksums
        Assert-FailedInstallPreservesPrevious -CaseName $failureCase.Name `
            -ExpectedErrorPattern $failureCase.Expected -DownloadAsset $downloader
    }

    # Force failure on the second same-volume rename to exercise rollback after
    # the prior directory has already moved aside, then test an open executable.
    $normalDownloader = New-AssetDownloader -Mode 'normal' -ArchivePath $archiveLatest -ChecksumsPath $checksumsLatest
    Assert-InjectedPromotionFailureRollsBack -DownloadAsset $normalDownloader

    # Hold the existing executable without file sharing. The installer must
    # fail before moving its directory and preserve bytes, version and PATH.
    Assert-LockedReplacementFails -DownloadAsset $normalDownloader

    # Genuine package upgrade and repeat install; the path entry stays singular.
    & $installerPath -Version $latestVersion -InstallRoot $upgradeRoot
    Assert-InstalledVersion -InstallPath $upgradeRoot -ExpectedVersion $latestVersion
    $upgradeMarker = Get-Content -LiteralPath (Join-Path $upgradeRoot '.tzim-installer.json') -Raw | ConvertFrom-Json
    Assert-Equal -Expected $true -Actual ([bool]$upgradeMarker.path_entry_owned) -Message 'installer-owned PATH marker'
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $upgradeRoot) -Message 'upgrade retained a single PATH entry'
    & $installerPath -Version $latestVersion -InstallRoot $upgradeRoot
    Assert-Equal -Expected 1 -Actual (Get-UserPathMatchCount -InstallPath $upgradeRoot) -Message 'repeat upgrade did not duplicate PATH'
    & $installerPath -Uninstall -InstallRoot $upgradeRoot
    Assert-Equal -Expected 0 -Actual (Get-UserPathMatchCount -InstallPath $upgradeRoot) -Message 'uninstall removed only the owned PATH entry'
    Assert-Equal -Expected $initialUserPath -Actual ([Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)) -Message 'owned PATH cleanup restored unrelated entries'
    Assert-Equal -Expected $initialProcessPath -Actual $env:Path -Message 'owned PATH cleanup restored current process'
    Write-Host "PASS real 0.14.0 -> $latestVersion upgrade, rerun, lock failure rollback, and owned PATH uninstall"

    # Validate and install a local manifest built from the real latest published
    # ZIP. This uses winget's local --manifest path only; no community source is
    # added or queried for this package.
    $python = Get-Command -Name python -ErrorAction Stop
    $winget = Get-Command -Name winget.exe -ErrorAction Stop
    $wingetRoot = Join-Path $artifactRoot 'winget-local'
    $manifestDir = Join-Path $wingetRoot $latestVersion
    $generator = Join-Path $repositoryRoot 'packaging\winget\generate.py'
    $generateLog = Join-Path $artifactRoot 'winget-generate.log'
    $generation = & $python.Source $generator --tag $latestTag --windows-archive $archiveLatest --output-dir $wingetRoot 2>&1
    $generationExit = $LASTEXITCODE
    $generation | Tee-Object -FilePath $generateLog
    Assert-Equal -Expected 0 -Actual $generationExit -Message 'generate local winget manifests'

    $validateLog = Join-Path $artifactRoot 'winget-validate.log'
    $validation = & $winget.Source validate --manifest $manifestDir 2>&1
    $validationExit = $LASTEXITCODE
    $validation | Tee-Object -FilePath $validateLog
    Assert-Equal -Expected 0 -Actual $validationExit -Message 'winget validate --manifest local directory'

    $expectedDir = Join-Path $artifactRoot 'winget-expected-binary'
    Expand-Archive -LiteralPath $archiveLatest -DestinationPath $expectedDir -Force
    $expectedExeHash = (Get-FileHash -LiteralPath (Join-Path $expectedDir 'tzim.exe') -Algorithm SHA256).Hash
    $installLog = Join-Path $artifactRoot 'winget-install.log'
    $installOutput = & $winget.Source install --manifest $manifestDir --scope user `
        --accept-package-agreements --silent --disable-interactivity 2>&1
    $installExit = $LASTEXITCODE
    $installOutput | Tee-Object -FilePath $installLog
    Assert-Equal -Expected 0 -Actual $installExit -Message 'actual winget install --manifest'

    $userPathAfterWinget = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    if (-not [string]::IsNullOrEmpty($userPathAfterWinget)) {
        $env:Path = $userPathAfterWinget + ';' + $env:Path
    }
    $wingetTzim = Get-Command -Name tzim -CommandType Application -ErrorAction Stop
    $wingetExePath = $wingetTzim.Source
    $wingetVersion = Get-TzimArchiveVersion -ExecutablePath $wingetExePath
    $wingetExeHash = (Get-FileHash -LiteralPath $wingetExePath -Algorithm SHA256).Hash
    Assert-Equal -Expected $latestVersion -Actual $wingetVersion -Message 'winget-installed executable version'
    Assert-Equal -Expected $expectedExeHash -Actual $wingetExeHash -Message 'winget-installed executable SHA-256'
    Assert-Equal -Expected $initialMachinePath -Actual ([Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::Machine)) -Message 'installer and local winget left machine PATH unchanged'
    Write-Host "PASS winget local manifest install: $wingetExePath; version=$wingetVersion; executable SHA-256=$wingetExeHash"
    Write-Host "WINDOWS_INTEGRATION_EVIDENCE: artifact directory '$artifactRoot'; tested latest $latestTag and fixed v0.14.0 -> $latestTag upgrade."
}
finally {
    if ($installerFunctionsLoaded) {
        foreach ($path in @($latestRoot, $upgradeRoot)) {
            if ([System.IO.Directory]::Exists($path) -and (Test-Path -LiteralPath (Join-Path $path '.tzim-installer.json'))) {
                try {
                    Invoke-TzimUninstallCore -InstallRoot $path
                }
                catch {
                    Write-Warning "Cleanup could not remove test install '$path': $($_.Exception.Message)"
                }
            }
        }
    }
    if ([System.IO.Directory]::Exists($testBase)) {
        Remove-Item -LiteralPath $testBase -Recurse -Force -ErrorAction SilentlyContinue
    }
    [Environment]::SetEnvironmentVariable('Path', $initialUserPath, [EnvironmentVariableTarget]::User)
    $env:Path = $initialProcessPath
    Stop-Transcript | Out-Null
}
