use std::sync::atomic::{AtomicU32, Ordering};
use std::convert::Infallible;
use std::time::Duration;
use rsevents::{Awaitable, EventState, AutoResetEvent, TimeoutError};

type Count = u32;
type AtomicCount = AtomicU32;

pub struct Semaphore {
    max: Count,
    count: AtomicCount,
    event: AutoResetEvent,
}

enum Timeout {
    /// Return immediately,
    None,
    /// Wait indefinitely,
    Infinite,
    /// Wait for the duration to elapse
    Bounded(Duration),
}

impl Semaphore
{
    pub const fn new(initial_count: Count, max_count: Count) -> Self {
        #[allow(unused_comparisons)]
        if max_count < 0 {
            panic!("Invalid max_count < 0");
        }
        #[allow(unused_comparisons)]
        if initial_count < 0 {
            panic!("Invalid initial_count < 0");
        }
        if initial_count > max_count {
            panic!("Invalid initial_count > max_count");
        }

        Semaphore {
            max: max_count,
            count: AtomicCount::new(initial_count as Count),
            // The event is always unset unless there's contention around the max value
            event: AutoResetEvent::new(EventState::Unset),
        }
    }

    fn try_wait(&self, timeout: Timeout) -> Result<(), TimeoutError> {
        let mut count = self.count.load(Ordering::Relaxed);

        loop {
            #[allow(unused_comparisons)]
            if count < 0 {
                debug_assert!(false, "Count cannot be less than zero!");
            }
            debug_assert!(count <= self.max);

            count = if count == 0 {
                // eprintln!("Semaphore unavailable. Sleeping until the event is signalled.");
                match timeout {
                    Timeout::None => return Err(TimeoutError),
                    Timeout::Infinite => self.event.try_wait()?,
                    Timeout::Bounded(timeout) => self.event.try_wait_for(timeout)?,
                }

                self.count.load(Ordering::Relaxed)
            } else {
                // We can't just fetch_sub(1) and check the result because we might underflow.
                match self.count.compare_exchange(count, count - 1, Ordering::Relaxed, Ordering::Relaxed) {
                    Ok(_) => {
                        // We obtained the semaphore.
                        let new_count = count - 1;
                        // eprintln!("Semaphore available. New count: {new_count}");
                        if new_count > 0 {
                            self.event.set();
                        }
                        break;
                    },
                    Err(count) => count,
                }
            }
        }

        #[allow(unused_comparisons)]
        if count < 0 {
            debug_assert!(false, "Count cannot be less than zero!");
        }
        debug_assert!(count <= self.max);

        return Ok(());
    }

    /// Attempts to increment the available concurrency by `count`, and panics if this
    /// operation would result in a count that exceeds the `max_count` the `Semaphore` was
    /// created with (see [`Semaphore::new()`]).
    ///
    /// See [`try_release`](Self::try_release) for a non-panicking version of this function.
    pub fn release(&self, count: Count) {
        let prev_count = self.count.fetch_add(count, Ordering::Relaxed);
        match prev_count.checked_add(count) {
            Some(sum) if sum <= self.max => { },
            _ => panic!("Semaphore::release() called with an inappropriate count!"),
        }
        if prev_count == 0 {
            self.event.set();
        }
    }

    /// Attempts to increment the available concurrency counter by `count`, and returns `false` if
    /// this operation would result in a count that exceeds the `max_count` the `Semaphore` was
    /// created with (see [`Semaphore::new()`]).
    ///
    /// If you can guarantee that the count cannot exceed the maximum allowed, you may want to use
    /// [`Semaphore::release()`] instead as it is both lock-free and wait-free, whereas
    /// `try_release()` is only lock-free and may spin internally in case of contention.
    pub fn try_release(&self, count: Count) -> bool {
        let mut prev_count = self.count.load(Ordering::Relaxed);
        loop {
            match prev_count.checked_add(count) {
                Some(sum) if sum <= self.max => { },
                _ => return false,
            }
            match self.count.compare_exchange_weak(prev_count, prev_count + count, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(new_count) => prev_count = new_count,
            }
        }

        // We only need to set the AutoResetEvent if the count was previously exhausted.
        // In all other cases, the last thread to obtain the semaphore would have already set the
        // event (and auto-reset events saturate/clamp immediately).
        if prev_count == 0 {
            self.event.set();
        }

        return true;
    }
}

impl Awaitable for Semaphore {
    type T = ();
    type Error = TimeoutError;

    fn try_wait(&self) -> Result<(), Infallible> {
        self.try_wait(Timeout::Infinite).unwrap();
        Ok(())
    }

    fn try_wait_for(&self, limit: Duration) -> Result<(), rsevents::TimeoutError> {
        self.try_wait(Timeout::Bounded(limit))?;
        Ok(())
    }

    fn try_wait0(&self) -> Result<(), rsevents::TimeoutError> {
        self.try_wait(Timeout::None)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::Count;
    use crate::Semaphore;
    use std::thread;
    use std::time::Duration;
    use rsevents::Awaitable;

    #[test]
    fn uncontested_semaphore() {
        let sem = Semaphore::new(1, 1);
        assert_eq!(true, sem.wait0());
        assert_eq!(false, sem.wait0());
    }

    #[test]
    fn zero_semaphore() {
        let sem = Semaphore::new(0, 0);
        assert_eq!(false, sem.wait0());
    }

    fn release_x_of_y_sequentially(x: Count, y: Count) {
        let sem: Semaphore = Semaphore::new(0, y);

        // use thread::scope because it automatically joins all threads
        thread::scope(|scope| {
            for _ in 0..x {
                scope.spawn(|| {
                    assert_eq!(false, sem.wait0());
                    assert_eq!(true, sem.wait_for(Duration::from_secs(1)));
                });
            }

            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                for _ in 0..x {
                    sem.release(1);
                }
            });
        })
    }

    fn release_x_of_y(x: Count, y: Count) {
        let sem: Semaphore = Semaphore::new(0, y);

        // use thread::scope because it automatically joins all threads
        thread::scope(|scope| {
            for _ in 0..x {
                scope.spawn(|| {
                    assert_eq!(false, sem.wait0());
                    assert_eq!(true, sem.wait_for(Duration::from_secs(1)));
                });
            }

            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                sem.release(x);
            });
        })
    }

    #[test]
    fn release_1_of_1() {
        release_x_of_y(1, 1);
    }

    #[test]
    fn release_1_of_2() {
        release_x_of_y(1, 2);
    }

    #[test]
    fn release_2_of_3() {
        release_x_of_y(1, 2);
    }

    #[test]
    fn release_2_of_2() {
        release_x_of_y_sequentially(2, 2);
    }
}
