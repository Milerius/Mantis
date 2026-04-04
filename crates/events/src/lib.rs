//! Hot event language for the Mantis low-latency financial SDK.
//!
//! This crate defines the typed message language for hot-path event transport,
//! carried by `SpscRingCopy<HotEvent, N>` queues between single-writer owner threads.
//!
//! # Dependency firewall
//!
//! This crate depends on `mantis-types` but NOT on `mantis-fixed`.
//! `FixedI64` must never appear in hot event payloads. Decimal parsing
//! and normalization happen at the ingestion boundary, before events
//! enter the hot path.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

mod body;
mod control;
mod event;
mod execution;
mod flags;
mod header;
mod market;

pub use body::{EventBody, EventKind};
pub use control::{HeartbeatPayload, TimerKind, TimerPayload};
pub use event::HotEvent;
pub use execution::{FillPayload, OrderAckPayload, OrderRejectPayload, OrderStatus, RejectReason};
pub use flags::EventFlags;
pub use header::EventHeader;
pub use market::{BookDeltaPayload, TopOfBookPayload, TradePayload, UpdateAction};

const _: () = assert!(core::mem::size_of::<EventHeader>() == 24);
