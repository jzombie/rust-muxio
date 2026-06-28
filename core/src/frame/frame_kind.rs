use std::convert::TryFrom;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Open = 0,
    Data = 1,
    End = 2,
    Cancel = 3,
    Pong = 4,
    Ping = 5,
}

impl TryFrom<u8> for FrameKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(FrameKind::Open),
            1 => Ok(FrameKind::Data),
            2 => Ok(FrameKind::End),
            3 => Ok(FrameKind::Cancel),
            4 => Ok(FrameKind::Pong),
            5 => Ok(FrameKind::Ping),
            _ => Err(()),
        }
    }
}
