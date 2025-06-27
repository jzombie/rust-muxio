use std::io::Result;
use std::net::{IpAddr, SocketAddr};
use tokio::net::TcpListener;

/// Extracts the IP address and port from a bound `TcpListener`.
///
/// This helper function is particularly useful when a `TcpListener` has been
/// bound to an ephemeral port (port 0), as it provides a convenient way
/// to retrieve the actual OS-assigned port number and local IP address.
pub fn tcp_listener_to_host_port(listener: &TcpListener) -> Result<(IpAddr, u16)> {
    let remote_addr: SocketAddr = listener.local_addr()?;
    let server_host: IpAddr = remote_addr.ip();
    let server_port: u16 = remote_addr.port();

    Ok((server_host, server_port))
}
