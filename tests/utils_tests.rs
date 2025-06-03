// Note: `generate_u32_id` tests are colocated with the implementation
// to allow direct access to the local static counter for controlled testing

use muxio::utils::now;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_now_monotonicity() {
    let t1 = now();
    let t2 = now();
    assert!(t2 >= t1, "Timestamp is not monotonic: {} < {}", t2, t1);
}

#[test]
fn test_now_close_to_system_time() {
    let system_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_micros() as u64;

    let chrono_time = now();

    let delta = if system_time > chrono_time {
        system_time - chrono_time
    } else {
        chrono_time - system_time
    };

    // Acceptable skew threshold (e.g., 5 milliseconds)
    assert!(delta < 5_000, "Timestamp delta too large: {} µs", delta);
}
