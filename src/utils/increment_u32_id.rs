use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};

/// A simple counter which is initialized at 0.
static GLOBAL_ID_COUNTER: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

#[inline]
pub fn increment_u32_id() -> u32 {
    GLOBAL_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u32
}

// TODO: If converting a full SHA-1 hash to a compact u64 fingerprint, use a mixing
// strategy like this FNV-1a fold.
//
// fn sha1_to_u64_fnv_fold(digest: &[u8; 20]) -> u64 {
//     const FNV_OFFSET: u64 = 0xcbf29ce484222325;
//     const FNV_PRIME: u64 = 0x100000001b3;
//
//     let mut hash = FNV_OFFSET;
//     for &b in digest {
//         hash ^= b as u64;
//         hash = hash.wrapping_mul(FNV_PRIME);
//     }
//     hash
// }
//
// This compresses all 160 bits of SHA-1 into a single u64 using a fast, deterministic
// non-cryptographic hash (FNV-1a). It preserves more entropy than simple truncation,
// is collision-resistant for non-adversarial use, and works well for local indexing,
// deduplication hints, or stable identifiers.
//
// ⚠️ Not cryptographically secure — do not use for authentication, digital signatures,
// or any use case requiring preimage or collision resistance.
// fn sha1_to_u64_fnv_fold(digest: &[u8; 20]) -> u64 {
//     const FNV_OFFSET: u64 = 0xcbf29ce484222325;
//     const FNV_PRIME: u64 = 0x100000001b3;

//     let mut hash = FNV_OFFSET;
//     for &b in digest {
//         hash ^= b as u64;
//         hash = hash.wrapping_mul(FNV_PRIME);
//     }
//     hash
// }
