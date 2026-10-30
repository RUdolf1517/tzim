[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [switch]$Uninstall,
    [string]$InstallRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12

$script:TzimRepository = 'RUdolf1517/tzim'
$script:TzimArchiveName = 'tzim-x86_64-pc-windows-msvc.zip'
$script:TzimChecksumsName = 'SHA256SUMS'
$script:TzimMarkerName = '.tzim-installer.json'
$script:TzimManagedFiles = @('tzim.exe', 'LICENSE', '.tzim-installer.json')
$script:TzimRedirectHosts = @(
    'github.com',
    'api.github.com',
    'release-assets.githubusercontent.com',
    'objects.githubusercontent.com',
    'github-releases.githubusercontent.com'
)

function ConvertTo-TzimTag {
    param([Parameter(Mandatory)][string]$Value)

    $tag = $Value.Trim()
    if ($tag.StartsWith('v', [System.StringComparison]::OrdinalIgnoreCase)) {
        $tag = 'v' + $tag.Substring(1)
    }
    else {
        $tag = 'v' + $tag
    }
    if ($tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') {
        throw "Invalid Tzim version '$Value'. Expected a stable version such as 0.15.0 or v0.15.0."
    }
    return $tag
}

function Assert-TzimX64Architecture {
    $architecture = [string]$env:PROCESSOR_ARCHITEW6432
    if ([string]::IsNullOrWhiteSpace($architecture)) {
        $architecture = [string]$env:PROCESSOR_ARCHITECTURE
    }
    if ($architecture -notmatch '^(AMD64|x64)$') {
        throw "Unsupported Windows architecture '$architecture'. This release provides only the x64 (AMD64) binary."
    }
}

function Get-TzimLatestReleaseTag {
    $uri = 'https://api.github.com/repos/' + $script:TzimRepository + '/releases/latest'
    $headers = @{
        Accept = 'application/vnd.github+json'
        'X-GitHub-Api-Version' = '2022-11-28'
        'User-Agent' = 'tzim-install.ps1'
    }
    $release = Invoke-RestMethod -Method Get -Uri $uri -Headers $headers
    if ($null -eq $release -or [string]::IsNullOrWhiteSpace([string]$release.tag_name)) {
        throw 'GitHub latest-release response did not contain tag_name.'
    }
    return (ConvertTo-TzimTag -Value ([string]$release.tag_name))
}

function Save-TzimUrl {
    param(
        [Parameter(Mandatory)][string]$Uri,
        [Parameter(Mandatory)][string]$Destination
    )

    $parsed = [System.Uri]$Uri
    if ($parsed.Scheme -ne 'https' -or $script:TzimRedirectHosts -cnotcontains $parsed.Host) {
        throw "Refusing an unexpected download URL: $Uri"
    }

    $headers = @{
        Accept = 'application/octet-stream'
        'User-Agent' = 'tzim-install.ps1'
    }
    $response = Invoke-WebRequest -Method Get -Uri $parsed.AbsoluteUri -Headers $headers `
        -OutFile $Destination -MaximumRedirection 5 -UseBasicParsing

    if (-not (Test-Path -LiteralPath $Destination -PathType Leaf)) {
        throw "GitHub returned no file for $Uri"
    }
    if ((Get-Item -LiteralPath $Destination).Length -le 0) {
        throw "GitHub returned an empty file for $Uri"
    }

    # Release downloads normally redirect to GitHub's release-asset CDN. Check
    # the final URI when the PowerShell version exposes it; never allow an
    # unexpected redirect host to become a new download source.
    $finalUri = $null
    if ($null -ne $response) {
        $baseProperty = $response.PSObject.Properties['BaseResponse']
        if ($null -ne $baseProperty -and $null -ne $baseProperty.Value) {
            $base = $baseProperty.Value
            $responseUriProperty = $base.PSObject.Properties['ResponseUri']
            if ($null -ne $responseUriProperty -and $null -ne $responseUriProperty.Value) {
                $finalUri = [System.Uri]$responseUriProperty.Value
            }
            if ($null -eq $finalUri) {
                $requestMessageProperty = $base.PSObject.Properties['RequestMessage']
                if ($null -ne $requestMessageProperty -and $null -ne $requestMessageProperty.Value) {
                    $requestUriProperty = $requestMessageProperty.Value.PSObject.Properties['RequestUri']
                    if ($null -ne $requestUriProperty -and $null -ne $requestUriProperty.Value) {
                        $finalUri = [System.Uri]$requestUriProperty.Value
                    }
                }
            }
        }
    }
    if ($null -ne $finalUri -and $script:TzimRedirectHosts -cnotcontains $finalUri.Host) {
        Remove-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
        throw "Refusing a GitHub redirect to an unexpected host: $($finalUri.Host)"
    }
}

function Get-TzimReleaseChecksum {
    param(
        [Parameter(Mandatory)][string]$ChecksumPath,
        [Parameter(Mandatory)][string]$AssetName
    )

    $lines = [System.IO.File]::ReadAllLines($ChecksumPath)
    if ($lines.Length -eq 0) {
        throw 'SHA256SUMS is empty.'
    }

    $entries = [System.Collections.Generic.Dictionary[string, object]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($line in $lines) {
        $match = [System.Text.RegularExpressions.Regex]::Match(
            $line,
            '^(?<hash>[0-9a-fA-F]{64})  \./(?<name>[A-Za-z0-9._-]+)$'
        )
        if (-not $match.Success) {
            throw "Malformed SHA256SUMS line: '$line'"
        }
        $name = $match.Groups['name'].Value
        if ($entries.ContainsKey($name)) {
            throw "SHA256SUMS contains duplicate entries for '$name'."
        }
        $entries[$name] = [pscustomobject]@{
            Name = $name
            Hash = $match.Groups['hash'].Value.ToLowerInvariant()
        }
    }

    if (-not $entries.ContainsKey($AssetName) -or $entries[$AssetName].Name -cne $AssetName) {
        throw "SHA256SUMS has no exact entry for '$AssetName'."
    }
    return [string]$entries[$AssetName].Hash
}

function Get-TzimArchiveVersion {
    param([Parameter(Mandatory)][string]$ExecutablePath)

    if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
        throw "The release archive is missing tzim.exe: $ExecutablePath"
    }
    $output = @(& $ExecutablePath --version 2>&1)
    $exitCode = $LASTEXITCODE
    $text = ($output | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne 0) {
        throw "tzim.exe --version exited with ${exitCode}: $text"
    }
    $versionMatch = [System.Text.RegularExpressions.Regex]::Match(
        $text.Trim(),
        '^tzim (?<version>[0-9]+\.[0-9]+\.[0-9]+)$'
    )
    if (-not $versionMatch.Success) {
        throw "Unexpected tzim.exe --version output: $text"
    }
    return $versionMatch.Groups['version'].Value
}

function Get-TzimInstallRoot {
    param([string]$RequestedPath)

    $profile = [Environment]::GetEnvironmentVariable('USERPROFILE')
    $localAppData = [Environment]::GetEnvironmentVariable('LOCALAPPDATA')
    if ([string]::IsNullOrWhiteSpace($profile) -or [string]::IsNullOrWhiteSpace($localAppData)) {
        throw 'USERPROFILE and LOCALAPPDATA must be set for a per-user installation.'
    }

    if ([string]::IsNullOrWhiteSpace($RequestedPath)) {
        $RequestedPath = Join-Path $localAppData 'Programs\tzim'
    }
    if (-not [System.IO.Path]::IsPathRooted($RequestedPath)) {
        throw 'InstallRoot must be an absolute path inside the current user profile.'
    }

    $fullPath = [System.IO.Path]::GetFullPath($RequestedPath).TrimEnd([char[]]@('\', '/'))
    $fullProfile = [System.IO.Path]::GetFullPath($profile).TrimEnd([char[]]@('\', '/'))
    $prefix = $fullProfile + [System.IO.Path]::DirectorySeparatorChar
    if (-not $fullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "InstallRoot must be inside the current user profile '$fullProfile'."
    }
    return $fullPath
}

function ConvertTo-TzimComparablePath {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return ''
    }
    $value = [Environment]::ExpandEnvironmentVariables($Path.Trim().Trim('"'))
    try {
        return [System.IO.Path]::GetFullPath($value).TrimEnd([char[]]@('\', '/'))
    }
    catch {
        return $value.TrimEnd([char[]]@('\', '/'))
    }
}

function Test-TzimPathEntry {
    param(
        [string]$Entry,
        [string]$InstallPath
    )

    $left = ConvertTo-TzimComparablePath -Path $Entry
    $right = ConvertTo-TzimComparablePath -Path $InstallPath
    return [string]::Equals($left, $right, [System.StringComparison]::OrdinalIgnoreCase)
}

function Add-TzimProcessPathEntry {
    param([string]$InstallPath)

    $parts = @(([string]$env:Path).Split([char]';'))
    foreach ($part in $parts) {
        if (Test-TzimPathEntry -Entry $part -InstallPath $InstallPath) {
            return
        }
    }
    if ([string]::IsNullOrEmpty($env:Path)) {
        $env:Path = $InstallPath
    }
    else {
        $env:Path = $env:Path + ';' + $InstallPath
    }
}

function Add-TzimUserPathEntry {
    param(
        [Parameter(Mandatory)][string]$InstallPath,
        [bool]$PreviouslyOwned
    )

    $rawPath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $parts = @(([string]$rawPath).Split([char]';'))
    foreach ($part in $parts) {
        if (Test-TzimPathEntry -Entry $part -InstallPath $InstallPath) {
            Add-TzimProcessPathEntry -InstallPath $InstallPath
            return $PreviouslyOwned
        }
    }

    if ([string]::IsNullOrEmpty($rawPath)) {
        $updatedPath = $InstallPath
    }
    else {
        $updatedPath = $rawPath + ';' + $InstallPath
    }
    [Environment]::SetEnvironmentVariable('Path', $updatedPath, [EnvironmentVariableTarget]::User)
    Add-TzimProcessPathEntry -InstallPath $InstallPath
    return $true
}

function Remove-TzimProcessPathEntry {
    param([string]$InstallPath)

    $parts = @(([string]$env:Path).Split([char]';'))
    $kept = New-Object 'System.Collections.Generic.List[string]'
    $removed = $false
    foreach ($part in $parts) {
        if (-not $removed -and (Test-TzimPathEntry -Entry $part -InstallPath $InstallPath)) {
            $removed = $true
            continue
        }
        $kept.Add($part)
    }
    if ($removed) {
        $env:Path = [string]::Join(';', $kept.ToArray())
    }
}

function Remove-TzimUserPathEntry {
    param([Parameter(Mandatory)][string]$InstallPath)

    $rawPath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $parts = @(([string]$rawPath).Split([char]';'))
    $kept = New-Object 'System.Collections.Generic.List[string]'
    $removed = $false
    foreach ($part in $parts) {
        if (-not $removed -and (Test-TzimPathEntry -Entry $part -InstallPath $InstallPath)) {
            $removed = $true
            continue
        }
        $kept.Add($part)
    }
    if ($removed) {
        [Environment]::SetEnvironmentVariable('Path', [string]::Join(';', $kept.ToArray()), [EnvironmentVariableTarget]::User)
    }
    Remove-TzimProcessPathEntry -InstallPath $InstallPath
}

function Write-TzimInstallerMarker {
    param(
        [Parameter(Mandatory)][string]$InstallPath,
        [Parameter(Mandatory)][string]$Tag,
        [Parameter(Mandatory)][bool]$PathEntryOwned
    )

    $marker = [ordered]@{
        marker_schema = 1
        installer = 'tzim-per-user'
        version = $Tag.Substring(1)
        tag = $Tag
        install_root = $InstallPath
        path_entry_owned = $PathEntryOwned
        managed_files = @($script:TzimManagedFiles)
    }
    $json = ConvertTo-Json -InputObject $marker -Depth 4
    $encoding = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText((Join-Path $InstallPath $script:TzimMarkerName), $json, $encoding)
}

function Read-TzimInstallerMarker {
    param([Parameter(Mandatory)][string]$InstallPath)

    $markerPath = Join-Path $InstallPath $script:TzimMarkerName
    if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) {
        throw "'$InstallPath' is not owned by this installer; no files were changed."
    }
    try {
        $marker = Get-Content -LiteralPath $markerPath -Raw | ConvertFrom-Json
    }
    catch {
        throw "Installer ownership marker is malformed; no files were changed: $($_.Exception.Message)"
    }
    if ($marker.marker_schema -ne 1 -or $marker.installer -ne 'tzim-per-user') {
        throw 'Installer ownership marker has an unsupported schema or owner; no files were changed.'
    }
    if (-not [string]::Equals(
        (ConvertTo-TzimComparablePath -Path ([string]$marker.install_root)),
        (ConvertTo-TzimComparablePath -Path $InstallPath),
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        throw 'Installer ownership marker names a different install path; no files were changed.'
    }
    $managed = @($marker.managed_files)
    if ($managed.Count -ne $script:TzimManagedFiles.Count) {
        throw 'Installer ownership marker has an unexpected managed-file list; no files were changed.'
    }
    foreach ($name in $script:TzimManagedFiles) {
        if ($managed -cnotcontains $name) {
            throw 'Installer ownership marker has an unexpected managed-file list; no files were changed.'
        }
    }
    if ($marker.path_entry_owned -isnot [bool]) {
        throw 'Installer ownership marker has invalid PATH ownership metadata; no files were changed.'
    }
    return $marker
}

function Assert-TzimOwnedDirectory {
    param([Parameter(Mandatory)][string]$InstallPath)

    foreach ($item in Get-ChildItem -LiteralPath $InstallPath -Force) {
        if ($script:TzimManagedFiles -cnotcontains $item.Name) {
            throw "Unmanaged item '$($item.Name)' exists in '$InstallPath'; refusing to replace or remove it."
        }
        if ($item.PSIsContainer) {
            throw "Managed path '$($item.Name)' is a directory; refusing to replace or remove it."
        }
    }
}

function Assert-TzimExecutableAvailable {
    param([Parameter(Mandatory)][string]$ExecutablePath)

    if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
        return
    }
    $lastError = $null
    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        $stream = $null
        try {
            # A read handle with no sharing detects an executable held open by
            # another process before any directory rename is attempted.
            $stream = [System.IO.File]::Open(
                $ExecutablePath,
                [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read,
                [System.IO.FileShare]::None
            )
            return
        }
        catch {
            $lastError = $_.Exception
            if ($attempt -lt 4) {
                Start-Sleep -Milliseconds 200
            }
        }
        finally {
            if ($null -ne $stream) {
                $stream.Dispose()
            }
        }
    }
    throw "Cannot replace '$ExecutablePath': it is open, locked, or unreadable. Close programs using Tzim and retry. $lastError"
}

function Test-TzimArchiveEntries {
    param([Parameter(Mandatory)][string]$ArchivePath)

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        $names = @($archive.Entries | ForEach-Object { $_.FullName })
        if ($names.Count -ne 2 -or $names -cnotcontains 'tzim.exe' -or $names -cnotcontains 'LICENSE') {
            throw "Unexpected Windows release archive entries: $($names -join ', ')"
        }
    }
    finally {
        $archive.Dispose()
    }
}

function Invoke-TzimInstallCore {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$InstallRoot,
        [scriptblock]$ResolveLatest = { Get-TzimLatestReleaseTag },
        [scriptblock]$DownloadAsset = {
            param($Uri, $Destination)
            Save-TzimUrl -Uri $Uri -Destination $Destination
        },
        [scriptblock]$MoveDirectory = {
            param($Source, $Destination)
            [System.IO.Directory]::Move($Source, $Destination)
        }
    )

    Assert-TzimX64Architecture
    $installPath = Get-TzimInstallRoot -RequestedPath $InstallRoot
    if ($Version -ieq 'latest') {
        $tag = ConvertTo-TzimTag -Value ([string](& $ResolveLatest))
    }
    else {
        $tag = ConvertTo-TzimTag -Value $Version
    }
    $releaseVersion = $tag.Substring(1)

    $parent = [System.IO.Directory]::GetParent($installPath).FullName
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
    $id = [guid]::NewGuid().ToString('N')
    $workPath = Join-Path $parent ('.tzim-work-' + $id)
    $stagePath = Join-Path $parent ('.tzim-stage-' + $id)
    $backupPath = Join-Path $parent ('.tzim-backup-' + $id)
    $downloadPath = Join-Path $workPath 'download'
    $extractPath = Join-Path $workPath 'extract'
    $stagedInstallPath = Join-Path $workPath 'install'

    $oldMoved = $false
    $newPromoted = $false
    $pathAttempted = $false
    $previousUserPath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $previousProcessPath = $env:Path
    $oldMarker = $null

    try {
        [System.IO.Directory]::CreateDirectory($downloadPath) | Out-Null
        [System.IO.Directory]::CreateDirectory($extractPath) | Out-Null
        [System.IO.Directory]::CreateDirectory($stagedInstallPath) | Out-Null

        $baseUri = 'https://github.com/' + $script:TzimRepository + '/releases/download/' + $tag + '/'
        $archiveUri = $baseUri + $script:TzimArchiveName
        $checksumsUri = $baseUri + $script:TzimChecksumsName
        $archivePath = Join-Path $downloadPath $script:TzimArchiveName
        $checksumsPath = Join-Path $downloadPath $script:TzimChecksumsName
        & $DownloadAsset $archiveUri $archivePath
        & $DownloadAsset $checksumsUri $checksumsPath

        $expectedHash = Get-TzimReleaseChecksum -ChecksumPath $checksumsPath -AssetName $script:TzimArchiveName
        $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if (-not [string]::Equals($actualHash, $expectedHash, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "SHA-256 mismatch for $script:TzimArchiveName (expected $expectedHash, received $actualHash)."
        }

        Test-TzimArchiveEntries -ArchivePath $archivePath
        Expand-Archive -LiteralPath $archivePath -DestinationPath $extractPath -Force
        $extractedVersion = Get-TzimArchiveVersion -ExecutablePath (Join-Path $extractPath 'tzim.exe')
        if ($extractedVersion -ne $releaseVersion) {
            throw "Version check failed: tag $tag contains tzim $extractedVersion, not $releaseVersion."
        }
        if (-not (Test-Path -LiteralPath (Join-Path $extractPath 'LICENSE') -PathType Leaf)) {
            throw 'The release archive is missing the exact LICENSE file.'
        }

        Copy-Item -LiteralPath (Join-Path $extractPath 'tzim.exe') -Destination $stagedInstallPath
        Copy-Item -LiteralPath (Join-Path $extractPath 'LICENSE') -Destination $stagedInstallPath

        $targetExists = Test-Path -LiteralPath $installPath
        if ($targetExists) {
            if (-not [System.IO.Directory]::Exists($installPath)) {
                throw "Install path exists and is not a directory: $installPath"
            }
            $oldMarker = Read-TzimInstallerMarker -InstallPath $installPath
            Assert-TzimOwnedDirectory -InstallPath $installPath
            Assert-TzimExecutableAvailable -ExecutablePath (Join-Path $installPath 'tzim.exe')
        }

        # The candidate directory lives beside the destination, so these are
        # same-volume directory renames. If promotion fails, the prior directory
        # is moved back before the original error is returned.
        if ($targetExists) {
            & $MoveDirectory $installPath $backupPath
            $oldMoved = $true
        }
        & $MoveDirectory $stagedInstallPath $installPath
        $newPromoted = $true

        $wasPathOwned = $false
        if ($null -ne $oldMarker) {
            $wasPathOwned = [bool]$oldMarker.path_entry_owned
        }
        $pathAttempted = $true
        $pathOwned = Add-TzimUserPathEntry -InstallPath $installPath -PreviouslyOwned $wasPathOwned

        $installedVersion = Get-TzimArchiveVersion -ExecutablePath (Join-Path $installPath 'tzim.exe')
        if ($installedVersion -ne $releaseVersion) {
            throw "Post-install version check failed: expected $releaseVersion, got $installedVersion."
        }
        Write-TzimInstallerMarker -InstallPath $installPath -Tag $tag -PathEntryOwned $pathOwned

        if ($oldMoved) {
            try {
                Remove-Item -LiteralPath $backupPath -Recurse -Force
                $oldMoved = $false
            }
            catch {
                Write-Warning "New installation works, but the owned backup remains at '$backupPath': $($_.Exception.Message)"
            }
        }
        Write-Host "Installed tzim $releaseVersion for the current user at '$installPath'."
        Write-Host 'Open a new terminal to pick up the persisted user PATH entry; the current PowerShell process was updated too.'
    }
    catch {
        $installFailure = $_
        $rollbackErrors = New-Object 'System.Collections.Generic.List[string]'

        if ($pathAttempted) {
            try {
                [Environment]::SetEnvironmentVariable('Path', $previousUserPath, [EnvironmentVariableTarget]::User)
                $env:Path = $previousProcessPath
            }
            catch {
                $rollbackErrors.Add("could not restore PATH: $($_.Exception.Message)")
            }
        }
        if ($newPromoted -and [System.IO.Directory]::Exists($installPath)) {
            try {
                Remove-Item -LiteralPath $installPath -Recurse -Force
                $newPromoted = $false
            }
            catch {
                $rollbackErrors.Add("could not remove the staged replacement at '$installPath': $($_.Exception.Message)")
            }
        }
        if ($oldMoved -and [System.IO.Directory]::Exists($backupPath)) {
            try {
                [System.IO.Directory]::Move($backupPath, $installPath)
                $oldMoved = $false
            }
            catch {
                $rollbackErrors.Add("the previous installation is preserved at '$backupPath' but could not be moved back: $($_.Exception.Message)")
            }
        }

        if ($rollbackErrors.Count -gt 0) {
            throw "Installation failed: $($installFailure.Exception.Message) Rollback details: $($rollbackErrors -join '; ')"
        }
        throw $installFailure
    }
    finally {
        foreach ($temporaryPath in @($workPath, $stagePath)) {
            if ([System.IO.Directory]::Exists($temporaryPath)) {
                Remove-Item -LiteralPath $temporaryPath -Recurse -Force -ErrorAction SilentlyContinue
            }
        }
    }
}

function Invoke-TzimUninstallCore {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$InstallRoot)

    $installPath = Get-TzimInstallRoot -RequestedPath $InstallRoot
    if (-not [System.IO.Directory]::Exists($installPath)) {
        Write-Host "No tzim installer-owned installation was found at '$installPath'."
        return
    }

    $marker = Read-TzimInstallerMarker -InstallPath $installPath
    $backupPath = Join-Path ([System.IO.Directory]::GetParent($installPath).FullName) ('.tzim-uninstall-' + [guid]::NewGuid().ToString('N'))
    $movedFiles = New-Object 'System.Collections.Generic.List[string]'
    $previousUserPath = [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]::User)
    $previousProcessPath = $env:Path
    $pathAttempted = $false

    try {
        foreach ($name in $script:TzimManagedFiles) {
            $filePath = Join-Path $installPath $name
            if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
                continue
            }
            if (-not [System.IO.Directory]::Exists($backupPath)) {
                [System.IO.Directory]::CreateDirectory($backupPath) | Out-Null
            }
            [System.IO.File]::Move($filePath, (Join-Path $backupPath $name))
            $movedFiles.Add($name)
        }

        if ([bool]$marker.path_entry_owned) {
            $pathAttempted = $true
            Remove-TzimUserPathEntry -InstallPath $installPath
        }

        if ((Get-ChildItem -LiteralPath $installPath -Force | Measure-Object).Count -eq 0) {
            [System.IO.Directory]::Delete($installPath)
        }
        try {
            if ([System.IO.Directory]::Exists($backupPath)) {
                Remove-Item -LiteralPath $backupPath -Recurse -Force
            }
        }
        catch {
            Write-Warning "Owned uninstall staging files remain at '$backupPath': $($_.Exception.Message)"
        }
        Write-Host "Uninstalled installer-owned tzim files from '$installPath'. Unrelated files and PATH entries were left alone."
    }
    catch {
        $uninstallFailure = $_
        $rollbackErrors = New-Object 'System.Collections.Generic.List[string]'
        if ($pathAttempted) {
            try {
                [Environment]::SetEnvironmentVariable('Path', $previousUserPath, [EnvironmentVariableTarget]::User)
                $env:Path = $previousProcessPath
            }
            catch {
                $rollbackErrors.Add("could not restore PATH: $($_.Exception.Message)")
            }
        }
        $movedNames = $movedFiles.ToArray()
        for ($index = $movedNames.Length - 1; $index -ge 0; $index--) {
            $name = $movedNames[$index]
            $savedFile = Join-Path $backupPath $name
            $originalFile = Join-Path $installPath $name
            if (Test-Path -LiteralPath $savedFile -PathType Leaf) {
                try {
                    [System.IO.File]::Move($savedFile, $originalFile)
                }
                catch {
                    $rollbackErrors.Add("could not restore '$name': $($_.Exception.Message)")
                }
            }
        }
        if ($rollbackErrors.Count -gt 0) {
            throw "Uninstall failed: $($uninstallFailure.Exception.Message) Rollback details: $($rollbackErrors -join '; ')"
        }
        throw $uninstallFailure
    }
}

function Invoke-TzimInstaller {
    param(
        [string]$RequestedVersion = 'latest',
        [string]$RequestedInstallRoot
    )

    $installPath = Get-TzimInstallRoot -RequestedPath $RequestedInstallRoot
    Invoke-TzimInstallCore -Version $RequestedVersion -InstallRoot $installPath
}

function Invoke-TzimUninstaller {
    param([string]$RequestedInstallRoot)

    $installPath = Get-TzimInstallRoot -RequestedPath $RequestedInstallRoot
    Invoke-TzimUninstallCore -InstallRoot $installPath
}

# Dot-sourcing exposes the private core functions to the Windows integration
# test harness for deterministic fault injection. The normal download-and-run
# interface accepts no alternate URLs or checksum bypasses.
if ($MyInvocation.InvocationName -ne '.') {
    if ($Uninstall) {
        Invoke-TzimUninstaller -RequestedInstallRoot $InstallRoot
    }
    else {
        Invoke-TzimInstaller -RequestedVersion $Version -RequestedInstallRoot $InstallRoot
    }
}
