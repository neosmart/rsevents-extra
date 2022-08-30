mod countdown;
mod semaphore;

pub use self::countdown::CountdownEvent;
pub use self::semaphore::{Semaphore, SemaphoreGuard};

/// The `rsevents` abstraction over all types that can be awaited, implemented by types in this
/// crate.
///
pub use rsevents::Awaitable;
/// The default `rsevents` error for `Awaitable` implementations in this crate, indicating that
/// unbounded calls to [`Awaitable::wait()`] cannot fail.
///
pub use rsevents::TimeoutError;
