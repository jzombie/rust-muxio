[] Ensure, preferrably at compile time, that two methods cannot share the same ID
[] README examples should stop using unwrap.  Do we not have proper error handling?
[] README examples and any internal documentation should stop hardcoding route IDs. It's a bad pattern.
[] README handling-streaming-requests-on-the-server should work the same on client and server, correct?
[] Hardcoded magic numbers in this PR
[] How does the streaming API detect remote disconnect?
[] Clarify in new constants (like `pub const STREAM_INPUT_METHOD_ID: u64 = rpc_method_id!("session.stream_input");`) that they are session-dependent; every application can define their own.
