#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcTransportState {
    Connected,
    Disconnected,
    Connecting,
}
