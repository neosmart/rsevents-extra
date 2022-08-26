mod countdown;
mod semaphore;

pub use self::countdown::CountdownEvent;
pub use self::semaphore::Semaphore;

/// Re-export [`rsevents::Awaitable`] so consumers of this crate do not need to add an explicit
/// dependency on `rsevents` just to wait on one of our events.
pub use rsevents::Awaitable;
