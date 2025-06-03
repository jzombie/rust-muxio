use once_cell::sync::Lazy;
use rand::{RngCore, rng};
use std::sync::atomic::{AtomicU64, Ordering};

static GLOBAL_ID_COUNTER: Lazy<AtomicU64> = Lazy::new(|| {
    let mut thread_range = rng();
    AtomicU64::new(thread_range.next_u64())
});

#[inline]
pub fn generate_u32_id() -> u32 {
    GLOBAL_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_generate_u32_id_uniqueness() {
        let mut seen = HashSet::new();

        for _ in 0..10_000 {
            let id = generate_u32_id();
            assert!(seen.insert(id), "Duplicate ID generated: {}", id);
        }
    }

    #[test]
    fn test_generate_u32_id_rollover() {
        GLOBAL_ID_COUNTER.store(u32::MAX as u64 - 1, Ordering::Relaxed);

        let first = generate_u32_id();
        assert_eq!(first, 4294967294);
        assert_eq!(first, u32::MAX - 1);

        let second = generate_u32_id();
        assert_eq!(second, 4294967295);
        assert_eq!(second, u32::MAX);

        let third = generate_u32_id();
        assert_eq!(third, 0);
        assert_eq!(third, u32::MIN);

        let fourth = generate_u32_id();
        assert_eq!(fourth, 1);
        assert_eq!(fourth, u32::MIN + 1);
    }
}
