//! Polymarket venue decoder and client SDK for the Mantis SDK.
//!
//! Provides zero-allocation JSON decoders that convert Polymarket
//! WebSocket messages into [`mantis_events::HotEvent`] values.

#![deny(unsafe_code)]

pub mod market;
