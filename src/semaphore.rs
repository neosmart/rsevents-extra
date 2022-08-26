use std::sync::atomic::{AtomicU32, Ordering};
use std::convert::Infallible;
use std::time::Duration;
use rsevents::{Awaitable, EventState, AutoResetEvent, TimeoutError};

type Count = u32;
type AtomicCount = AtomicU32;

pub struct Semaphore<'a, T> {
    _lifetime: core::marker::PhantomData<&'a ()>,
    max: Count,
    count: AtomicCount,
    event: AutoResetEvent,
    value: T
}

enum Timeout {
    /// Return immediately,
    None,
    /// Wait indefinitely,
    Infinite,
    /// Wait for the duration to elapse
    Bounded(Duration),
}

impl<'a, T> Semaphore<'a, T>
{
    pub const fn new(initial_count: Count, max_count: Count, value: T) -> Self {
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
            _lifetime: std::marker::PhantomData::<&'a ()>,
            max: max_count,
            count: AtomicCount::new(initial_count as Count),
            // The event is always unset unless there's contention around the max value
            event: AutoResetEvent::new(EventState::Unset),
            value,
        }
    }

    fn try_wait(&self, timeout: Timeout) -> Result<&T, TimeoutError> {
        let mut count = self.count.load(Ordering::Relaxed);

        loop {
            #[allow(unused_comparisons)]
            if count < 0 {
                debug_assert!(false, "Count cannot be less than zero!");
            }
            debug_assert!(count <= self.max);

            count = if count == 0 {
                eprintln!("Semaphore unavailable. Sleeping until the event is signalled.");
                match timeout {
                    Timeout::None => self.event.try_wait0()?,
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
                        eprintln!("Semaphore available. New count: {new_count}");
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

        Ok(&self.value)
    }

    pub fn release(&self, count: Count) {
        let prev_count = self.count.fetch_add(count, Ordering::Relaxed);
        // This is an overflow-safe version of the "prev_count + count > max"
        if self.max - prev_count < count {
            panic!("Semaphore::release() called with an inappropriate count!");
        }
        if prev_count == 0 {
            self.event.set();
        }
    }
}

impl<'a, T> Awaitable for Semaphore<'a, T>
where T: std::fmt::Debug {
    type T = ();
    type Error = TimeoutError;

    fn try_wait(&self) -> Result<Self::T, Infallible> {
        self.try_wait(Timeout::Infinite).unwrap();
        Ok(())
    }

    fn try_wait_for(&self, limit: Duration) -> Result<Self::T, rsevents::TimeoutError> {
        self.try_wait(Timeout::Bounded(limit))?;
        Ok(())
    }

    fn try_wait0(&self) -> Result<Self::T, rsevents::TimeoutError> {
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
        let sem = Semaphore::new(1, 1, ());
        assert_eq!(true, sem.wait0());
        assert_eq!(false, sem.wait0());
    }

    #[test]
    fn zero_semaphore() {
        let sem = Semaphore::new(0, 0, ());
        assert_eq!(false, sem.wait0());
    }

    fn release_x_of_y_sequentially(x: Count, y: Count) {
        let sem: Semaphore<()> = Semaphore::new(0, y, ());

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
        let sem: Semaphore<()> = Semaphore::new(0, y, ());

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
