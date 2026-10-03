//! Bounded, serial CPU preparation. No world or GPU device crosses this boundary.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreparationPriority {
    Prefetch,
    Scene,
    Visible,
}
/// Whether admission should be retried or reported as a permanent failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationError {
    Busy,
    Unavailable,
}
impl std::fmt::Display for PreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "asset preparation queue is full (64 jobs)",
            Self::Unavailable => "asset preparation worker unavailable",
        })
    }
}
impl std::error::Error for PreparationError {}
struct Job {
    priority: PreparationPriority,
    run: Box<dyn FnOnce() + Send>,
}
#[derive(Clone)]
pub(crate) struct PreparationQueue {
    tx: mpsc::Sender<Job>,
    pending: Arc<AtomicUsize>,
}
impl PreparationQueue {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let pending = Arc::new(AtomicUsize::new(0));
        let count = pending.clone();
        let _worker = std::thread::Builder::new()
            .name("fynkel-prepare".into())
            .spawn(move || {
                let mut jobs = Vec::new();
                while let Ok(job) = rx.recv() {
                    jobs.push(job);
                    jobs.extend(rx.try_iter());
                    while !jobs.is_empty() {
                        // Stable sorting retains FIFO within a priority.
                        jobs.sort_by_key(|j| std::cmp::Reverse(j.priority));
                        let job = jobs.remove(0);
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.run));
                        count.fetch_sub(1, Ordering::Release);
                        jobs.extend(rx.try_iter());
                    }
                }
            });
        Self { tx, pending }
    }
    pub(crate) fn submit(
        &self,
        priority: PreparationPriority,
        run: impl FnOnce() + Send + 'static,
    ) -> Result<(), PreparationError> {
        self.pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 64).then_some(n + 1)
            })
            .map_err(|_| PreparationError::Busy)?;
        if self
            .tx
            .send(Job {
                priority,
                run: Box::new(run),
            })
            .is_err()
        {
            self.pending.fetch_sub(1, Ordering::Release);
            return Err(PreparationError::Unavailable);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bounded_admission_priorities_fifo_and_panic_recovery() {
        let queue = PreparationQueue::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        queue
            .submit(PreparationPriority::Scene, move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (tx, rx) = mpsc::channel();
        for i in 0..63 {
            let tx = tx.clone();
            let priority = if i % 2 == 0 {
                PreparationPriority::Prefetch
            } else {
                PreparationPriority::Visible
            };
            queue
                .submit(priority, move || {
                    tx.send(i).unwrap();
                })
                .unwrap();
        }
        assert_eq!(
            queue.submit(PreparationPriority::Visible, || {}),
            Err(PreparationError::Busy)
        );
        release_tx.send(()).unwrap();
        let actual: Vec<_> = (0..63)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        let expected: Vec<_> = (0..63)
            .filter(|i| i % 2 == 1)
            .chain((0..63).filter(|i| i % 2 == 0))
            .collect();
        assert_eq!(actual, expected);
        queue
            .submit(PreparationPriority::Scene, || panic!("test job panic"))
            .unwrap();
        queue
            .submit(PreparationPriority::Scene, move || {
                tx.send(99).unwrap();
            })
            .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 99);
    }
}
