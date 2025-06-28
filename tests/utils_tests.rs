use muxio::utils::{increment_u32_id, now};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_now_monotonicity() {
    let t1 = now();
    let t2 = now();
    assert!(t2 >= t1, "Timestamp is not monotonic: {t2} < {t1}");
}

#[test]
fn test_now_close_to_system_time() {
    let system_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_micros() as u64;

    let chrono_time = now();

    let delta = system_time.abs_diff(chrono_time);

    // Acceptable skew threshold (e.g., 5 milliseconds)
    assert!(delta < 5_000, "Timestamp delta too large: {delta} µs");
}

#[test]
fn test_increment_u32_id_uniqueness() {
    let mut seen = HashSet::new();

    for _ in 0..10_000 {
        let id = increment_u32_id();
        assert!(seen.insert(id), "Duplicate ID generated: {id}");
    }
}
