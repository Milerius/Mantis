//! Domain types for the Polymarket trading bot.
//!
//! `no_std` by default. Enable `std` feature for serialization support.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;
