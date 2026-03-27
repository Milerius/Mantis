//! FFI bindings to `rigtorp::SPSCQueue` (C++ contender).
//!
//! Wraps the C functions from `cpp/rigtorp_ffi.cpp` into safe Rust
//! types for use in benchmarks.

#![allow(unsafe_code, missing_docs, clippy::missing_panics_doc)]

use crate::messages::{Message48, Message64};

unsafe extern "C" {
    fn rigtorp_u64_create(capacity: usize) -> *mut core::ffi::c_void;
    fn rigtorp_u64_destroy(q: *mut core::ffi::c_void);
    fn rigtorp_u64_try_push(q: *mut core::ffi::c_void, value: u64) -> bool;
    fn rigtorp_u64_try_pop(q: *mut core::ffi::c_void, out: *mut u64) -> bool;

    fn rigtorp_msg48_create(capacity: usize) -> *mut core::ffi::c_void;
    fn rigtorp_msg48_destroy(q: *mut core::ffi::c_void);
    fn rigtorp_msg48_try_push(q: *mut core::ffi::c_void, value: *const Message48) -> bool;
    fn rigtorp_msg48_try_pop(q: *mut core::ffi::c_void, out: *mut Message48) -> bool;

    fn rigtorp_msg64_create(capacity: usize) -> *mut core::ffi::c_void;
    fn rigtorp_msg64_destroy(q: *mut core::ffi::c_void);
    fn rigtorp_msg64_try_push(q: *mut core::ffi::c_void, value: *const Message64) -> bool;
    fn rigtorp_msg64_try_pop(q: *mut core::ffi::c_void, out: *mut Message64) -> bool;
}

/// Safe wrapper around `rigtorp::SPSCQueue<u64>`.
pub struct RigtorpU64 {
    ptr: *mut core::ffi::c_void,
}

impl RigtorpU64 {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        // SAFETY: FFI call to create a heap-allocated C++ queue.
        let ptr = unsafe { rigtorp_u64_create(capacity) };
        assert!(!ptr.is_null(), "rigtorp allocation failed");
        Self { ptr }
    }

    pub fn try_push(&mut self, value: u64) -> bool {
        // SAFETY: ptr is valid, single-threaded bench usage.
        unsafe { rigtorp_u64_try_push(self.ptr, value) }
    }

    pub fn try_pop(&mut self) -> Option<u64> {
        let mut out = 0u64;
        // SAFETY: ptr is valid, out is a valid pointer.
        if unsafe { rigtorp_u64_try_pop(self.ptr, &raw mut out) } {
            Some(out)
        } else {
            None
        }
    }
}

impl Drop for RigtorpU64 {
    fn drop(&mut self) {
        // SAFETY: ptr was created by rigtorp_u64_create.
        unsafe { rigtorp_u64_destroy(self.ptr) }
    }
}

/// Safe wrapper around `rigtorp::SPSCQueue<Message48>`.
pub struct RigtorpMsg48 {
    ptr: *mut core::ffi::c_void,
}

impl RigtorpMsg48 {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let ptr = unsafe { rigtorp_msg48_create(capacity) };
        assert!(!ptr.is_null(), "rigtorp allocation failed");
        Self { ptr }
    }

    pub fn try_push(&mut self, value: &Message48) -> bool {
        unsafe { rigtorp_msg48_try_push(self.ptr, value) }
    }

    pub fn try_pop(&mut self) -> Option<Message48> {
        let mut out = Message48::default();
        if unsafe { rigtorp_msg48_try_pop(self.ptr, &raw mut out) } {
            Some(out)
        } else {
            None
        }
    }
}

impl Drop for RigtorpMsg48 {
    fn drop(&mut self) {
        unsafe { rigtorp_msg48_destroy(self.ptr) }
    }
}

/// Safe wrapper around `rigtorp::SPSCQueue<Message64>`.
pub struct RigtorpMsg64 {
    ptr: *mut core::ffi::c_void,
}

impl RigtorpMsg64 {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let ptr = unsafe { rigtorp_msg64_create(capacity) };
        assert!(!ptr.is_null(), "rigtorp allocation failed");
        Self { ptr }
    }

    pub fn try_push(&mut self, value: &Message64) -> bool {
        unsafe { rigtorp_msg64_try_push(self.ptr, value) }
    }

    pub fn try_pop(&mut self) -> Option<Message64> {
        let mut out = Message64::default();
        if unsafe { rigtorp_msg64_try_pop(self.ptr, &raw mut out) } {
            Some(out)
        } else {
            None
        }
    }
}

impl Drop for RigtorpMsg64 {
    fn drop(&mut self) {
        unsafe { rigtorp_msg64_destroy(self.ptr) }
    }
}
