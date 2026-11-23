//! The crate's only `unsafe`: one kernel32 call, so a detached child does not inherit the hook's pipe.
//!
//! Why: Rust's `CreateProcessW` passes `bInheritHandles = TRUE`, so a child inherits every
//! inheritable handle of its parent. The hook's stdin, stdout and stderr came from Claude Code, so
//! they are inheritable, and a detached `knowledge flush` kept the hook's stdout open: Claude Code
//! read until EOF and waited for the flush (6.2 s at SessionEnd, up to the flush's 120 s timeout).
//! A trampoline process does not help, the flag is copied with the handle (measured 4.1 s).
//! `CommandExt::inherit_handles(false)` is the safe fix and is unstable (rust-lang/rust#146407);
//! when it stabilises, this file goes and the crate returns to `forbid(unsafe_code)`.
//! node and bun do the same at startup (libuv's `uv_disable_stdio_inheritance`).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;

const HANDLE_FLAG_INHERIT: u32 = 0x1;

// kernel32 is linked by std already: no dependency.
unsafe extern "system" {
    fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
}

/// Clear the inherit flag on this process's three standard handles. Children still get the stdio
/// they are given (std duplicates those as inheritable for the one spawn); they just stop picking
/// up the hook's own pipes. A failure leaves the old behaviour, which is slow, not wrong.
pub fn stdio_not_inherited() {
    for h in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        if h.is_null() {
            continue; // no console and no redirect: nothing to inherit
        }
        // SAFETY: `h` is one of this process's standard handles, valid for the process's life
        // (std never closes them). SetHandleInformation only changes the handle's flags; it reads
        // and writes no memory of ours. An invalid handle makes it return 0, which is ignored.
        unsafe {
            SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
        }
    }
}
