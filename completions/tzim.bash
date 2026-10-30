# bash completion for tzim(1).
#
# Installed by the .deb, the .rpm and the Homebrew formula; not by install.sh,
# which puts one binary in one directory and nothing else.
#
# The option list is checked against `tzim --help` by tests/packaging.rs, so an
# option added to the parser without a line here fails CI.

_tzim() {
    local cur prev
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    case "$prev" in
        -j|--jobs|--min-size|--max-size|--max-pixels|--max-uncompressed|--max-ratio|--max-depth|--memory-budget)
            COMPREPLY=( $(compgen -W "0 1 2 4 8" -- "$cur") )
            return 0
            ;;
        --color)
            COMPREPLY=( $(compgen -W "auto always never" -- "$cur") )
            return 0
            ;;
        --formats)
            COMPREPLY=( $(compgen -W "png gzip zip ico jpeg tiff zlib fits flac pdf" -- "$cur") )
            return 0
            ;;
        --include|--exclude)
            COMPREPLY=( $(compgen -W "png apng gzip tgz zip jar ico cur jpeg jpg tif tiff zlib" -- "$cur") )
            return 0
            ;;
    esac

    if [[ "$cur" == -* ]]; then
        COMPREPLY=( $(compgen -W "--recursive --dry-run --jobs --json --quiet --color --no-color \
            --no-progress --fail-fast --include --exclude --min-size --max-size --max-pixels \
            --max-uncompressed --max-ratio --memory-budget --nested --max-depth --formats \
            --fits-tile --zopfli \
            --follow-symlinks --allow-hardlinks --allow-root-ownership-change --no-fsync \
            --strip-metadata --info --list-formats --check-deps --help --version \
            -r -n -j -q -i -h -V" -- "$cur") )
        return 0
    fi

    # Paths, which is the common case: the file is the command.
    COMPREPLY=( $(compgen -f -- "$cur") )
}

complete -o filenames -F _tzim tzim
