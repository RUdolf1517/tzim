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
//! ⠹ 01:12 [████████▏···]  37/148  6.2/s  eta 00:18
//! ```
//!
//! A single file is a job with no counter: the spinner plus the verb says
//! everything there is to say. A batch gets the clock, the bar, the counter, the
//! rate and an estimate, in that order, so the eye finds the position without
//! reading a sentence.
//!
//! The line deliberately contains no path. A path makes the line as wide as the
//! deepest directory in the tree, wraps in narrow terminals, and turns a status
//! indicator into a source of noise.
//!
//! **No dependency draws this.** A bar is arithmetic: an eighth-block character
//! for the last partial cell, `·` for the empty ones, a carriage return and one
//! write. The three things a library was paying 92–96 KiB for — a clock, the
//! width of a non-ASCII glyph, and a line that leaves the cursor where the report
//! expects it — are [`clock`], [`crate::report::display_width`] and [`erase`],
//! and all three are in this repository already (#53).
//!
//! Two consequences of doing it by hand, stated rather than implied:
//!
//! - **Width.** There is no portable ioctl in safe Rust without a crate, so the
//!   terminal width is `COLUMNS` when it is set and 80 when it is not, and the
//!   line is built to fit. `docs/cli.md` §10 says so.
//! - **Drawing.** One thread draws and the workers only bump a counter, so no
//!   worker ever touches the terminal and no lock is held while writing.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::report::display_width;
use crate::style::{Pen, Style};

/// Ten frames of a braille spinner. Braille looks better than `|/-\` and is the
/// one thing here that needs a font with U+2800 — which every terminal written
/// since 1990 has, and which a redirected stream never sees.
const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// How often the line is redrawn — one frame per tick, which is the animation
/// and the redraw cap at once: ten times a second, exactly.
const TICK: Duration = Duration::from_millis(100);

/// How long the run may last before the first frame appears. A run that finishes
/// in 40 ms should not flash a line at all.
const FIRST: Duration = Duration::from_millis(150);

/// An estimate before three seconds is a guess, and a wrong guess on a one-second
/// job is worse than no estimate.
const ETA_AFTER: Duration = Duration::from_secs(3);

/// The moving average takes the last few completions, not the mean of the whole
/// run: one huge file at the start must not make the estimate lie for the rest.
const WINDOW: usize = 8;

/// Eight cells: narrow enough for an 80-column terminal, wide enough that a tree
/// of hundreds of files visibly moves.
const CELLS: usize = 8;

/// One full cell, the empty cell, and the fractions of a cell that make an
/// eight-cell bar move in 64 steps instead of 8.
const FULL: char = '█';
const EMPTY: char = '·';
const PARTIAL: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

/// Width to assume when `COLUMNS` is missing or nonsense. 80 is the terminal
/// every other default in this program assumes.
const DEFAULT_WIDTH: usize = 80;

/// Elapsed time as `mm:ss`.
fn clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// Where the bar stands, in eighths of a cell, from 0 to `CELLS * 8`.
///
/// Integer arithmetic and a single division: `finished * cells * 8 / total`.
/// A finished run is pinned to the last position rather than left to rounding,
/// because "148/148" with a bar one eighth short reads as a bug.
fn bar_position(finished: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    if finished >= total {
        return CELLS * 8;
    }
    finished * CELLS * 8 / total
}

/// The bar, already painted. Full cells green, the partial cell bright, the
/// empty ones dim — the same three attributes the report uses.
fn bar(finished: usize, total: usize, style: Style) -> String {
    let position = bar_position(finished, total);
    let mut full = String::with_capacity(CELLS);
    let mut empty = String::with_capacity(CELLS);
    let mut out = String::with_capacity(CELLS * 4 + 16);

    let whole = position / 8;
    let part = position % 8;
    for _ in 0..whole {
        full.push(FULL);
    }
    // The partial cell belongs to the filled side when there is any ink in it.
    if part > 0 && whole < CELLS {
        full.push(PARTIAL[part]);
    }
    for _ in (whole + usize::from(part > 0 && whole < CELLS))..CELLS {
        empty.push(EMPTY);
    }

    style.paint_into(&mut out, Pen::Green, &full);
    style.paint_into(&mut out, Pen::Dim, &empty);
    out
}

/// Files per second over the last few completions, one decimal.
///
/// `None` before two completions: one sample is not a rate, it is a duration.
fn rate(recent: &VecDeque<Instant>) -> Option<f64> {
    if recent.len() < 2 {
        return None;
    }
    let first = *recent.front()?;
    let last = *recent.back()?;
    let seconds = last.duration_since(first).as_secs_f64();
    if seconds <= 0.0 {
        return None;
    }
    Some((recent.len() - 1) as f64 / seconds)
}

/// Everything one frame of the line is built from. Split out from the running
/// state so the composition can be tested without sleeping.
#[derive(Clone)]
pub struct Frame {
    pub started: Instant,
    pub now: Instant,
    pub finished: usize,
    pub total: usize,
    /// Completion instants of the last [`WINDOW`] files, oldest first.
    pub recent: VecDeque<Instant>,
    pub style: Style,
    /// Frame index into [`FRAMES`]; the ticker keeps it.
    pub tick: usize,
}

impl Frame {
    /// The whole line, without a leading `\r` and without a trailing newline.
    ///
    /// The optional tail is dropped in order of how little it is worth when the
    /// terminal is narrow: the estimate first, then the rate. Nothing is ever
    /// truncated in the middle of a glyph, and the result never buys a wrap.
    pub fn render(&self, width: usize) -> String {
        let elapsed = self.now.duration_since(self.started);
        let mut line = String::with_capacity(width.min(160));

        if self.total <= 1 {
            self.style.paint_into(
                &mut line,
                Pen::Green,
                &FRAMES[self.tick % FRAMES.len()].to_string(),
            );
            line.push_str(" converting…");
            return clip(line, width);
        }

        let spinner = FRAMES[self.tick % FRAMES.len()].to_string();
        self.style.paint_into(&mut line, Pen::Green, &spinner);
        line.push(' ');
        self.style.paint_into(&mut line, Pen::Dim, &clock(elapsed));
        line.push(' ');
        line.push('[');
        line.push_str(&bar(self.finished, self.total, self.style));
        line.push_str("]  ");

        let counter = format!("{}/{}", self.finished, self.total);
        self.style.paint_into(&mut line, Pen::Bold, &counter);

        let rate_text = rate(&self.recent).map(|r| format!("  {r:.1}/s"));
        let eta_text = rate(&self.recent)
            .filter(|_| elapsed >= ETA_AFTER)
            .and_then(|r| {
                let remaining = (self.total - self.finished) as f64;
                let seconds = remaining / r;
                seconds
                    .is_finite()
                    .then(|| format!("  eta {}", clock(Duration::from_secs_f64(seconds.max(0.0)))))
            });

        for tail in [eta_text, rate_text].into_iter().flatten() {
            let candidate = format!("{line}{tail}");
            if display_width(&candidate) <= width {
                line = candidate;
            }
        }
        clip(line, width)
    }
}

/// Truncates to `width` display cells, counting a combining mark as zero.
///
/// The last resort, for a width narrower than the counter alone. Wide glyphs are
/// never split: a cell that does not fit is dropped whole, and the SGR sequences
/// are reset before the cut so a truncated line cannot leak colour into the
/// report.
fn clip(line: String, width: usize) -> String {
    if display_width(&line) <= width {
        return line;
    }
    // Colour is decoration; a truncated line is easier to read plain.
    let plain: String = strip_sgr(&line);
    let mut out = String::with_capacity(width);
    let mut used = 0;
    for c in plain.chars() {
        let w = display_width(&c.to_string());
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

/// Removes `ESC [ ... m` sequences. Only the ones this program writes exist, and
/// they are all SGR, so there is no state machine to get wrong.
fn strip_sgr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('\u{1b}') {
        out.push_str(&rest[..start]);
        match rest[start..].find('m') {
            Some(end) => rest = &rest[start + end + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The terminal width, in cells: `COLUMNS` when it is a plausible number, 80
/// otherwise. There is no ioctl here on purpose — see the module comment.
pub fn width_from_env() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|w| *w >= 20)
        .unwrap_or(DEFAULT_WIDTH)
}

/// A live progress line. Dropping it stops the animation and erases the line.
pub struct Progress {
    /// `None` when the line is disabled or the run has nothing to count.
    shared: Option<Arc<Shared>>,
    ticker: Option<JoinHandle<()>>,
}

struct Shared {
    started: Instant,
    total: usize,
    finished: AtomicUsize,
    done: AtomicBool,
    /// Completion instants, oldest first, capped at [`WINDOW`]. The only lock in
    /// this module, held for a push and a pop.
    recent: Mutex<VecDeque<Instant>>,
    /// Cells written by the last frame, so the next one can erase the excess
    /// with spaces instead of leaving a tail behind.
    painted: AtomicUsize,
    style: Style,
}

impl Progress {
    /// `enabled` is the caller's decision; `total` only chooses the shape.
    /// `style` decides whether the spinner and the bar carry SGR sequences —
    /// the same decision, made once, that the report uses.
    pub fn new(total: usize, enabled: bool, style: Style) -> Progress {
        if !enabled || total == 0 {
            return Progress {
                shared: None,
                ticker: None,
            };
        }
        let shared = Arc::new(Shared {
            started: Instant::now(),
            total,
            finished: AtomicUsize::new(0),
            done: AtomicBool::new(false),
            recent: Mutex::new(VecDeque::with_capacity(WINDOW)),
            painted: AtomicUsize::new(0),
            style,
        });

        // The delay lives on its own thread rather than in the worker path: the
        // first frame must not wait for a file, and a 40 ms run must not draw.
        let state = Arc::clone(&shared);
        let ticker = thread::spawn(move || {
            thread::sleep(FIRST);
            if state.done.load(Ordering::Relaxed) || !std::io::stderr().is_terminal() {
                return;
            }
            let mut tick = 0usize;
            while !state.done.load(Ordering::Relaxed) {
                draw(&state, tick);
                tick = tick.wrapping_add(1);
                thread::sleep(TICK);
            }
        });

        Progress {
            shared: Some(shared),
            ticker: Some(ticker),
        }
    }

    /// Records one finished file. Called from worker threads, which is why it
    /// touches nothing but an atomic and a short-lived lock.
    pub fn advance(&self) {
        let Some(state) = &self.shared else {
            return;
        };
        state.finished.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut recent) = state.recent.lock() {
            recent.push_back(Instant::now());
            while recent.len() > WINDOW {
                recent.pop_front();
            }
        }
    }

    /// True when a line will actually be drawn. Used by tests and by nothing
    /// else.
    pub fn is_live(&self) -> bool {
        self.shared.is_some()
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let Some(state) = &self.shared else {
            return;
        };
        // Stops the ticker before the report prints: a frame written after the
        // first result would land in the middle of it.
        state.done.store(true, Ordering::Relaxed);
        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }
        erase(&state);
    }
}

/// One frame: compose, then write once with a carriage return in front.
///
/// Errors are swallowed on purpose. A terminal that went away is not a reason to
/// fail a run that may have optimised a thousand files, and this is the same rule
/// the report applies to a closed pipe.
fn draw(state: &Shared, tick: usize) {
    let frame = Frame {
        started: state.started,
        now: Instant::now(),
        finished: state.finished.load(Ordering::Relaxed),
        total: state.total,
        recent: state.recent.lock().map(|r| r.clone()).unwrap_or_default(),
        style: state.style,
        tick,
    };
    let width = width_from_env();
    let line = frame.render(width);
    let cells = display_width(&line);
    let previous = state.painted.swap(cells, Ordering::Relaxed);
    let padding = " ".repeat(previous.saturating_sub(cells));

    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = write!(handle, "\r{line}{padding}");
    let _ = handle.flush();
}

/// Blank the line so the report starts on an empty row.
fn erase(state: &Shared) {
    let painted = state.painted.swap(0, Ordering::Relaxed);
    if painted == 0 {
        return;
    }
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = write!(handle, "\r{}\r", " ".repeat(painted));
    let _ = handle.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(finished: usize, total: usize, elapsed: Duration) -> Frame {
        let started = Instant::now();
        Frame {
            started,
            now: started + elapsed,
            finished,
            total,
            recent: VecDeque::new(),
            style: Style::plain(),
            tick: 0,
        }
    }

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
    fn the_spinner_has_ten_frames() {
        assert_eq!(FRAMES.len(), 10);
        assert_eq!(FRAMES[0], '⠋');
    }

    /// The acceptance criterion for the bar: every one of the 64 positions of an
    /// eight-cell bar, including both ends.
    #[test]
    fn the_bar_has_sixty_four_positions() {
        let total = CELLS * 8;
        let mut seen = Vec::new();
        for finished in 0..=total {
            let position = bar_position(finished, total);
            assert_eq!(position, finished, "one file, one eighth");
            seen.push(position);
        }
        assert_eq!(seen.first(), Some(&0));
        assert_eq!(seen.last(), Some(&(CELLS * 8)));
    }

    #[test]
    fn an_empty_run_is_an_empty_bar_and_a_finished_run_is_full() {
        assert_eq!(bar_position(0, 148), 0);
        assert_eq!(bar(0, 148, Style::plain()), EMPTY.to_string().repeat(CELLS));
        assert_eq!(
            bar(148, 148, Style::plain()),
            FULL.to_string().repeat(CELLS)
        );
        // A finished run is pinned to the end rather than left to rounding.
        assert_eq!(bar_position(148, 148), CELLS * 8);
        assert_eq!(bar_position(1000, 148), CELLS * 8);
        // Nothing to count is not a division by zero.
        assert_eq!(bar_position(0, 0), 0);
    }

    #[test]
    fn the_partial_cell_is_the_right_eighth() {
        // 1/64 of the bar is one eighth of the first cell.
        assert_eq!(
            bar(1, 64, Style::plain()),
            format!("{}{}", PARTIAL[1], EMPTY.to_string().repeat(CELLS - 1))
        );
        // 4/64 is a half cell.
        assert_eq!(
            bar(4, 64, Style::plain()),
            format!("{}{}", PARTIAL[4], EMPTY.to_string().repeat(CELLS - 1))
        );
        // And a whole cell is a whole cell, not an eighth of the next one.
        assert_eq!(
            bar(8, 64, Style::plain()),
            format!("{}{}", FULL, EMPTY.to_string().repeat(CELLS - 1))
        );
    }

    #[test]
    fn the_bar_is_exactly_eight_cells_wide() {
        for finished in 0..=148 {
            let painted = bar(finished, 148, Style::plain());
            assert_eq!(painted.chars().count(), CELLS, "{finished}/148");
        }
    }

    #[test]
    fn a_single_file_says_converting_and_nothing_else() {
        let line = frame(0, 1, Duration::from_secs(2)).render(80);
        assert!(line.starts_with('⠋'), "{line}");
        assert!(line.ends_with("converting…"), "{line}");
        assert!(!line.contains('['), "a single file has no bar: {line}");
    }

    #[test]
    fn a_batch_shows_the_clock_the_bar_and_the_counter() {
        let line = frame(3, 12, Duration::from_secs(61)).render(80);
        assert!(line.contains("01:01"), "{line}");
        assert!(line.contains('[') && line.contains(']'), "{line}");
        assert!(line.contains("3/12"), "{line}");
    }

    #[test]
    fn the_estimate_needs_three_seconds_and_a_rate() {
        let started = Instant::now();
        let mut recent = VecDeque::new();
        for i in 0..4 {
            recent.push_back(started + Duration::from_millis(200 * i));
        }
        let early = Frame {
            started,
            now: started + Duration::from_secs(1),
            finished: 4,
            total: 40,
            recent: recent.clone(),
            style: Style::plain(),
            tick: 0,
        };
        assert!(!early.render(200).contains("eta"), "{}", early.render(200));

        let steady: Vec<Frame> = (0..6)
            .map(|i| Frame {
                started,
                now: started + Duration::from_secs(5 + i),
                finished: 4,
                total: 40,
                recent: recent.clone(),
                style: Style::plain(),
                tick: 0,
            })
            .collect();
        let first = steady[0].render(200);
        assert!(first.contains("eta"), "{first}");
        // A uniform run must not wobble: the same rate and the same remaining
        // count give the same estimate, second after second. The clock beside it
        // does move, which is the point of comparing only the estimate.
        let estimate = |line: &str| {
            line.find("eta")
                .map(|i| line[i..].to_string())
                .unwrap_or_default()
        };
        for frame in &steady[1..] {
            assert_eq!(estimate(&first), estimate(&frame.render(200)));
        }
    }

    #[test]
    fn the_rate_needs_two_completions() {
        let started = Instant::now();
        let mut recent = VecDeque::new();
        assert!(rate(&recent).is_none());
        recent.push_back(started);
        assert!(rate(&recent).is_none());
        recent.push_back(started + Duration::from_millis(500));
        assert!((rate(&recent).unwrap() - 2.0).abs() < 0.001);
    }

    #[test]
    fn the_line_never_exceeds_the_width_and_never_wraps() {
        for width in [20usize, 24, 40, 80, 200] {
            let line = frame(120, 148, Duration::from_secs(90)).render(width);
            assert!(
                display_width(&line) <= width,
                "width {width}: {line:?} is {} cells",
                display_width(&line)
            );
            assert!(!line.contains('\n'), "{line:?}");
            assert!(!line.contains('\r'), "{line:?}");
        }
    }

    #[test]
    fn a_narrow_line_drops_the_estimate_before_the_rate() {
        let started = Instant::now();
        let mut recent = VecDeque::new();
        for i in 0..4 {
            recent.push_back(started + Duration::from_millis(100 * i));
        }
        let wide = Frame {
            started,
            now: started + Duration::from_secs(5),
            finished: 4,
            total: 100,
            recent,
            style: Style::plain(),
            tick: 0,
        };
        assert!(wide.render(200).contains("eta"));
        assert!(!wide.render(30).contains("eta"), "{}", wide.render(30));
    }

    #[test]
    fn colour_changes_the_bytes_and_not_the_cells() {
        let started = Instant::now();
        let plain = Frame {
            started,
            now: started + Duration::from_secs(2),
            finished: 2,
            total: 10,
            recent: VecDeque::new(),
            style: Style::plain(),
            tick: 3,
        };
        let coloured = Frame {
            style: Style::new(true),
            ..plain.clone()
        };
        let a = plain.render(80);
        let b = coloured.render(80);
        assert_ne!(a, b, "colour must actually paint something");
        assert_eq!(display_width(&a), display_width(&b));
        assert!(!a.contains('\u{1b}'));
        assert!(b.contains("\u{1b}[32m"), "the bar or spinner must be green");
        assert_eq!(strip_sgr(&b), a, "the same line, minus the sequences");
    }

    #[test]
    fn nothing_is_drawn_when_disabled_or_empty() {
        assert!(!Progress::new(10, false, Style::plain()).is_live());
        assert!(!Progress::new(0, true, Style::plain()).is_live());
        assert!(!Progress::new(0, false, Style::plain()).is_live());
    }

    #[test]
    fn a_live_line_is_created_when_asked_for() {
        // stderr is not a terminal here, so nothing is painted: the decision is
        // what the caller reads, and the ticker goes away with the guard.
        let progress = Progress::new(12, true, Style::plain());
        assert!(progress.is_live());
        progress.advance();
        progress.advance();
    }

    #[test]
    fn advancing_without_a_line_is_harmless() {
        let progress = Progress::new(3, false, Style::plain());
        for _ in 0..5 {
            progress.advance();
        }
    }

    #[test]
    fn redrawing_is_capped_at_ten_frames_a_second() {
        assert!(
            TICK >= Duration::from_millis(100),
            "the ticker must not redraw more than ten times a second"
        );
        assert!(
            FIRST >= TICK,
            "the first frame must not arrive before a whole tick"
        );
    }

    #[test]
    fn the_width_comes_from_the_environment_when_it_is_plausible() {
        // The variable is process-wide and shared with other tests, so this only
        // asserts the parse rules through the same helpers the caller uses.
        assert!(width_from_env() >= 20);
    }
}
