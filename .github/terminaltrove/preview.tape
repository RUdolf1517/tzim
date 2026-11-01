Output tzim-terminaltrove.gif

Set Shell "bash"
Set FontSize 22
Set Width 1800
Set Height 900
Set Padding 24
Set TypingSpeed 40ms
Set Framerate 30
Set Theme "Catppuccin Mocha"
Set CursorBlink false

# Prepare an isolated copy and start a raw PTY transcript without recording it.
Hide
Type "mkdir -p $SESSION_DIR"
Enter
Type "cp fixture.png $SESSION_DIR/fixture.png"
Enter
Type "cd $SESSION_DIR"
Enter
Type "cp fixture.png sample.png"
Enter
Type "export NO_COLOR=1 PS1='tzim$ '"
Enter
Type "script --quiet --flush --return --command 'bash --noprofile --norc -i' $SESSION_LOG"
Enter
Sleep 1s
Type "export PS1='tzim$ '"
Enter
Type "set -euo pipefail"
Enter
Type "clear"
Enter

# Only the real release-binary commands and their output are visible.
Show
Sleep 750ms
Type "tzim --version"
Enter
Sleep 1s
Type "stat -c 'Before: %s bytes' sample.png"
Enter
Sleep 1s
Type "tzim --no-progress sample.png"
Enter
Sleep 3s
Type "stat -c 'After: %s bytes' sample.png"
Enter
Sleep 6s

# Close the nested shell after the final visible frame; do not expose setup.
Hide
Type "exit"
Enter
Sleep 500ms
