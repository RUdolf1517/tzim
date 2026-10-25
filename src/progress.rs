//! The progress line on stderr.
//!
//! One line, redrawn in place, only when stderr is a terminal: an animation
//! written into a log file is a wall of `\r` that made nobody's build easier to
//! read. The decision is made by the caller ([`crate::main`] has the flags) and
//! is passed in, so this module stays testable without a terminal.
//!
//! The line deliberately contains no path. A path makes the line as wide as the
//! deepest directory in the tree, wraps in narrow terminals, and turns a status
//! indicator into a source of noise; the counters and the elapsed time are what
//! the animation is for. Frames are ASCII (`|/-\`) for the same reason the report
//! prints `->` instead of an arrow.
//!
//! The thread is not a runtime and not a pool: it exists for the duration of a
//! run, writes to stderr, and is joined when [`Progress`] is dropped — before the
//! report starts, so the report never shares a line with it.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::report::size;

/// Four frames, rotated on every tick. Braille spinners look better and are the
/// one thing that breaks on a terminal without a decent font.
const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

/// How often the line is redrawn, and how long the run may take before the first
/// frame appears. A run that finishes in 40 ms should not flash a spinner.
const TICK: Duration = Duration::from_millis(100);
const FIRST: Duration = Duration::from_millis(150);

struct Shared {
    done: AtomicUsize,
    saved: AtomicU64,
    stop: AtomicBool,
}

/// A live progress line. Dropping it stops the animation and erases the line.
pub struct Progress {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Progress {
    /// `enabled` is the caller's decision; `total` only shapes the text.
    pub fn new(total: usize, enabled: bool) -> Progress {
        let shared = Arc::new(Shared {
            done: AtomicUsize::new(0),
            saved: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let thread = if enabled && total > 0 {
            let state = Arc::clone(&shared);
            let started = Instant::now();
            Some(thread::spawn(move || animate(&state, total, started)))
        } else {
            None
        };
        Progress { shared, thread }
    }

    /// Records one finished file. Called from worker threads, hence atomics.
    pub fn advance(&self, saved: u64) {
        self.shared.done.fetch_add(1, Ordering::Relaxed);
        if saved > 0 {
            self.shared.saved.fetch_add(saved, Ordering::Relaxed);
        }
    }

    /// True when a line will actually be drawn. Used by tests and by nothing else.
    pub fn is_live(&self) -> bool {
        self.thread.is_some()
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
            // The cursor sits at the end of the line; go back and erase it, so
            // the summary is the first thing on the row.
            let mut err = io::stderr();
            let _ = write!(err, "\r\x1b[K");
            let _ = err.flush();
        }
    }
}

fn animate(shared: &Shared, total: usize, started: Instant) {
    thread::sleep(FIRST);
    let mut err = io::stderr();
    let mut frame = 0usize;
    while !shared.stop.load(Ordering::Relaxed) {
        let done = shared.done.load(Ordering::Relaxed).min(total);
        let saved = shared.saved.load(Ordering::Relaxed);
        let line = line_text(frame, done, total, saved, started.elapsed());
        if write!(err, "\r\x1b[K{line}").is_err() || err.flush().is_err() {
            // stderr is gone; the report will report it. Keep working.
            return;
        }
        frame = frame.wrapping_add(1);
        thread::sleep(TICK);
    }
}

/// The text of one frame. Pure, so the only thing that can be wrong about it is
/// the text itself.
pub fn line_text(frame: usize, done: usize, total: usize, saved: u64, elapsed: Duration) -> String {
    let mut out = String::with_capacity(48);
    out.push_str(FRAMES[frame % FRAMES.len()]);
    out.push(' ');
    out.push_str(&done.to_string());
    out.push('/');
    out.push_str(&total.to_string());
    out.push_str(" files");
    out.push_str("  ");
    out.push_str(&duration(elapsed));
    if saved > 0 {
        out.push_str("  ");
        out.push_str(&size(saved));
        out.push_str(" saved");
    }
    out
}

/// Seconds with two decimals, the same shape the summary uses.
fn duration(elapsed: Duration) -> String {
    let hundredths = (elapsed.as_micros() + 5_000) / 10_000;
    format!("{}.{:02} s", hundredths / 100, hundredths % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_has_counters_and_the_clock() {
        let text = line_text(0, 12, 694, 0, Duration::from_millis(1500));
        assert_eq!(text, "| 12/694 files  1.50 s");
    }

    #[test]
    fn the_frame_rotates_and_wraps() {
        let frames: Vec<String> = (0..6)
            .map(|i| line_text(i, 1, 2, 0, Duration::ZERO))
            .collect();
        assert!(frames[0].starts_with("| "));
        assert!(frames[1].starts_with("/ "));
        assert!(frames[2].starts_with("- "));
        assert!(frames[3].starts_with("\\ "));
        assert!(frames[4].starts_with("| "), "wraps around");
    }

    #[test]
    fn savings_appear_only_once_there_are_any() {
        assert!(!line_text(0, 1, 2, 0, Duration::ZERO).contains("saved"));
        let text = line_text(0, 1, 2, 2048, Duration::ZERO);
        assert!(text.ends_with("2.00 KiB saved"), "{text}");
    }

    #[test]
    fn a_line_stays_short() {
        // 40 characters is the width of a narrow terminal minus a cursor; a line
        // that fits everywhere cannot wrap and cannot leave debris behind.
        let text = line_text(0, 694, 694, 2_130_943, Duration::from_secs(3600));
        assert!(text.chars().count() <= 40, "{text}");
    }

    #[test]
    fn nothing_is_drawn_when_disabled_or_empty() {
        assert!(!Progress::new(10, false).is_live());
        assert!(!Progress::new(0, true).is_live());
        assert!(Progress::new(1, true).is_live());
    }

    #[test]
    fn advancing_without_a_thread_is_harmless() {
        let progress = Progress::new(3, false);
        progress.advance(10);
        progress.advance(0);
        drop(progress);
    }
}
