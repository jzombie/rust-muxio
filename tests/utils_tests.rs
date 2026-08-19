use muxio::utils::increment_u32_id;
use std::collections::HashSet;

#[test]
fn test_increment_u32_id_uniqueness() {
    let mut seen = HashSet::new();

    for _ in 0..10_000 {
        let id = increment_u32_id();
        assert!(seen.insert(id), "Duplicate ID generated: {id}");
    }
}
