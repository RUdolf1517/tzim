//! The progress line on stderr.
//!
//! One line, redrawn in place, only when stderr is a terminal: an animation
//! written into a log file is a wall of `\r` that made nobody's build easier to
//! read. The decision is made by the caller ([`crate::main`] has the flags) and
//! is passed in, so what this module owns is the *shape* of the line, not the
//! question of whether to draw it.
//!
//! Two shapes, because the two runs are different things:
//!
//! ```text
//! ⠋ converting…
//! ⠋ 00:01 [████░░░░] 3/12
//! ```
//!
//! A single file is a job with no counter: the spinner plus the verb says
//! everything there is to say. A batch gets the clock, the bar and the counter,
//! in that order, so the eye finds the position without reading a sentence.
//!
//! The line deliberately contains no path. A path makes the line as wide as the
//! deepest directory in the tree, wraps in narrow terminals, and turns a status
//! indicator into a source of noise.
//!
//! The drawing is `indicatif`'s job. The reason is not laziness: a bar that
//! redraws from several worker threads has to be synchronised against the
//! terminal, has to know the width of a multi-byte glyph, and has to leave the
//! cursor where the report expects it. That is a library's problem, and this
//! module keeps the part that is ours — the templates, the clock, and the rule
//! that the line never appears in a pipe.

use std::fmt;
use std::fmt::Write as _;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use indicatif::style::ProgressTracker;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};

use crate::style::Style;

/// Ten frames of a braille spinner. Braille looks better than `|/-\` and is the
/// one thing here that needs a font with U+2800 — which every terminal written
/// since 1990 has, and which a redirected stream never sees.
const FRAMES: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";

/// How often the spinner advances, and how long the run may take before the
/// first frame appears. A run that finishes in 40 ms should not flash a line.
const TICK: Duration = Duration::from_millis(100);
const FIRST: Duration = Duration::from_millis(150);

/// One file: the verb is the whole message. `{t}` is the clock defined below.
const ONE_PLAIN: &str = "{spinner} converting…";
const ONE_COLOUR: &str = "{spinner:.green} converting…";
/// A batch: clock, bar, counter. Eight cells is narrow enough for an 80-column
/// terminal and wide enough to show progress on a tree of hundreds of files.
const BATCH_PLAIN: &str = "{spinner} {t} [{bar:8}] {pos}/{len}";
const BATCH_COLOUR: &str = "{spinner:.green} {t} [{bar:8.green/dim}] {pos}/{len}";

/// Elapsed time as `mm:ss`.
///
/// `indicatif` offers `{elapsed_precise}` (`00:00:01`) and `{elapsed}` (`1s`);
/// neither is the shape a one-line status wants after ten minutes. The tracker
/// holds its own start instant, so `write` stays a pure read of the clock.
fn clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

struct Clock(Instant);

impl ProgressTracker for Clock {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(Clock(self.0))
    }

    fn tick(&mut self, _state: &ProgressState, _now: Instant) {}

    fn reset(&mut self, _state: &ProgressState, _now: Instant) {}

    fn write(&self, _state: &ProgressState, w: &mut dyn fmt::Write) {
        let _ = write!(w, "{}", clock(self.0.elapsed()));
    }
}

/// The style for a run of `total` files, with or without colour.
///
/// Not a `Result`: both templates are constants, and a test parses them, so the
/// only way this can fail is a typo that never reaches a release.
fn frame_style(total: usize, colour: bool, started: Instant) -> ProgressStyle {
    let template = match (total > 1, colour) {
        (false, false) => ONE_PLAIN,
        (false, true) => ONE_COLOUR,
        (true, false) => BATCH_PLAIN,
        (true, true) => BATCH_COLOUR,
    };
    let style = ProgressStyle::with_template(template)
        .expect("the progress templates are constants, and a test parses them")
        .tick_chars(FRAMES)
        .with_key("t", Clock(started));
    if total > 1 {
        style.progress_chars("██░")
    } else {
        style
    }
}

/// A live progress line. Dropping it stops the animation and erases the line.
pub struct Progress {
    bar: ProgressBar,
    live: bool,
    /// Set when the run ends, so a line that was still waiting for its first
    /// frame never appears after the report has started printing.
    done: Arc<AtomicBool>,
}

impl Progress {
    /// `enabled` is the caller's decision; `total` only chooses the shape.
    /// `style` decides whether the spinner and the bar carry SGR sequences —
    /// the same decision, made once, that the report uses.
    pub fn new(total: usize, enabled: bool, style: Style) -> Progress {
        let done = Arc::new(AtomicBool::new(false));
        if !enabled || total == 0 {
            return Progress {
                bar: ProgressBar::hidden(),
                live: false,
                done,
            };
        }

        let started = Instant::now();
        let bar = ProgressBar::with_draw_target(
            (total > 1).then_some(total as u64),
            ProgressDrawTarget::hidden(),
        );
        bar.set_style(frame_style(total, style.is_enabled(), started));

        // The delay lives on its own thread rather than in the worker path: the
        // first frame must not wait for a file, and a 40 ms run must not draw.
        let reveal = bar.clone();
        let finished = Arc::clone(&done);
        thread::spawn(move || {
            thread::sleep(FIRST);
            if finished.load(Ordering::Relaxed) || !std::io::stderr().is_terminal() {
                return;
            }
            reveal.set_draw_target(ProgressDrawTarget::stderr());
            reveal.enable_steady_tick(TICK);
        });

        Progress {
            bar,
            live: true,
            done,
        }
    }

    /// Records one finished file. Called from worker threads; `indicatif` is
    /// synchronised internally, which is the reason it is here at all.
    pub fn advance(&self) {
        self.bar.inc(1);
    }

    /// True when a line will actually be drawn. Used by tests and by nothing
    /// else.
    pub fn is_live(&self) -> bool {
        self.live
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        // Erases the line: the report is the first thing on the row afterwards.
        self.bar.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_is_minutes_and_seconds() {
        assert_eq!(clock(Duration::ZERO), "00:00");
        assert_eq!(clock(Duration::from_millis(900)), "00:00");
        assert_eq!(clock(Duration::from_millis(1_900)), "00:01");
        assert_eq!(clock(Duration::from_secs(59)), "00:59");
        assert_eq!(clock(Duration::from_secs(60)), "01:00");
        assert_eq!(clock(Duration::from_secs(3_599)), "59:59");
        assert_eq!(clock(Duration::from_secs(3_600)), "60:00");
    }

    #[test]
    fn every_template_parses() {
        // `with_template` is the only fallible step, and it is fed constants, so
        // this test is what keeps the constants legal.
        for total in [1usize, 12] {
            for colour in [false, true] {
                let style = frame_style(total, colour, Instant::now());
                // The three shapes differ; an accidental swap would be caught by
                // the templates' own text, which is what the next test reads.
                assert_eq!(style.get_tick_str(0), "⠋");
            }
        }
    }

    #[test]
    fn a_batch_gets_a_bar_and_a_single_file_does_not() {
        assert!(BATCH_PLAIN.contains("{bar:8}"));
        assert!(BATCH_PLAIN.contains("{pos}/{len}"));
        assert!(BATCH_PLAIN.contains("{t}"));
        assert!(!ONE_PLAIN.contains("{bar"));
        assert!(ONE_PLAIN.ends_with("converting…"));
        // Colour is decoration: the same keys, plus styles.
        for (plain, colour) in [(ONE_PLAIN, ONE_COLOUR), (BATCH_PLAIN, BATCH_COLOUR)] {
            for key in ["{spinner}", "{t}"] {
                assert_eq!(plain.contains(key), colour.contains(key), "{key}");
            }
        }
    }

    #[test]
    fn the_spinner_has_ten_frames_and_a_final_one() {
        assert_eq!(FRAMES.chars().count(), 10);
        // `tick_chars` treats the last frame as the finished state, so it must
        // not be a frame the animation stops on mid-run.
        assert_eq!(FRAMES.chars().last(), Some('⠏'));
    }

    #[test]
    fn nothing_is_drawn_when_disabled_or_empty() {
        assert!(!Progress::new(10, false, Style::plain()).is_live());
        assert!(!Progress::new(0, true, Style::plain()).is_live());
        assert!(!Progress::new(0, false, Style::plain()).is_live());
    }

    #[test]
    fn a_live_line_is_created_when_asked_for() {
        // Not a terminal here, so nothing is painted: the draw target is
        // stderr, and `indicatif` hides a target that is not a terminal. The
        // test asserts the decision, which is what the caller reads.
        let progress = Progress::new(1, true, Style::plain());
        assert!(progress.is_live());
        progress.advance();
        let batch = Progress::new(12, true, Style::plain());
        assert!(batch.is_live());
        batch.advance();
        drop(batch);
    }

    #[test]
    fn advancing_without_a_line_is_harmless() {
        let progress = Progress::new(3, false, Style::plain());
        progress.advance();
        progress.advance();
        drop(progress);
    }
}
