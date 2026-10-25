//! Colour, decided once, from the flags, the environment and the terminal.
//!
//! There is no colour crate here: five SGR sequences and a `bool` are the whole
//! feature, and a dependency that writes `\x1b[32m` for us would cost more bytes
//! than the strings it hides. The decision, on the other hand, is the part worth
//! getting right, so it is a pure function of a [`Probe`] and is tested as one.
//!
//! The rules are the established ones, in this order:
//!
//! 1. `--color=never` / `--no-color` disables, `--color=always` enables. An
//!    explicit flag always wins, in both directions.
//! 2. `NO_COLOR` set to anything non-empty disables ([no-color.org]).
//! 3. `CLICOLOR_FORCE` set to anything but `0` enables, `CLICOLOR=0` disables.
//! 4. `TERM=dumb` disables.
//! 5. Otherwise colour is on only when the stream is a terminal — and, on
//!    Windows, only when that terminal is known to interpret ANSI sequences.
//!
//! [no-color.org]: https://no-color.org/

use std::env;

/// What the user asked for, if anything. `Auto` means "look at the terminal".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Auto,
    Always,
    Never,
}

impl Choice {
    /// Parses the value of `--color`. Only the three documented words are
    /// accepted: guessing what `--color=yes` means would be a second interface.
    pub fn parse(value: &str) -> Option<Choice> {
        match value {
            "auto" => Some(Choice::Auto),
            "always" => Some(Choice::Always),
            "never" => Some(Choice::Never),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Choice::Auto => "auto",
            Choice::Always => "always",
            Choice::Never => "never",
        }
    }
}

/// Everything the decision depends on, captured once.
///
/// Tests build this directly instead of mutating the process environment, which
/// they share with every other test in the binary.
#[derive(Debug, Default, Clone)]
pub struct Probe {
    pub tty: bool,
    pub no_color: bool,
    pub clicolor: Option<String>,
    pub clicolor_force: Option<String>,
    pub term: Option<String>,
    /// Whether the terminal is known to paint ANSI escapes. Always true off
    /// Windows, where the question does not arise.
    pub ansi_capable: bool,
}

impl Probe {
    /// Reads the environment. `tty` is not read here, because it is a property
    /// of a stream rather than of the process: stdout and stderr are probed
    /// separately.
    pub fn from_env(tty: bool) -> Probe {
        let set = |name: &str| env::var_os(name).map(|v| !v.is_empty()).unwrap_or(false);
        Probe {
            tty,
            no_color: set("NO_COLOR"),
            clicolor: env::var("CLICOLOR").ok(),
            clicolor_force: env::var("CLICOLOR_FORCE").ok(),
            term: env::var("TERM").ok(),
            ansi_capable: ansi_capable(),
        }
    }
}

/// Windows consoles before Windows 10 only paint ANSI escapes when the program
/// turns them on with a console call, which would need `unsafe` or a dependency.
/// Terminals that set one of these markers do understand the escapes, so colour
/// is offered there and withheld everywhere else rather than printing `[32m`
/// into somebody's `cmd.exe`. `--color=always` overrides this, as it overrides
/// everything else.
#[cfg(windows)]
fn ansi_capable() -> bool {
    let set = |name: &str| env::var_os(name).map(|v| !v.is_empty()).unwrap_or(false);
    set("WT_SESSION") || set("TERM") || set("ANSICON") || set("TERM_PROGRAM")
}

#[cfg(not(windows))]
fn ansi_capable() -> bool {
    true
}

/// The decision.
pub fn enabled(choice: Choice, probe: &Probe) -> bool {
    match choice {
        Choice::Never => false,
        Choice::Always => true,
        Choice::Auto => {
            if probe.no_color {
                return false;
            }
            let force = probe.clicolor_force.as_deref().unwrap_or("");
            if !force.is_empty() && force != "0" {
                return true;
            }
            if probe.clicolor.as_deref() == Some("0") {
                return false;
            }
            if probe.term.as_deref() == Some("dumb") {
                return false;
            }
            probe.tty && probe.ansi_capable
        }
    }
}

/// The few attributes this program uses. Colour marks an outcome, nothing else:
/// a status word, a saving, a failure. Headings and tables stay plain, so the
/// output still reads correctly when piped into a file or a pager.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pen {
    Bold,
    Dim,
    Red,
    Green,
    Yellow,
}

impl Pen {
    fn code(self) -> &'static str {
        match self {
            Pen::Bold => "1",
            Pen::Dim => "2",
            Pen::Red => "31",
            Pen::Green => "32",
            Pen::Yellow => "33",
        }
    }
}

/// A resolved decision plus the painting that follows from it.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    enabled: bool,
}

impl Style {
    pub fn new(enabled: bool) -> Style {
        Style { enabled }
    }

    /// No escape sequences, whatever the terminal says.
    pub fn plain() -> Style {
        Style { enabled: false }
    }

    pub fn is_enabled(self) -> bool {
        self.enabled
    }

    /// Appends `text` wrapped in `pen`, or exactly `text` when colour is off.
    ///
    /// Writing into an existing `String` keeps the report to one allocation per
    /// line whether or not colour is on.
    pub fn paint_into(self, out: &mut String, pen: Pen, text: &str) {
        if self.enabled {
            out.push_str("\x1b[");
            out.push_str(pen.code());
            out.push('m');
            out.push_str(text);
            out.push_str("\x1b[0m");
        } else {
            out.push_str(text);
        }
    }

    pub fn paint(self, pen: Pen, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 8);
        self.paint_into(&mut out, pen, text);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> Probe {
        Probe {
            tty: true,
            no_color: false,
            clicolor: None,
            clicolor_force: None,
            term: Some("xterm-256color".to_string()),
            ansi_capable: true,
        }
    }

    #[test]
    fn flags_win_over_everything() {
        let off = Probe {
            no_color: true,
            ..probe()
        };
        assert!(
            enabled(Choice::Always, &off),
            "an explicit yes beats NO_COLOR"
        );

        let on = Probe {
            clicolor_force: Some("1".to_string()),
            ..probe()
        };
        assert!(!enabled(Choice::Never, &on));
    }

    #[test]
    fn no_color_is_honoured_and_empty_means_nothing() {
        let set = Probe {
            no_color: true,
            ..probe()
        };
        assert!(!enabled(Choice::Auto, &set));
        // `NO_COLOR=` (empty) is not a request, per no-color.org.
        assert!(enabled(Choice::Auto, &probe()));
    }

    #[test]
    fn clicolor_force_beats_the_terminal_check() {
        let piped = Probe {
            tty: false,
            clicolor_force: Some("1".to_string()),
            ..probe()
        };
        assert!(enabled(Choice::Auto, &piped));
        let zero = Probe {
            clicolor_force: Some("0".to_string()),
            tty: false,
            ..probe()
        };
        assert!(!enabled(Choice::Auto, &zero), "0 means unset, not force");
    }

    #[test]
    fn clicolor_zero_and_dumb_terminals_disable() {
        let opted_out = Probe {
            clicolor: Some("0".to_string()),
            ..probe()
        };
        assert!(!enabled(Choice::Auto, &opted_out));

        let dumb = Probe {
            term: Some("dumb".to_string()),
            ..probe()
        };
        assert!(!enabled(Choice::Auto, &dumb));
    }

    #[test]
    fn a_pipe_gets_no_colour() {
        let piped = Probe {
            tty: false,
            ..probe()
        };
        assert!(!enabled(Choice::Auto, &piped));
    }

    #[test]
    fn a_console_without_ansi_gets_no_colour() {
        let legacy = Probe {
            ansi_capable: false,
            ..probe()
        };
        assert!(!enabled(Choice::Auto, &legacy));
    }

    #[test]
    fn only_three_words_are_commands() {
        assert_eq!(Choice::parse("auto"), Some(Choice::Auto));
        assert_eq!(Choice::parse("always"), Some(Choice::Always));
        assert_eq!(Choice::parse("never"), Some(Choice::Never));
        assert_eq!(Choice::parse("yes"), None);
        assert_eq!(Choice::parse(""), None);
        assert_eq!(Choice::parse("Always"), None);
    }

    #[test]
    fn painting_is_exact() {
        let on = Style::new(true);
        assert_eq!(on.paint(Pen::Red, "failed"), "\x1b[31mfailed\x1b[0m");
        let off = Style::plain();
        assert_eq!(off.paint(Pen::Red, "failed"), "failed");
        assert!(!off.is_enabled());
    }

    #[test]
    fn paint_into_accumulates() {
        let on = Style::new(true);
        let mut line = String::from("a");
        on.paint_into(&mut line, Pen::Green, "b");
        line.push('c');
        assert_eq!(line, "a\x1b[32mb\x1b[0mc");
    }
}
