use rsevents::{Awaitable, ManualResetEvent, EventState, TimeoutError};
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// An `Awaitable` type that can be used to block until _n_ parallel tasks have completed.
///
/// A countdown event is a special type of [`ManualResetEvent`] that makes it easy to wait for a
/// given number of tasks to complete asynchronously, and then carry out some action. A countdown
/// event is first initialized with a count equal to the number of outstanding tasks, and each time
/// a task is completed, [`CountdownEvent::tick()`] is called. A call to [`CountdownEvent::wait()`]
/// will block until all outstanding tasks have completed and the internal counter reaches 0.
///
/// Countdown events are thread-safe and may be wrapped in an [`Arc`](std::sync::Arc) to easily
/// share across threads.
///
/// ## Example:
///
/// ```rust
/// use rsevents_extra::{Awaitable, CountdownEvent};
///
/// // CountdownEvent::new() is const and can be used directly in a static
/// // static context without needing lazy_static or once_cell:
/// static ALMOST_DONE: CountdownEvent = CountdownEvent::new(0);
///
/// fn worker_thread() {
///     for _ in 0..250 {
///         // Do something really important
///         // ...
///
///         // Each time we've finished a task, report our progress against
///         // the countdown event.
///         // Note that it's OK to keep calling this after the countdown
///         // event has already fired.
///         ALMOST_DONE.tick();
///     }
/// }
///
/// fn main() {
///     // Re-init the countdown event to fire after the first 750 tasks have
///     // been completed.
///     ALMOST_DONE.reset(750);
///
///     // Start 4 threads to begin work in parallel
///     std::thread::scope(|scope| {
///         for _ in 0..4 {
///             scope.spawn(worker_thread);
///         }
///
///         // Wait for the 750 tasks to be finished. This gives us more
///         // freedom than blocking until all threads have exited (as they may
///         // be long-lived and service many different tasks of different
///         // types, each of which we could track separately.)
///         ALMOST_DONE.wait();
///
///         eprintln!("Worker threads have almost finished! Hang tight!");
///     });
/// }
/// ```
pub struct CountdownEvent {
    count: AtomicUsize,
    event: ManualResetEvent,
}

impl CountdownEvent {
    /// Creates a new countdown event with the internal count initialized to `count`. If a count of
    /// zero is specified, the event is immediately set.
    pub const fn new(count: usize) -> Self {
        let result = Self {
            count: AtomicUsize::new(count),
            event: ManualResetEvent::new(if count == 0 { EventState::Set } else { EventState::Unset }),
        };

        result
    }

    /// Decrements the internal countdown. When the internal countdown reaches zero, the countdown
    /// event enters a [set](EventState::Set) state and any outstanding or future calls to
    /// [`Awaitable::wait()`] will be let through without blocking (until [the event is
    /// reset](CountdownEvent::reset())).
    pub fn tick(&self) {
        let old_ticks = self.count.fetch_sub(1, Ordering::Relaxed);
        if old_ticks == 1 {
            self.event.set();
        }
    }

    /// Resets a countdown event to the specified `count`. If a count of zero is specified, the
    /// countdown event is immediately set.
    ///
    /// Beware that unless you have a mutable reference to the countdown event, calls to `reset()`
    /// may race with calls to [`tick`](Self::tick) from any still-running threads with a reference
    /// to the countdown event!
    pub fn reset(&self, count: usize) {
        self.count.store(count, Ordering::Relaxed);
        if count == 0 {
            self.event.set();
        } else {
            self.event.reset();
        }
    }

    /// Get the current internal countdown value.
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

impl Awaitable<'_> for CountdownEvent {
    type T = ();
    type Error = TimeoutError;

    /// Waits for the internal countdown of the [`CountdownEvent`] to reach zero.
    fn try_wait(&self) -> Result<(), Infallible> {
        self.event.try_wait()
    }

    /// Waits for the internal countdown of the [`CountdownEvent`] to reach zero or returns an error
    /// in case of a timeout.
    fn try_wait_for(&self, limit: Duration) -> Result<(), TimeoutError> {
        self.event.try_wait_for(limit)
    }

    /// An optimized (wait-free, lock-free) check to see if the `CountdownEvent` has reached zero or
    /// not.
    fn try_wait0(&self) -> Result<(), TimeoutError> {
        self.event.try_wait0()
    }
}

#[test]
fn basic_countdown() {
    let countdown = CountdownEvent::new(1);
    assert_eq!(countdown.wait0(), false);
    countdown.tick();
    assert_eq!(countdown.wait0(), true);
}

#[test]
fn reset_countdown() {
    let countdown = CountdownEvent::new(1);
    assert_eq!(countdown.wait0(), false);
    countdown.tick();
    assert_eq!(countdown.wait0(), true);
    countdown.reset(1);
    assert_eq!(countdown.wait0(), false);
}

#[test]
fn start_at_zero() {
    let countdown = CountdownEvent::new(0);
    assert_eq!(countdown.wait0(), true);
}

#[test]
fn threaded_countdown() {
    use std::thread;

    static COUNTDOWN: CountdownEvent = CountdownEvent::new(2);

    assert_eq!(COUNTDOWN.wait0(), false);

    let thread1 = thread::spawn(move || {
        assert_eq!(COUNTDOWN.wait0(), false);
        COUNTDOWN.tick();
    });

    let thread2 = thread::spawn(move || {
        assert_eq!(COUNTDOWN.wait0(), false);
        COUNTDOWN.tick();
    });

    COUNTDOWN.wait();

    // To catch any panics
    thread1.join().unwrap();
    thread2.join().unwrap();
}
