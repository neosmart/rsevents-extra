use rsevents::{Awaitable, ManualResetEvent, EventState, TimeoutError};
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub struct CountdownEvent {
    count: AtomicUsize,
    event: ManualResetEvent,
}

/// A countdown event is a special type of [`ManualResetEvent`] that makes it easy to wait for a
/// given number of tasks to complete asynchronously, and then carry out some action. A countdown
/// event is first initialized with a count equal to the number of outstanding tasks, and each time
/// a task is completed, [`CountdownEvent::tick()`] is called. A call to
/// [`CountdownEvent::wait()`](Awaitable::wait()) will block until all outstanding tasks have
/// completed and the internal counter reaches 0.
///
/// Countdown events are thread-safe and may be wrapped in an [`Arc`](std::sync::Arc) to easily
/// share across threads.
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

impl Awaitable for CountdownEvent {
    type T = ();
    type Error = TimeoutError;

    /// Waits for the internal countdown of the [`CountdownEvent`] to reach zero.
    fn try_wait(&self) -> Result<(), Infallible> {
        self.event.try_wait()
    }

    fn try_wait_for(&self, limit: Duration) -> Result<(), TimeoutError> {
        self.event.try_wait_for(limit)
    }

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
