#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RpcMessageType {
    Call = 0,
    Response = 1,
    Event = 2,
}

impl TryFrom<u8> for RpcMessageType {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, ()> {
        match v {
            0 => Ok(RpcMessageType::Call),
            1 => Ok(RpcMessageType::Response),
            2 => Ok(RpcMessageType::Event),
            _ => Err(()),
        }
    }
}
