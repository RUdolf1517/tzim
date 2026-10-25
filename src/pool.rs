use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use crate::report::{Outcome, Status};

/// Runs `work` over `files` with bounded parallelism and returns the outcomes in
/// discovery order, so output is deterministic regardless of timing.
///
/// `--fail-fast` and verification failures stop new files from starting; files
/// already in flight are allowed to finish, because aborting them mid-write is
/// exactly the behaviour this project refuses to have.
///
/// `on_done` is called once per finished file, from whichever thread finished it.
/// It exists for the progress line and must stay cheap and lock-free; the order
/// of its calls is not the order of the results.
pub fn run<F, G>(
    files: &[PathBuf],
    jobs: usize,
    fail_fast: bool,
    work: F,
    on_done: G,
) -> Vec<Outcome>
where
    F: Fn(&Path) -> Outcome + Sync,
    G: Fn(&Outcome) + Sync,
{
    let total = files.len();
    if total == 0 {
        return Vec::new();
    }

    if jobs <= 1 {
        let mut results = Vec::with_capacity(total);
        let mut aborted = false;
        for path in files {
            if aborted {
                let outcome = aborted_outcome(path);
                on_done(&outcome);
                results.push(outcome);
                continue;
            }
            let outcome = work(path);
            aborted = should_abort(&outcome, fail_fast);
            on_done(&outcome);
            results.push(outcome);
        }
        return results;
    }

    let workers = jobs.min(total);
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (tx, rx) = mpsc::channel::<(usize, Outcome)>();

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let next = &next;
            let stop = &stop;
            let work = &work;
            let on_done = &on_done;
            scope.spawn(move || loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= total {
                    return;
                }
                let outcome = work(&files[index]);
                on_done(&outcome);
                if should_abort(&outcome, fail_fast) {
                    stop.store(true, Ordering::Relaxed);
                }
                if tx.send((index, outcome)).is_err() {
                    return;
                }
            });
        }
        drop(tx);

        let mut slots: Vec<Option<Outcome>> = (0..total).map(|_| None).collect();
        for (index, outcome) in rx {
            slots[index] = Some(outcome);
        }

        slots
            .into_iter()
            .enumerate()
            .map(|(i, slot)| slot.unwrap_or_else(|| aborted_outcome(&files[i])))
            .collect()
    })
}

fn should_abort(outcome: &Outcome, fail_fast: bool) -> bool {
    if let Some(err) = &outcome.error {
        if err.is_verify() {
            return true;
        }
    }
    fail_fast && outcome.status == Status::Failed
}

fn aborted_outcome(path: &Path) -> Outcome {
    let mut outcome = Outcome::new(path.to_path_buf(), Status::Skipped, 0);
    outcome.note = Some("not processed: the run was aborted".to_string());
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(n: usize) -> Vec<PathBuf> {
        (0..n).map(|i| PathBuf::from(format!("f{i}"))).collect()
    }

    #[test]
    fn preserves_discovery_order() {
        let list = files(20);
        let out = run(
            &list,
            4,
            false,
            |p| Outcome::new(p.to_path_buf(), Status::AlreadyOptimal, 1),
            |_| {},
        );
        assert_eq!(out.len(), 20);
        for (i, o) in out.iter().enumerate() {
            assert_eq!(o.path, list[i]);
        }
    }

    #[test]
    fn sequential_mode_matches() {
        let list = files(5);
        let out = run(
            &list,
            1,
            false,
            |p| Outcome::new(p.to_path_buf(), Status::AlreadyOptimal, 1),
            |_| {},
        );
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn every_finished_file_is_announced_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let list = files(20);
        let seen = AtomicUsize::new(0);
        let out = run(
            &list,
            4,
            false,
            |p| Outcome::new(p.to_path_buf(), Status::AlreadyOptimal, 1),
            |_| {
                seen.fetch_add(1, Ordering::Relaxed);
            },
        );
        assert_eq!(out.len(), 20);
        assert_eq!(seen.load(Ordering::Relaxed), 20);
    }

    #[test]
    fn fail_fast_stops_new_work() {
        let list = files(8);
        let out = run(
            &list,
            1,
            true,
            |p| {
                let mut o = Outcome::new(p.to_path_buf(), Status::Failed, 1);
                o.error = Some(crate::error::Error::parse("boom"));
                o
            },
            |_| {},
        );
        // The first file fails; the rest are reported as not processed.
        assert_eq!(out[0].status, Status::Failed);
        assert!(out[1..].iter().all(|o| o.status == Status::Skipped));
    }
}
