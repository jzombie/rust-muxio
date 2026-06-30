[] Ensure, preferrably at compile time, that two methods cannot share the same ID
[] README examples should stop using unwrap.  Do we not have proper error handling?
[] Lots of unwrap handling throughout the library.
[] README examples and any internal documentation should stop hardcoding route IDs. It's a bad pattern.
[] README handling-streaming-requests-on-the-server should work the same on client and server, correct?
[] README should make it clear that it supports events, streaming, etc. (whatever it supports)
[] Hardcoded magic numbers in this PR
[] How does the streaming API detect remote disconnect?
[] Clarify in new constants (like `pub const STREAM_INPUT_METHOD_ID: u64 = rpc_method_id!("session.stream_input");`) that they are session-dependent; every application can define their own.
[] Add more complex muxio-ext-test with multiple streams emit from both parties, with a mix of prebuffered RPC calls. All transports should work the same this way. This would test that streams are actually streaming, multiplexed, and concurrently bidirectional in a single test.
