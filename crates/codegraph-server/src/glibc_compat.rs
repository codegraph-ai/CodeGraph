// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! glibc 2.31 compatibility storage for `__libc_single_threaded`.
//!
//! `__libc_single_threaded` was added in glibc 2.32 but ONNX Runtime
//! references it, so a build for SLES 15 SP4 and similar needs a definition to
//! link against. Every target that links ONNX Runtime - the binary and the
//! test executables, which do not include `main.rs` - has to supply one.
//!
//! The storage must be WRITABLE. It was previously `pub static ...: u8 = 0`,
//! which lands in .rodata, under a comment claiming "on newer glibc the real
//! symbol shadows this at runtime" - the opposite of how ELF resolves it. A
//! definition in the executable takes precedence over the one in libc, and on
//! aarch64 this symbol is also emitted into .dynsym, so glibc bound its own
//! startup write of the flag to the read-only byte and took SIGSEGV before
//! `main()`: every invocation died, including `--version` and `--help`
//! (issue #15). x86_64 escaped only because the symbol is not dynamically
//! exported there, so glibc kept using its own copy.
//!
//! glibc owns the value: it sets the flag at startup and clears it when a
//! thread is created. We only supply the storage, and never read it. On a
//! glibc too old to maintain it, the byte stays 0, which is the conservative
//! "not single threaded" answer.
//!
//! Every definition of the symbol must use [`SingleThreaded`] so the writable
//! storage cannot drift back to a plain `u8` in one target and not another.

use std::cell::UnsafeCell;

/// Writable single-byte storage for glibc's `__libc_single_threaded` flag.
#[repr(transparent)]
pub struct SingleThreaded(UnsafeCell<u8>);

impl SingleThreaded {
    /// The initial value of the flag: "not single threaded".
    ///
    /// `declare_interior_mutable_const` warns because copying a const with
    /// interior mutability normally gives each use its own hidden cell. That
    /// is precisely the intent here: this const exists only to initialise the
    /// `#[no_mangle] static` in each target, so there is exactly one cell per
    /// target and nothing ever reads it through the const.
    #[allow(clippy::declare_interior_mutable_const)]
    pub const ZERO: Self = Self(UnsafeCell::new(0));
}

// SAFETY: glibc is the only writer, from its own startup and thread-creation
// paths, and this process never reads the byte. The UnsafeCell is what places
// it in writable memory rather than .rodata.
unsafe impl Sync for SingleThreaded {}
