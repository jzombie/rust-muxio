use std::io::Result;
use tokio::net::TcpListener;

/// Creates a `TcpListener` and binds it to a random, available port on the
/// local loopback address (`127.0.0.1`).
///
/// This utility is useful for test environments or applications where services
/// need to start on a guaranteed-free port without manual configuration.
pub async fn bind_tcp_listener_on_random_port() -> Result<(TcpListener, u16)> {
    // Bind to port 0 on the loopback address. The OS will replace 0 with
    // an available ephemeral port.
    let listener = TcpListener::bind("127.0.0.1:0").await?;

    // Retrieve the actual address, including the assigned port.
    let port = listener.local_addr()?.port();

    Ok((listener, port))
}
