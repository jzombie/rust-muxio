#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Connected,
    Disconnected,
    Connecting,
}
