# Muxio Ext Test

This crate is intended for testing of crates that would otherwise have circular dependencies (and therefore not publishable).

## Test discovery

Any Rust files placed under the `tests/` directory (recursively) will be auto-included
when running `cargo test`. A build script (`build.rs`) generates `tests/auto_tests.rs`
which declares modules for each discovered `*.rs` file. Do not edit `tests/auto_tests.rs`
manually; it is regenerated on test builds.

If you need to add tests, create `.rs` files under `tests/` (for example `tests/foo/bar.rs`).
The test runner will generate module names automatically.
