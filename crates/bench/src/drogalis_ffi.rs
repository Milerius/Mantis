//! FFI bindings to `dro::SPSCQueue` (C++ contender).
//!
//! Wraps the C functions from `cpp/drogalis_ffi.cpp` into safe Rust
//! types for use in benchmarks.

#![allow(unsafe_code, missing_docs, clippy::missing_panics_doc)]

use crate::messages::{Message48, Message64};

unsafe extern "C" {
    fn drogalis_u64_create(capacity: usize) -> *mut core::ffi::c_void;
    fn drogalis_u64_destroy(q: *mut core::ffi::c_void);
    fn drogalis_u64_try_push(q: *mut core::ffi::c_void, value: u64) -> bool;
    fn drogalis_u64_try_pop(q: *mut core::ffi::c_void, out: *mut u64) -> bool;

    fn drogalis_msg48_create(capacity: usize) -> *mut core::ffi::c_void;
    fn drogalis_msg48_destroy(q: *mut core::ffi::c_void);
    fn drogalis_msg48_try_push(q: *mut core::ffi::c_void, value: *const Message48) -> bool;
    fn drogalis_msg48_try_pop(q: *mut core::ffi::c_void, out: *mut Message48) -> bool;

    fn drogalis_msg64_create(capacity: usize) -> *mut core::ffi::c_void;
    fn drogalis_msg64_destroy(q: *mut core::ffi::c_void);
    fn drogalis_msg64_try_push(q: *mut core::ffi::c_void, value: *const Message64) -> bool;
    fn drogalis_msg64_try_pop(q: *mut core::ffi::c_void, out: *mut Message64) -> bool;
}

/// Safe wrapper around `dro::SPSCQueue<u64>`.
pub struct DrogalisU64 {
    ptr: *mut core::ffi::c_void,
}

impl DrogalisU64 {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        // SAFETY: FFI call to create a heap-allocated C++ queue.
        let ptr = unsafe { drogalis_u64_create(capacity) };
        assert!(!ptr.is_null(), "drogalis allocation failed");
        Self { ptr }
    }

    pub fn try_push(&mut self, value: u64) -> bool {
        // SAFETY: ptr is valid, single-threaded bench usage.
        unsafe { drogalis_u64_try_push(self.ptr, value) }
    }

    pub fn try_pop(&mut self) -> Option<u64> {
        let mut out = 0u64;
        // SAFETY: ptr is valid, out is a valid pointer.
        if unsafe { drogalis_u64_try_pop(self.ptr, &raw mut out) } {
            Some(out)
        } else {
            None
        }
    }
}

impl Drop for DrogalisU64 {
    fn drop(&mut self) {
        // SAFETY: ptr was created by drogalis_u64_create.
        unsafe { drogalis_u64_destroy(self.ptr) }
    }
}

/// Safe wrapper around `dro::SPSCQueue<Message48>`.
pub struct DrogalisMsg48 {
    ptr: *mut core::ffi::c_void,
}

impl DrogalisMsg48 {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let ptr = unsafe { drogalis_msg48_create(capacity) };
        assert!(!ptr.is_null(), "drogalis allocation failed");
        Self { ptr }
    }

    pub fn try_push(&mut self, value: &Message48) -> bool {
        unsafe { drogalis_msg48_try_push(self.ptr, value) }
    }

    pub fn try_pop(&mut self) -> Option<Message48> {
        let mut out = Message48::default();
        if unsafe { drogalis_msg48_try_pop(self.ptr, &raw mut out) } {
            Some(out)
        } else {
            None
        }
    }
}

impl Drop for DrogalisMsg48 {
    fn drop(&mut self) {
        unsafe { drogalis_msg48_destroy(self.ptr) }
    }
}

/// Safe wrapper around `dro::SPSCQueue<Message64>`.
pub struct DrogalisMsg64 {
    ptr: *mut core::ffi::c_void,
}

impl DrogalisMsg64 {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let ptr = unsafe { drogalis_msg64_create(capacity) };
        assert!(!ptr.is_null(), "drogalis allocation failed");
        Self { ptr }
    }

    pub fn try_push(&mut self, value: &Message64) -> bool {
        unsafe { drogalis_msg64_try_push(self.ptr, value) }
    }

    pub fn try_pop(&mut self) -> Option<Message64> {
        let mut out = Message64::default();
        if unsafe { drogalis_msg64_try_pop(self.ptr, &raw mut out) } {
            Some(out)
        } else {
            None
        }
    }
}

impl Drop for DrogalisMsg64 {
    fn drop(&mut self) {
        unsafe { drogalis_msg64_destroy(self.ptr) }
    }
}
