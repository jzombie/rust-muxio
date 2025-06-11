use super::RpcStreamEvent;
use crate::frame::FrameDecodeError;

pub trait RpcEmit: FnMut(&[u8]) {}
impl<T: FnMut(&[u8])> RpcEmit for T {}

pub trait RpcResponseHandler: FnMut(RpcStreamEvent) + Send {}
impl<T: FnMut(RpcStreamEvent) + Send> RpcResponseHandler for T {}

pub trait RpcStreamEventDecoderHandler:
    FnMut(RpcStreamEvent) -> Result<(), FrameDecodeError>
{
}
impl<T: FnMut(RpcStreamEvent) -> Result<(), FrameDecodeError>> RpcStreamEventDecoderHandler for T {}
