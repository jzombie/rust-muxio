Reference the HTTP/2 flow-control: https://docs.rs/http2/0.5.19/http2/#flow-control

An HTTP/2 client or server may not send unlimited data to the peer. When a stream is initiated, both the client and the server are provided with an initial window size for that stream. A window size is the number of bytes the endpoint can send to the peer. At any point in time, the peer may increase this window size by sending a WINDOW_UPDATE frame. Once a client or server has sent data filling the window for a stream, no further data may be sent on that stream until the peer increases the window.

There is also a connection level window governing data sent across all streams.

Managing flow control for inbound data is done through FlowControl. Managing flow control for outbound data is done through SendStream. See the struct level documentation for those two types for more details.
