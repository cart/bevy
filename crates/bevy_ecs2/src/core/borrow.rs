// Copyright 2019 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// modified by Bevy contributors

use core::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Atomically enforces Rust-style borrow checking at runtime
#[derive(Debug)]
pub struct AtomicBorrow(AtomicUsize);

impl AtomicBorrow {
    /// Creates a new AtomicBorrow
    pub const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    /// Starts a new immutable borrow. This can be called any number of times
    pub fn borrow(&self) -> bool {
        let value = self.0.fetch_add(1, Ordering::Acquire).wrapping_add(1);
        if value == 0 {
            // Wrapped, this borrow is invalid!
            core::panic!()
        }
        if value & UNIQUE_BIT != 0 {
            self.0.fetch_sub(1, Ordering::Release);
            false
        } else {
            true
        }
    }

    /// Starts a new mutable borrow. This must be unique. It cannot be done in parallel with other borrows or borrow_muts
    pub fn borrow_mut(&self) -> bool {
        self.0
            .compare_exchange(0, UNIQUE_BIT, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// Release an immutable borrow.
    pub fn release(&self) {
        let value = self.0.fetch_sub(1, Ordering::Release);
        debug_assert!(value != 0, "unbalanced release");
        debug_assert!(value & UNIQUE_BIT == 0, "shared release of unique borrow");
    }

    /// Release a mutable borrow.
    pub fn release_mut(&self) {
        let value = self.0.fetch_and(!UNIQUE_BIT, Ordering::Release);
        debug_assert_ne!(value & UNIQUE_BIT, 0, "unique release of shared borrow");
    }
}

const UNIQUE_BIT: usize = !(usize::max_value() >> 1);
