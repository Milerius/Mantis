//! Signal engine for Polymarket crypto Up/Down temporal arbitrage.
//!
//! `no_std` by default. Pure math, zero I/O.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;
