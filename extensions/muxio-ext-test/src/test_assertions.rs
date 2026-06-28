use muxio_rpc_service::error::{RpcServiceError, RpcServiceErrorCode};
use muxio_rpc_service_caller::RpcTransportState;

pub fn assert_rpc_success<T>(result: Result<T, impl std::fmt::Debug>, expected: &T)
where
    T: PartialEq + std::fmt::Debug,
{
    let value = result.unwrap_or_else(|e| panic!("Expected Ok, got Err({:?})", e));
    assert_eq!(&value, expected);
}

pub fn assert_rpc_error<T: std::fmt::Debug>(
    result: Result<T, RpcServiceError>,
    expected_code: RpcServiceErrorCode,
    expected_message_substring: &str,
) {
    let err = result.unwrap_err();
    match err {
        RpcServiceError::Rpc(payload) => {
            assert_eq!(
                payload.code, expected_code,
                "Expected error code {:?}, got {:?}",
                expected_code, payload.code
            );
            assert!(
                payload.message.contains(expected_message_substring),
                "Expected error message to contain '{}', got '{}'",
                expected_message_substring,
                payload.message
            );
        }
        other => panic!("Expected RpcServiceError::Rpc, got {:?}", other),
    }
}

pub fn assert_method_not_found_error<T: std::fmt::Debug>(result: Result<T, RpcServiceError>) {
    let err = result.unwrap_err();
    match err {
        RpcServiceError::Rpc(payload) => {
            assert_eq!(
                payload.code,
                RpcServiceErrorCode::NotFound,
                "Expected NotFound error code, got {:?}",
                payload.code
            );
        }
        other => panic!("Expected RpcServiceError::Rpc, got {:?}", other),
    }
}

pub fn assert_state_change_handler_called(
    received_states: &[RpcTransportState],
    expected: &[RpcTransportState],
) {
    assert_eq!(
        received_states, expected,
        "State change handler received {:?}, expected {:?}",
        received_states, expected
    );
}
