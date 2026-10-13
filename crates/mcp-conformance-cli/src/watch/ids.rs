// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Random trace and span ids, from the operating system's CSPRNG.
//!
//! `ring`'s `SystemRandom` — already in the tree as the recorder's TLS
//! provider (`rustls`'s `ring` backend), so no new crate — reads the OS
//! generator (`getrandom` on Linux). W3C Trace Context asks for ids random
//! enough not to collide across processes; nothing here is a secret.
//!
//! Should the OS generator ever fail, ids fall back to std's randomly keyed
//! `SipHash` over a counter and the time: unpredictable enough for ids, and an
//! export never stops for want of one.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher as _, Hasher as _};
use std::sync::atomic::{AtomicU64, Ordering};

use ring::rand::{SecureRandom as _, SystemRandom};

/// A source of ids.
#[derive(Debug)]
pub(super) struct Ids {
    system: SystemRandom,
    fallback: RandomState,
    counter: AtomicU64,
}

impl Ids {
    pub(super) fn new() -> Self {
        Self {
            system: SystemRandom::new(),
            fallback: RandomState::new(),
            counter: AtomicU64::new(0),
        }
    }

    /// `N` random bytes.
    pub(super) fn bytes<const N: usize>(&self) -> [u8; N] {
        let mut bytes = [0_u8; N];
        if self.system.fill(&mut bytes).is_err() {
            for chunk in bytes.chunks_mut(8) {
                let word = self.fallback_word().to_le_bytes();
                chunk.copy_from_slice(&word[..chunk.len()]);
            }
        }
        bytes
    }

    fn fallback_word(&self) -> u64 {
        let mut hasher = self.fallback.build_hasher();
        hasher.write_u64(self.counter.fetch_add(1, Ordering::Relaxed));
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos()),
        );
        hasher.finish()
    }

    /// A trace id: 16 random bytes, never all zeros (OTLP's "invalid").
    pub(super) fn trace_id(&self) -> [u8; 16] {
        loop {
            let id = self.bytes::<16>();
            if id != [0; 16] {
                return id;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_differ_and_are_never_zero() {
        let ids = Ids::new();
        let traces: std::collections::HashSet<[u8; 16]> = (0..64).map(|_| ids.trace_id()).collect();
        assert_eq!(traces.len(), 64);
        let spans: std::collections::HashSet<[u8; 8]> = (0..64).map(|_| ids.bytes::<8>()).collect();
        assert_eq!(spans.len(), 64);
        assert_ne!(ids.fallback_word(), ids.fallback_word());
    }
}
