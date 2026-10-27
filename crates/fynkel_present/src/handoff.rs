//! Something one thread draws with while another may take it away: the
//! Android surface. The render worker borrows it for each frame, and a
//! frame keeps it until it is presented, so two frames may hold it at once
//! (one being drawn, one waiting for the GPU); the platform thread replaces
//! it (a resize makes a new surface) or takes it away (the app went to the
//! background), and either waits until no frame is using it, because the
//! window under it may be released as soon as it returns. While it waits,
//! no new frame may borrow it.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

pub(crate) struct Handoff<T> {
    state: Mutex<State<T>>,
    /// Signalled when a lease ends.
    idle: Condvar,
}

struct State<T> {
    current: Option<Arc<T>>,
    /// [`Lease`]s out.
    leases: usize,
    /// Threads in [`Handoff::put`] or [`Handoff::take`] waiting for it.
    waiting: usize,
}

/// The current value, for one frame. While it lives, [`Handoff::put`] and
/// [`Handoff::take`] wait.
pub(crate) struct Lease<T> {
    handoff: Arc<Handoff<T>>,
    /// Always there until it drops, which lets go of it before it says
    /// it's done: the taker then holds the only reference.
    value: Option<Arc<T>>,
}

impl<T> Handoff<T> {
    pub(crate) fn new() -> Self {
        Self { state: Mutex::new(State { current: None, leases: 0, waiting: 0 }), idle: Condvar::new() }
    }

    fn state(&self) -> MutexGuard<'_, State<T>> {
        // Nothing panics while holding the lock.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits until no lease is out, then hands the lock back.
    fn idle(&self) -> MutexGuard<'_, State<T>> {
        let mut state = self.state();
        state.waiting += 1;
        while state.leases > 0 {
            state = self.idle.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.waiting -= 1;
        state
    }

    /// Makes `value` the current one, once no frame uses the old one, and
    /// returns the old one. Dropping it is then safe.
    pub(crate) fn put(&self, value: T) -> Option<Arc<T>> {
        self.idle().current.replace(Arc::new(value))
    }

    /// Takes the current value away, once no frame uses it.
    pub(crate) fn take(&self) -> Option<Arc<T>> {
        self.idle().current.take()
    }

    /// The current value for one frame, or `None` when there is none, or
    /// when another thread waits to put or take it: the leases out end,
    /// and no new one keeps it waiting.
    pub(crate) fn lease(self: &Arc<Self>) -> Option<Lease<T>> {
        let mut state = self.state();
        if state.waiting > 0 {
            return None;
        }
        let value = state.current.clone()?;
        state.leases += 1;
        Some(Lease { handoff: self.clone(), value: Some(value) })
    }
}

impl<T> Lease<T> {
    /// Whether another thread waits to put or take the value: the lease
    /// should end soon.
    // Only the Android target asks.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn wanted(&self) -> bool {
        self.handoff.state().waiting > 0
    }
}

impl<T> std::ops::Deref for Lease<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value.as_deref().expect("a lease has its value until it drops")
    }
}

impl<T> Drop for Lease<T> {
    fn drop(&mut self) {
        self.value = None;
        self.handoff.state().leases -= 1;
        self.handoff.idle.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    /// Stage 14's BR4: once `take` returns, a lease that just ended holds
    /// no reference, so the taker's is the only one (the iOS layer is
    /// unwrapped with `Arc::into_inner`).
    #[test]
    fn the_taker_holds_the_only_reference() {
        let rounds = 20_000;
        let mut shared = 0;
        for _ in 0..rounds {
            let handoff = Arc::new(Handoff::new());
            handoff.put(0u64);
            let lease = handoff.lease().unwrap();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let other = barrier.clone();
            let ending = thread::spawn(move || {
                other.wait();
                drop(lease);
            });
            barrier.wait();
            if Arc::into_inner(handoff.take().unwrap()).is_none() {
                shared += 1;
            }
            ending.join().unwrap();
        }
        assert_eq!(shared, 0, "{shared} of {rounds} takes still shared the value with a lease");
    }

    #[test]
    fn nothing_to_lease_until_a_value_is_put() {
        let handoff = Arc::new(Handoff::new());
        assert!(handoff.lease().is_none());
        assert!(handoff.put(1).is_none());
        assert_eq!(*handoff.lease().unwrap(), 1);
        assert_eq!(handoff.take().as_deref(), Some(&1));
        assert!(handoff.lease().is_none());
    }

    #[test]
    fn put_returns_the_old_value() {
        let handoff = Arc::new(Handoff::new());
        handoff.put(1);
        assert_eq!(handoff.put(2).as_deref(), Some(&1));
        assert_eq!(*handoff.lease().unwrap(), 2);
    }

    /// The point of it all: taking the surface away waits for the frame
    /// that is drawing with it.
    #[test]
    fn take_and_put_wait_for_the_frame_in_progress() {
        for replace in [false, true] {
            let handoff = Arc::new(Handoff::new());
            handoff.put(1);
            let (done, finished) = mpsc::channel();
            thread::scope(|scope| {
                let lease = handoff.lease().unwrap();
                assert!(!lease.wanted());
                scope.spawn(|| {
                    let old = if replace { handoff.put(2) } else { handoff.take() };
                    done.send(old.map(|old| *old)).unwrap();
                });
                // Still drawing: the other thread must still be waiting.
                assert!(finished.recv_timeout(Duration::from_millis(200)).is_err(), "didn't wait for the frame");
                assert!(lease.wanted(), "the frame can see it's waited for");
                drop(lease);
                assert_eq!(finished.recv_timeout(Duration::from_secs(5)).unwrap(), Some(1));
            });
            assert_eq!(handoff.lease().map(|lease| *lease), replace.then_some(2));
        }
    }

    /// Two frames may hold it, and taking it waits for both; meanwhile no
    /// new frame gets it.
    #[test]
    fn take_waits_for_every_frame_and_lends_no_more_meanwhile() {
        let handoff = Arc::new(Handoff::new());
        handoff.put(1);
        let (done, finished) = mpsc::channel();
        thread::scope(|scope| {
            let (first, second) = (handoff.lease().unwrap(), handoff.lease().unwrap());
            scope.spawn(|| done.send(handoff.take().map(|old| *old)).unwrap());
            while !first.wanted() {
                thread::yield_now();
            }
            assert!(handoff.lease().is_none(), "no new frame while it's waited for");
            drop(first);
            assert!(finished.recv_timeout(Duration::from_millis(200)).is_err(), "didn't wait for the second");
            drop(second);
            assert_eq!(finished.recv_timeout(Duration::from_secs(5)).unwrap(), Some(1));
        });
    }

    #[test]
    fn a_taken_value_is_dropped_by_whoever_took_it() {
        let handoff = Arc::new(Handoff::new());
        let value = Arc::new(());
        handoff.put(value.clone());
        drop(handoff.lease());
        let taken = handoff.take().unwrap();
        // The handoff holds no copy any more, and the lease left none.
        assert_eq!(Arc::strong_count(&taken), 1);
        drop(taken);
        assert_eq!(Arc::strong_count(&value), 1);
    }
}
