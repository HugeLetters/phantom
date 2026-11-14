//! A torrent's machines. Its lifecycle is the shared one (the `lifecycle` crate, [ADR 0006]),
//! which the GUI's fake service runs too; its librqbit side ([`inlib`]) is the engine's own.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

pub use lifecycle::{Broke, Do, Event, Lib, Observation, Phase, Refusal, State, Wants, decide, next};

pub mod inlib;
