// TODO: Move this out of the main `Muxio` crate, as this crate should be considered lower-level

#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RpcResultStatus {
    Success = 0,
    Fail = 1,
    SystemError = 2,
}

impl RpcResultStatus {
    #[inline]
    pub fn value(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for RpcResultStatus {
    type Error = ();

    #[inline]
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            x if x == RpcResultStatus::Success as u8 => Ok(RpcResultStatus::Success),
            x if x == RpcResultStatus::Fail as u8 => Ok(RpcResultStatus::Fail),
            x if x == RpcResultStatus::SystemError as u8 => Ok(RpcResultStatus::SystemError),
            _ => Err(()),
        }
    }
}
