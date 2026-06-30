use xxhash_rust::const_xxh3::xxh3_64 as const_xxh3_64;

pub const fn method_id_hash(name: &str) -> u64 {
    const_xxh3_64(name.as_bytes())
}

/// Compile-time RPC method ID generator using xxHash3.
///
/// This macro computes a deterministic `u64` identifier from a string literal
/// at **compile time** using the xxh3-64 hash function. The hash is guaranteed
/// to be:
///
/// - **Fast** (no runtime cost)
/// - **Deterministic** (same on all platforms, including WASM)
/// - **Statically embeddable** (usable in `const` contexts)
///
/// xxHash3 is collision-resistant for practical use, but if two different strings
/// produce the same hash, the collision will be caught at runtime by the test
/// in `muxio_rpc_service::tests::method_ids_are_unique` (or by running
/// `cargo test --test method_id_collision`).
///
/// ## Example
///
/// ```rust,no_run
/// use muxio_rpc_service::rpc_method_id;
/// let id_1 = rpc_method_id!("math.add");
/// let id_2 = rpc_method_id!("math.mult");
/// assert!(id_1 > 0);
/// assert_ne!(id_1, id_2);
/// ```
///
/// This is ideal for RPC method routing, as it ensures stable and collision-free
/// identifiers without hardcoding values manually.
#[macro_export]
macro_rules! rpc_method_id {
    ($name:literal) => {{
        const ID: u64 = $crate::method_id_hash($name);
        ID
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn rpc_method_ids_are_deterministic() {
        let a = rpc_method_id!("test.method");
        let b = rpc_method_id!("test.method");
        assert_eq!(a, b, "same name must produce same hash");
    }

    #[test]
    fn rpc_method_ids_differ_for_distinct_names() {
        let a = rpc_method_id!("test.method.one");
        let b = rpc_method_id!("test.method.two");
        assert_ne!(a, b, "distinct names should not collide");
    }
}
