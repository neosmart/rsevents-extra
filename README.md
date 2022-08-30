# rsevents-extra

[![crates.io](https://img.shields.io/crates/v/rsevents-extra.svg)](https://crates.io/crates/rsevents-extra)
[![docs.rs](https://docs.rs/rsevents-extra/badge.svg)](https://docs.rs/rsevents-extra/latest/rsevents_extra)

`rsevents-extra` is a utility crate with a number of useful synchronization "primitives" built on top of (and therefore, at a higher level than) [`rsevents`](https://github.com/neosmart/rsevents/).
`rsevents-extra` is a community project, feel free to contribute additional synchronization objects to this crate!

## About `rsevents`

Please refer to the `rsevents` [README](https://github.com/neosmart/rsevents/) and [documentation](https://docs.rs/rsevents/latest/rsevents/) to learn more about `rsevents`, the library that this crate is built on top of.
At its core, `rsevents` is a low-level signalling and synchronization library, mimicking the behavior of the WIN32 auto- and manual-reset events, and can be useful to add lightweight and performant synchronization to programs where the needs do not strictly align with the concepts of mutexes or critical sections.

This crate includes some additional synchronization types built on top of the core events in the `rsevents` library.

## Utility events included in this crate

This crate contains implementations of the following events:

* Countdown Event
* Semaphore

### Countdown Event

A countdown event is a useful synchronization tool for spawning tasks and checking on their completion status.
A `CountdownEvent` object is instantiated with a count, and upon each call to `CountdownEvent::tick()`, the internal count is decremented.
A waiter can call `CountdownEvent::wait()` (or any of the other wait routines exposed by the `Awaitable` trait) to block efficiently until the countdown reaches zero.
Once the internal countdown reaches zero, the event becomes set and waiters are woken/notified and the event remains set until a call to `CountdownEvent::reset()` is made.

### Semaphore

A semaphore is a synchronization primitive used to limit concurrency or concurrent access to a particular resource or region.
A semaphore created with `Semaphore::new()` is assigned both a maximum concurrency and an initial concurrency (up to the maximum).
Threads obtain a concurrency token by calling `Semaphore::wait()`, which reserves them a slot to access the concurrency-limited region until the concurrency token is dropped at the end of the scope.
If more threads attempt to obtain access to a semaphore-protected region, their calls to `Semaphore::wait()` will block (while they efficiently sleep) until another thread drops its concurrency token or the semaphore's concurrency limit is increased.
