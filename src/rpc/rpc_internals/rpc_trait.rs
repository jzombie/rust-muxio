use super::RpcStreamEvent;

pub trait RpcEmit: FnMut(&[u8]) {}
impl<T: FnMut(&[u8])> RpcEmit for T {}

pub trait RpcResponseHandler: FnMut(RpcStreamEvent) + Send {}
impl<T: FnMut(RpcStreamEvent) + Send> RpcResponseHandler for T {}

pub trait RpcStreamEventHandler: FnMut(RpcStreamEvent) {}
impl<T: FnMut(RpcStreamEvent)> RpcStreamEventHandler for T {}
