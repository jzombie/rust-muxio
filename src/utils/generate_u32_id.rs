use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};

/// A simple counter which is initialized at 0.
static GLOBAL_ID_COUNTER: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

#[inline]
pub fn generate_u32_id() -> u32 {
    GLOBAL_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u32
}
