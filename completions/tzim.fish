# fish completion for tzim(1).
#
# Installed by the .deb, the .rpm and the Homebrew formula. The option list is
# checked against `tzim --help` by tests/packaging.rs.

complete -c tzim -f

complete -c tzim -s r -l recursive -d 'descend into directories'
complete -c tzim -s n -l dry-run -d 'do the work, write nothing'
complete -c tzim -s j -l jobs -x -d 'files processed at once'
complete -c tzim -l json -d 'newline-delimited JSON on stdout'
complete -c tzim -s q -l quiet -d 'suppress per-file output'
complete -c tzim -l color -x -a 'auto always never' -d 'colouring of the report'
complete -c tzim -l no-color -d 'same as --color=never'
complete -c tzim -l no-progress -d 'no progress line on stderr'
complete -c tzim -l fail-fast -d 'stop after the first failed file'
complete -c tzim -l include -x -d 'only these extensions'
complete -c tzim -l exclude -x -d 'skip these extensions'
complete -c tzim -l min-size -x -d 'skip smaller files'
complete -c tzim -l max-size -x -d 'skip larger files'
complete -c tzim -l max-pixels -x -d 'skip larger images'
complete -c tzim -l max-uncompressed -x -d 'refuse archives expanding beyond this'
complete -c tzim -l max-ratio -x -d 'refuse archives above this ratio'
complete -c tzim -l memory-budget -x -d 'skip files whose worst case exceeds this'
complete -c tzim -l nested -d 'also optimize files inside archives'
complete -c tzim -l max-depth -x -d 'nesting depth limit'
complete -c tzim -l formats -x -a 'png gzip zip ico jpeg tiff zlib fits flac pdf' -d 'only these formats'
complete -c tzim -l fits-tile -d 'FITS: rewrite an image HDU as a tile-compressed table'
complete -c tzim -l zopfli -d 'use the slower, stronger encoder'
complete -c tzim -l strip-metadata -d 'remove the documented metadata list'
complete -c tzim -l follow-symlinks -d 'resolve and follow symbolic links'
complete -c tzim -l allow-hardlinks -d 'process files with several hard links'
complete -c tzim -l allow-root-ownership-change -d 'replace files owned by others when root'
complete -c tzim -l no-fsync -d 'skip fsync before the rename'
complete -c tzim -s i -l info -d 'what this program is'
complete -c tzim -l list-formats -d 'the handlers in this build'
complete -c tzim -l check-deps -d 'external programs needed at run time'
complete -c tzim -s h -l help -d 'the full option list'
complete -c tzim -s V -l version -d 'the version, one line'

# With no option typed yet, complete paths: the file is the command.
complete -c tzim -n 'not string match -q -- "-*" (commandline -ct)' -F
