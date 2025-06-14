use num_enum::{IntoPrimitive, TryFromPrimitive};

#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
pub enum RpcResultStatus {
    Success = 0,
    Fail = 1,
    SystemError = 2,
    MethodNotFound = 3,
}
