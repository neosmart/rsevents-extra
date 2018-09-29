# rsevents-extra

[![crates.io](https://img.shields.io/crates/v/rsevents-extra.svg)](https://crates.io/crates/rsevents-extra)
[![docs.rs](https://docs.rs/rsevents-extra/badge.svg)](https://docs.rs/crate/rsevents-extra)

`rsevents-extra` is a utility crate with a number of useful synchronization "primitives" built on
top of (and therefore, at a higher level then) [`rsevents`](https://github.com/neosmart/rsevents/).
`rsevents-extra` is a community project, feel free to contribute additional synchronization objects
to this crate!

## About `rsevents`

Please refer to the `rsevents` [README](https://github.com/neosmart/rsevents/) and
[documentation](https://docs.rs/crate/rsevents) to learn more about `rsevents`. At its core,
`rsevents` is a low-level signalling and synchronization library, mimicking the behavior of the
WIN32 auto- and manual-reset events, and can be useful to add lightweight and performant
synchronization to programs where the needs do not strictly align with the concepts of critical
sections.

## Utility events included in this crate

This crate contains implementations of the following events:

* Countdown Event

### Countdown Event

A countdown event is a useful synchronization tool for spawning tasks and checking on their
completion status. A `CountdownEvent` object is instantiated with a count, and upon each call to
`CountdownEvent::tick()`, the internal count is decremented. A waiter can call
`CountdownEvent::wait()` (or any of the other wait routines exposed by the `Awaitable` trait) to
block (efficiently) until the countdown reaches zero. Once the internal countdown reaches zero, the
event becomes set and waiters are woken/notified.


