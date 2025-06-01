/// Returns the current timestamp in microseconds since the UNIX epoch
/// (January 1, 1970). The implementation differs based on the target
/// architecture, using different methods to retrieve the current time
/// depending on whether the code is running natively or in WebAssembly (WASM).
///
/// # Platform-specific behavior:
///
/// - **For non-WASM targets (`not(target_arch = "wasm32")`)**:
///     - Uses `std::time::SystemTime` to get the current system time.
///     - Computes the duration since the UNIX epoch using
///       `duration_since(UNIX_EPOCH)`.
///     - The duration is converted to microseconds (`as_micros()`) and returned
///       as a `u64` timestamp.
///     - If the calculation fails (e.g., system time issue), it returns `0`
///       as a fallback.
///
/// - **For WASM targets (`target_arch = "wasm32"`)**:
///     - Uses the `web_sys::Performance` API in the browser environment.
///     - Calls `performance.now()`, which returns the current time in
///       milliseconds with high precision.
///     - The milliseconds value is multiplied by `1000` to convert it to
///       microseconds, and the result is returned as a `u64` timestamp.
///
/// # Returns:
/// - A `u64` timestamp representing the current time in microseconds since
///   the UNIX epoch.
///
/// # Example:
/// ```rust
/// use muxio::utils::now;
/// let timestamp = now(); // Gets the current time in microseconds
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Returns the current timestamp in microseconds since the UNIX epoch
/// (January 1, 1970). The implementation differs based on the target
/// architecture, using different methods to retrieve the current time
/// depending on whether the code is running natively or in WebAssembly (WASM).
///
/// # Platform-specific behavior:
///
/// - **For non-WASM targets (`not(target_arch = "wasm32")`)**:
///     - Uses `std::time::SystemTime` to get the current system time.
///     - Computes the duration since the UNIX epoch using
///       `duration_since(UNIX_EPOCH)`.
///     - The duration is converted to microseconds (`as_micros()`) and returned
///       as a `u64` timestamp.
///     - If the calculation fails (e.g., system time issue), it returns `0`
///       as a fallback.
///
/// - **For WASM targets (`target_arch = "wasm32"`)**:
///     - Uses the `web_sys::Performance` API in the browser environment.
///     - Calls `performance.now()`, which returns the current time in
///       milliseconds with high precision.
///     - The milliseconds value is multiplied by `1000` to convert it to
///       microseconds, and the result is returned as a `u64` timestamp.
///
/// # Returns:
/// - A `u64` timestamp representing the current time in microseconds since
///   the UNIX epoch.
#[cfg(target_arch = "wasm32")]
pub fn now() -> u64 {
    use wasm_bindgen::JsCast;

    let binding = js_sys::global();
    let performance = binding.unchecked_ref::<web_sys::Performance>();

    let millis = performance.now();
    (millis * 1000.0) as u64
}
