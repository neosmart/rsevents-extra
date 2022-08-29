use std::sync::atomic::{AtomicU32, Ordering};
use std::convert::Infallible;
use std::time::Duration;
use rsevents::{Awaitable, EventState, AutoResetEvent, TimeoutError};

type Count = u32;
type AtomicCount = AtomicU32;

/// A concurrency-limiting synchronization primitive, used to limit the number of threads
/// performing a certain operation or accessing a particular resource at the same time.
///
/// A `Semaphore` is created with a maximum concurrency count that can never be exceeded, and an
/// initial concurrency count that determines the available concurrency at creation. Threads
/// attempting to access a limited-concurrency resource or perform a concurrency-limited operation
/// [wait on the `Semaphore`](Semaphore::wait()), an operation which either immediately grants
/// access to the calling thread if the available concurrency has not been saturated or blocks,
/// sleeping the thread until another thread completes its concurrency-limited operation or the
/// available concurrency limit is further increased.
///
/// While the available concurrency count may be modified (decremented to zero or incremented up to
/// the maximum specified at the time of its instantiation), the maximum concurrency limit cannot be
/// changed once the `Semaphore` has been created.
///
/// ## Example:
///
/// ```no_run
/// use rsevents_extra::{Awaitable, Semaphore};
/// use std::sync::atomic::{AtomicU32, Ordering};
///
/// // Limit maximum number of simultaneous network requests to 4, but start
/// // with only 1 simultaneous network request allowed.
/// const MAX_REQUESTS: u32 = 4;
/// const START_REQUESTS: u32 = 1;
/// static HTTP_SEM: Semaphore = Semaphore::new(START_REQUESTS, MAX_REQUESTS);
/// static TASKS_LEFT: AtomicU32 = AtomicU32::new(42);
///
/// fn download_file(url: &str) -> Result<Vec<u8>, std::io::Error> {
///     fn inner_download_file(url: &str) -> Result<Vec<u8>, std::io::Error> {
///         todo!();
///     }
///
///     // Make sure we never exceed the maximum number of simultaneous
///     // network connections allowed.
///     HTTP_SEM.wait();
///     // Now we can access the network guaranteeing that the HTTP_SEM concurrency
///     // limit is respected.
///     let result = inner_download_file(url);
///     // Make sure we give up our network access slot to let another thread in.
///     HTTP_SEM.release(1);
///
///     return result;
/// }
///
/// fn get_file_from_cache(url: &str) -> Result<Vec<u8>, ()> { todo!() }
///
/// fn do_work() -> Result<(), std::io::Error> {
///     loop {
///         let mut file_in_cache = false;
///         // Do some stuff that takes time here...
///         // ...
///         let url = "https://some-url/some/path/";
///         let file = get_file_from_cache(url).or_else(|_| download_file(url))?;
///         // Do something with the file...
///         // ...
///         TASKS_LEFT.fetch_sub(1, Ordering::Relaxed);
///     }
/// }
///
/// fn main() {
///     // Start a thread to read control messages from the user
///     std::thread::spawn(|| {
///         let mut network_limit = START_REQUESTS;
///         loop {
///             println!("Press f to go faster or s to go slower");
///             let mut input = String::new();
///             std::io::stdin().read_line(&mut input).unwrap();
///             match input.trim() {
///                 "f" if network_limit < MAX_REQUESTS => {
///                     HTTP_SEM.release(1);
///                     network_limit += 1;
///                 }
///                 "s" if network_limit > 0 => {
///                     HTTP_SEM.wait();
///                     network_limit -= 1;
///                 }
///                 _ => eprintln!("Invalid request!"),
///             }
///         }
///     });
///
///     // Start 8 worker threads and wait for them to exit
///     std::thread::scope(|scope| {
///         for _ in 0..4 {
///             scope.spawn(do_work);
///         }
///     });
/// }
/// ```
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
    /// Create a new [`Semaphore`] with a maximum available concurrency count of `max_count`
    /// and an initial available concurrency count of `initial_count`.
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

    /// Attempts to obtain access to the resource or code protected by the `Semaphore`, subject to
    /// the available concurrency count. Returns immediately if the `Semaphore`'s internal
    /// concurrency count is non-zero or blocks sleeping until the `Semaphore` becomes available
    /// (via another thread completing its access to the controlled-concurrency region or if the
    /// semaphore's concurrency limit is raised).
    ///
    /// A successful wait against the semaphore decrements its internal available concurrency
    /// count (possibly preventing other threads from obtaining the semaphore) until
    /// [`Semaphore::release()`] is called.
    fn try_wait(&self) -> Result<(), Infallible> {
        self.try_wait(Timeout::Infinite).unwrap();
        Ok(())
    }

    /// Attempts a time-bounded wait against the `Semaphore`, returning `Ok(())` if and when the
    /// semaphore becomes available or a [`TimeoutError`](rsevents::TimeoutError) if the specified
    /// time limit elapses without the semaphore becoming available to the calling thread.
    fn try_wait_for(&self, limit: Duration) -> Result<(), rsevents::TimeoutError> {
        self.try_wait(Timeout::Bounded(limit))?;
        Ok(())
    }

    /// Attempts to obtain the `Semaphore` without waiting, returning `Ok(())` if the semaphore
    /// is immediately available or a [`TimeoutError`](rsevents::TimeoutError) otherwise.
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