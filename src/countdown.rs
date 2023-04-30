use rsevents::{AutoResetEvent, Awaitable, EventState, ManualResetEvent, TimeoutError};
use std::convert::{Infallible, TryInto};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

/// An `Awaitable` type that can be used to block until _n_ parallel tasks have completed.
///
/// A countdown event is a special type of [`ManualResetEvent`] that makes it easy to wait for a
/// given number of tasks to complete asynchronously, and then carry out some action. A countdown
/// event is first initialized with a count equal to the number of outstanding tasks, and each time
/// a task is completed, [`CountdownEvent::tick()`] is called. A call to [`CountdownEvent::wait()`]
/// will block until all outstanding tasks have completed and the internal counter reaches 0.
///
/// Countdown events are thread-safe and may be declared as static variables or wrapped in an
/// [`Arc`](std::sync::Arc) to easily share across threads.
///
/// ## Example:
///
/// ```rust
/// use rsevents_extra::{Awaitable, CountdownEvent};
///
/// // CountdownEvent::new() is const and can be used directly in a static
/// // context without needing lazy_static or once_cell:
/// static ALMOST_DONE: CountdownEvent = CountdownEvent::new(0);
///
/// fn worker_thread() {
///     for _ in 0..250 {
///         // <Do something really important...>
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
    /// The internal count tracking the number of events left. While we could use an unsigned type
    /// and just wrap on under/overflow and that would be fine (since we only set the event in
    /// response to a `tick()` call and never reset it), it means calls to `CountdownEvent::count()`
    /// would report the overflow and we couldn't intercept it.
    count: AtomicIsize,
    /// The core synchronization event, waited on by calls to `wait()` but only accessed on the
    /// final call to `tick()`.
    event: ManualResetEvent,
    /// The event used to adjudicate disputes between calls to `reset()` or `increment()` coinciding
    /// with the final call to `tick()`.
    event2: AutoResetEvent,
}

impl CountdownEvent {
    /// Creates a new countdown event with the internal count initialized to `count`. If a count of
    /// zero is specified, the event is immediately set.
    ///
    /// This is a `const` function and can be used in a `static` context, (e.g. to declare a shared,
    /// static variable without using lazy_static or once_cell).
    pub const fn new(count: usize) -> Self {
        const MAX: usize = isize::MAX as usize;
        let count: isize = match count {
            0..=MAX => count as isize,
            _ => panic!("count cannot exceeed isize::MAX"),
        };

        let result = Self {
            count: AtomicIsize::new(count),
            event: ManualResetEvent::new(if count == 0 {
                EventState::Set
            } else {
                EventState::Unset
            }),
            event2: AutoResetEvent::new(EventState::Set),
        };

        result
    }

    /// Decrements the internal countdown. When the internal countdown reaches zero, the countdown
    /// event enters a [set](EventState::Set) state and any outstanding or future calls to
    /// [`CountdownEvent::wait()`] will be let through without blocking (until [the event is
    /// reset](CountdownEvent::reset()) [or incremented](Self::increment())).
    pub fn tick(&self) {
        let prev = self.count.fetch_sub(1, Ordering::Relaxed);
        if prev == 1 {
            self.event2.wait();
            if self.count.load(Ordering::Relaxed) == 0 {
                self.event.set();
            }
            self.event2.set();
        } else if prev == 0 {
            panic!("tick() called more times than outstanding jobs!");
        }
    }

    /// Increment the internal count (e.g. to add a work item).
    ///
    /// This resets the event (makes it unavailable) if the previous count was zero.
    pub fn increment(&self) {
        let prev = self.count.fetch_add(1, Ordering::Relaxed);
        if prev == 0 {
            self.event2.wait();
            if self.count.load(Ordering::Relaxed) == 0 {
                self.event.set();
            }
            self.event2.set();
        }
    }

    /// Resets a countdown event to the specified `count`. If a count of zero is specified, the
    /// countdown event is immediately set.
    pub fn reset(&self, count: usize) {
        let count: isize = match count.try_into() {
            Ok(count) => count,
            Err(_) => panic!("count cannot exceeed isize::MAX"),
        };

        self.count.store(count, Ordering::Relaxed);
        if self.count.load(Ordering::Relaxed) == 0 {
            self.event2.wait();
            if self.count.load(Ordering::Relaxed) == 0 {
                self.event.set();
            }
            self.event2.set();
            self.event.set();
        }
    }

    /// Get the current internal countdown value.
    pub fn count(&self) -> usize {
        match self.count.load(Ordering::Relaxed) {
            count @ 0.. => count as usize,
            _ => 0,
        }
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

#[test]
fn negative_countdown() {
    let countdown = CountdownEvent::new(1);
    assert_eq!(false, countdown.wait0());
    countdown.tick();
    assert_eq!(countdown.count(), 0);
    assert_eq!(true, countdown.wait0());
    countdown.tick();
    assert_eq!(countdown.count(), 0);
    assert_eq!(true, countdown.wait0());
}
