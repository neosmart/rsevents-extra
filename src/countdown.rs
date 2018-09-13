use rsevents::{Awaitable, Event, ManualResetEvent, State};
use std::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};

pub struct CountdownEvent {
    count: AtomicUsize,
    event: ManualResetEvent,
}

impl CountdownEvent {
    pub fn new(count: usize) -> Self {
        let result = Self {
            count: ATOMIC_USIZE_INIT,
            event: ManualResetEvent::new(if count == 0 { State::Set } else { State::Unset }),
        };

        result.count.store(count, Ordering::Relaxed);

        result
    }

    fn tick(&self) {
        let old_ticks = self.count.fetch_sub(1, Ordering::Relaxed);
        if old_ticks == 1 {
            self.event.set();
        }
    }

    fn reset(&self, count: usize) {
        self.count.store(count, Ordering::Relaxed);
        self.event.reset();
    }
}

impl Awaitable for CountdownEvent {
    fn wait(&self) {
        self.event.wait()
    }

    fn wait0(&self) -> bool {
        self.event.wait0()
    }

    fn wait_for(&self, limit: Duration) -> bool {
        self.event.wait_for(limit)
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
    use std::sync::Arc;
    use std::thread;

    let countdown = Arc::new(CountdownEvent::new(2));
    assert_eq!(countdown.wait0(), false);

    let thread1 = {
        let countdown = countdown.clone();
        thread::spawn(move || {
            assert_eq!(countdown.wait0(), false);
            countdown.tick();
        })
    };

    let thread2 = {
        let countdown = countdown.clone();
        thread::spawn(move || {
            assert_eq!(countdown.wait0(), false);
            countdown.tick();
        })
    };

    countdown.wait();

    // To catch any panics
    thread1.join().unwrap();
    thread2.join().unwrap();
}
