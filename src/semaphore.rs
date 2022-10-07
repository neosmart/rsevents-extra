use std::fmt::Debug;
use std::sync::atomic::{AtomicU16, Ordering};
use std::convert::Infallible;
use std::time::Duration;
use rsevents::{Awaitable, EventState, AutoResetEvent, TimeoutError};

type Count = u16;
type AtomicCount = AtomicU16;
type ICount = i16;
type INext = i32;

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
/// use rsevents_extra::{Semaphore};
/// use std::sync::atomic::{AtomicU32, Ordering};
///
/// // Limit maximum number of simultaneous network requests to 4, but start
/// // with only 1 simultaneous network request allowed.
/// const MAX_REQUESTS: u16 = 4;
/// const START_REQUESTS: u16 = 1;
/// static HTTP_SEM: Semaphore = Semaphore::new(START_REQUESTS, MAX_REQUESTS);
/// static TASKS_LEFT: AtomicU32 = AtomicU32::new(42);
///
/// fn download_file(url: &str) -> Result<Vec<u8>, std::io::Error> {
///     // Make sure we never exceed the maximum number of simultaneous
///     // network connections allowed.
///     let sem_guard = HTTP_SEM.wait();
///
///     // <download the file here>
///
///     // When `sem_guard` is dropped at the end of the scope, we give up our
///     // network access slot letting another thread through.
///     return Ok(unimplemented!());
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
///                     HTTP_SEM.wait().forget();
///                     network_limit -= 1;
///                 }
///                 _ => eprintln!("Invalid request!"),
///             }
///         }
///     });
///
///     // Start 8 worker threads and wait for them to finish
///     std::thread::scope(|scope| {
///         for _ in 0..8 {
///             scope.spawn(do_work);
///         }
///     });
/// }
/// ```
pub struct Semaphore {
    /// The maximum available concurrency for this semaphore, set at the time of initialization and
    /// static thereafter.
    max: Count,
    /// The current available concurrency for this semaphore, `> 0 && <= max`. This is like
    /// `current` but it also includes "currently borrowed" semaphore instances. The only reason for
    /// this field to exist is so that a truly safe `Semaphore::try_release()` method can exist (one
    /// that can guarantee not only that the new `count` won't exceed `max`, but also that the
    /// release operation will never cause `count` to exceed `max` even after all borrowed semaphore
    /// slots are returned.
    current: AtomicCount,
    /// The currently available concurrency count, equal to `current` minus any borrowed/obtained
    /// semaphore slots.
    count: AtomicCount,
    /// The auto-reset event used to sleep awaiting threads until a zero concurrency count is
    /// incremented, waking only one awaiter at a time.
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
            current: AtomicCount::new(initial_count),
            count: AtomicCount::new(initial_count as Count),
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
                match self.count.compare_exchange_weak(count, count - 1, Ordering::Relaxed, Ordering::Relaxed) {
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

    /// Attempts to obtain access to the resource or code protected by the `Semaphore`, subject to
    /// the available concurrency count. Returns immediately if the `Semaphore`'s internal
    /// concurrency count is non-zero or blocks sleeping until the `Semaphore` becomes available
    /// (via another thread completing its access to the controlled-concurrency region or if the
    /// semaphore's concurrency limit is raised).
    ///
    /// A successful wait against the semaphore decrements its internal available concurrency
    /// count (possibly preventing other threads from obtaining the semaphore) until
    /// [`Semaphore::release()`] is called (which happens automatically when the `SemaphoreGuard`
    /// concurrency token is dropped).
    pub fn wait<'a>(&'a self) -> SemaphoreGuard<'a> {
        self.try_wait(Timeout::Infinite).unwrap();
        SemaphoreGuard { semaphore: &self }
    }

    #[allow(unused)]
    fn wait0<'a>(&'a self) -> Result<SemaphoreGuard<'a>, rsevents::TimeoutError> {
        self.try_wait(Timeout::None)?;
        Ok(SemaphoreGuard { semaphore: &self })
    }

    /// Attempts a time-bounded wait against the `Semaphore`, returning `Ok(())` if and when the
    /// semaphore becomes available or a [`TimeoutError`](rsevents::TimeoutError) if the specified
    /// time limit elapses without the semaphore becoming available to the calling thread.
    pub fn wait_for<'a>(&'a self, limit: Duration) -> Result<SemaphoreGuard<'a>, rsevents::TimeoutError> {
        match limit {
            Duration::ZERO => self.try_wait(Timeout::None)?,
            timeout => self.try_wait(Timeout::Bounded(timeout))?,
        };
        Ok(SemaphoreGuard { semaphore: &self })
    }

    #[inline]
    /// Directly increments the available concurrency count by `count`, without checking if this
    /// would violate the maximum available concurrency count.
    unsafe fn release_internal(&self, count: Count) {
        let prev_count = self.count.fetch_add(count, Ordering::Relaxed);

        // We only need to set the AutoResetEvent if the count was previously exhausted.
        // In all other cases, the last thread to obtain the semaphore would have already set the
        // event (and auto-reset events saturate/clamp immediately).
        if prev_count == 0 {
            self.event.set();
        }
    }

    /// Directly modifies the maximum currently available concurrency `current`, without regard for
    /// overflow or a violation of the semaphore's maximum allowed count.
    unsafe fn modify_current(&self, count: ICount) {
        match count.signum() {
            0 => return,
            1 => self.current.fetch_add(count as Count, Ordering::Relaxed),
            -1 => self.current.fetch_sub((count as INext).abs() as Count, Ordering::Relaxed),
            _ => unsafe { core::hint::unreachable_unchecked() },
        };
    }

    /// Directly increments or decrements the current availability limit for a `Semaphore` without
    /// blocking. This is only possible when the semaphore is not currently borrowed or being waited
    /// on. Panics if the change will result in an available concurrency limit of less than zero or
    /// greater than the semaphore's maximum. See [`Semaphore::try_modify()`] for a non-panicking
    /// alternative.
    ///
    /// To increment the semaphore's concurrency limit without an `&mut Semaphore` reference, call
    /// [`Semaphore::release()`] instead. To decrement the concurrency limit, wait on the semaphore
    /// then call [`forget()`](SemaphoreGuard::forget) on the returned `SemaphoreGuard`:
    ///
    /// ```rust
    /// use rsevents_extra::Semaphore;
    ///
    /// fn adjust_sem(sem: &Semaphore, count: i16) {
    ///     if count >= 0 {
    ///        sem.release(count as u16);
    ///     } else {
    ///         // Note: this will block if the semaphore isn't available!
    ///         for _ in 0..(-1 * count) {
    ///             let guard = sem.wait();
    ///             guard.forget();
    ///         }
    ///     }
    /// }
    /// ```
    pub fn modify(&mut self, count: ICount) {
        let current = self.current.load(Ordering::Relaxed);
        match (current as INext).checked_add(count as INext) {
            Some(sum) if sum <= (self.max as INext) => {},
            _ => panic!("An invalid count was supplied to Semaphore::modify()"),
        };

        match count.signum() {
            0 => return,
            1 => {
                self.current.fetch_add(count as Count, Ordering::Relaxed);
                self.count.fetch_add(count as Count, Ordering::Relaxed);
            },
            -1 => {
                self.current.fetch_add((count as INext).abs() as Count, Ordering::Relaxed);
                self.count.fetch_add((count as INext).abs() as Count, Ordering::Relaxed);
            }
            _ => unsafe { core::hint::unreachable_unchecked(); },
        }
    }

    /// Directly increments or decrements the current availability limit for a `Semaphore` without
    /// blocking. This is only possible when the semaphore is not currently borrowed or being waited
    /// on. Returns `false` if the change will result in an available concurrency limit of less
    /// than zero or greater than the semaphore's maximum.
    ///
    /// See [`Semaphore::modify()`] for more info.
    pub fn try_modify(&mut self, count: ICount) -> bool {
        let current = self.current.load(Ordering::Relaxed);
        match (current as INext).checked_add(count as INext) {
            Some(sum) if sum <= (self.max as INext) => {},
            _ => return false,
        };

        match count.signum() {
            0 => return true,
            1 => {
                self.current.fetch_add(count as Count, Ordering::Relaxed);
                self.count.fetch_add(count as Count, Ordering::Relaxed);
            },
            -1 => {
                self.current.fetch_add((count as INext).abs() as Count, Ordering::Relaxed);
                self.count.fetch_add((count as INext).abs() as Count, Ordering::Relaxed);
            }
            _ => unsafe { core::hint::unreachable_unchecked(); },
        };

        return true;
    }

    /// Increments the available concurrency by `count`, and panics if this results in a count that
    /// exceeds the `max_count` the `Semaphore` was created with (see [`Semaphore::new()`]). Unlike
    /// [`Semaphore::modify()`], this can be called with a non-mutable reference to the semaphore,
    /// but can only increment the concurrency level.
    ///
    /// See [`try_release`](Self::try_release) for a non-panicking version of this function.
    /// See the documentation for [`modify()`](Self::modify) for info on decrementing the available
    /// concurrency level.
    pub fn release(&self, count: Count) {
        // Increment the "current maximum" which includes borrowed semaphore instances.
        let prev_count = self.current.fetch_add(count, Ordering::Relaxed);
        match prev_count.checked_add(count) {
            Some(sum) if sum <= self.max => { },
            _ => panic!("Semaphore::release() called with an inappropriate count!"),
        }
        // Increment the actual "currently available" count to match. The two fields do not need to
        // be updated atomically because we only care that the previous operation succeeded, but do
        // not need to modify this variable contingent on that one.
        unsafe { self.release_internal(count); }
    }

    /// Attempts to increment the available concurrency counter by `count`, and returns `false` if
    /// this operation would result in a count that exceeds the `max_count` the `Semaphore` was
    /// created with (see [`Semaphore::new()`]).
    ///
    /// If you can guarantee that the count cannot exceed the maximum allowed, you may want to use
    /// [`Semaphore::release()`] instead as it is both lock-free and wait-free, whereas
    /// `try_release()` is only lock-free and may spin internally in case of contention.
    pub fn try_release(&self, count: Count) -> bool {
        // Try to increment the "current maximum" which includes borrowed semaphore instances.
        let mut prev_count = self.current.load(Ordering::Relaxed);
        loop {
            match prev_count.checked_add(count) {
                Some(sum) if sum <= self.max => { },
                _ => return false,
            }
            match self.current.compare_exchange_weak(prev_count, prev_count + count, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(new_count) => prev_count = new_count,
            }
        }

        // Increment the actual "currently available" count to match. The two fields do not need to
        // be updated atomically because we only care that the previous operation succeeded, but do
        // not need to modify this variable contingent on that one.
        unsafe { self.release_internal(count); }

        return true;
    }
}

impl<'a> Awaitable<'a> for Semaphore {
    type T = SemaphoreGuard<'a>;
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
    fn try_wait(&'a self) -> Result<SemaphoreGuard<'a>, Infallible> {
        self.try_wait(Timeout::Infinite).unwrap();
        Ok(SemaphoreGuard { semaphore: &self })
    }

    /// Attempts a time-bounded wait against the `Semaphore`, returning `Ok(())` if and when the
    /// semaphore becomes available or a [`TimeoutError`](rsevents::TimeoutError) if the specified
    /// time limit elapses without the semaphore becoming available to the calling thread.
    fn try_wait_for(&'a self, limit: Duration) -> Result<SemaphoreGuard<'a>, rsevents::TimeoutError> {
        self.try_wait(Timeout::Bounded(limit))?;
        Ok(SemaphoreGuard { semaphore: &self })
    }

    /// Attempts to obtain the `Semaphore` without waiting, returning `Ok(())` if the semaphore
    /// is immediately available or a [`TimeoutError`](rsevents::TimeoutError) otherwise.
    fn try_wait0(&'a self) -> Result<SemaphoreGuard<'a>, rsevents::TimeoutError> {
        self.try_wait(Timeout::None)?;
        Ok(SemaphoreGuard { semaphore: &self })
    }
}

/// The concurrency token returned by [`Semaphore::wait()`], allowing access to the
/// concurrency-limited region/code. Gives up its slot when dropped, allowing another thread to
/// enter the semaphore in its place.
///
/// `SemaphoreGuard` instances should never be passed to `std::mem::forget()` &ndash;
/// [`SemaphoreGuard::forget()`] should be called instead to forget a `SemaphoreGuard` and
/// permanently decrease the available concurrency.
pub struct SemaphoreGuard<'a> {
    semaphore: &'a Semaphore,
}

impl SemaphoreGuard<'_> {
    /// Safely "forgets" a semaphore's guard, permanently reducing the concurrency limit of the
    /// associated `Semaphore`. `SemaphoreGuard::forget()` internally decrements the semaphore's
    /// availablibility counter to make sure that future calls to `Semaphore::release()` or
    /// `Semaphore::try_release()` do not incorrectly report failure.
    ///
    /// A `SemaphoreGuard` instance should never be passed to `std::mem::forget()` directly, as that
    /// would violate the internal contract; this method should be used instead.
    pub fn forget(self) {
        unsafe { self.semaphore.modify_current(-1); }
        core::mem::forget(self);
    }
}

impl Debug for SemaphoreGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemaphoreGuard").finish_non_exhaustive()
    }
}

impl Drop for SemaphoreGuard<'_> {
    fn drop(&mut self) {
        unsafe { self.semaphore.release_internal(1); }
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
        let _1 = sem.wait0().unwrap();
        sem.try_wait0().unwrap_err();
    }

    #[test]
    fn zero_semaphore() {
        let sem = Semaphore::new(0, 0);
        sem.try_wait0().unwrap_err();
    }

    fn release_x_of_y_sequentially(x: Count, y: Count) -> Semaphore {
        let sem: Semaphore = Semaphore::new(0, y);

        // Use thread::scope because it automatically joins all threads,
        // which is useful if they panic.
        thread::scope(|scope| {
            for _ in 0..x {
                scope.spawn(|| {
                    sem.wait0().unwrap_err();
                    let lock = sem.wait_for(Duration::from_secs(1)).unwrap();
                    // Correct way to "forget" a semaphore slot; never pass a
                    // SemaphoreGuard to std::mem::forget()!
                    lock.forget();
                });
            }

            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                for _ in 0..x {
                    sem.release(1);
                }
            });
        });

        sem
    }

    fn release_x_of_y(x: Count, y: Count) -> Semaphore {
        let sem: Semaphore = Semaphore::new(0, y);

        // Use thread::scope because it automatically joins all threads,
        // which is useful if they panic.
        thread::scope(|scope| {
            for _ in 0..x {
                scope.spawn(|| {
                    sem.wait0().unwrap_err();
                    let lock = sem.wait_for(Duration::from_secs(1)).unwrap();
                    std::mem::forget(lock);
                });
            }

            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                sem.release(x);
            });
        });

        sem
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
        let sem = release_x_of_y_sequentially(2, 2);
        sem.wait0().unwrap_err();
    }
}
