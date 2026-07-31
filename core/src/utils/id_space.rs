/// Partition of the connection-level request/stream id space.
///
/// Both ends of a connection allocate ids (`request_id`, `stream_id`) from a
/// process-global counter, so a client-allocated id can numerically equal a
/// server-allocated id. Routing tables on each side (`response_handlers`,
/// per-stream decoders) are keyed by these ids, so such a collision silently
/// overwrites one route with another — e.g. a server-initiated `OnPtyResized`
/// call can clobber a client's `STREAM_INPUT` handler, killing that client's
/// input while its output keeps flowing.
///
/// To make collisions impossible, the id space is split in half: clients
/// allocate ids with the high bit clear, servers with the high bit set. Both
/// ends apply the same split, so the two allocators can never overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdSpace {
    Client,
    Server,
}

impl IdSpace {
    /// High bit that divides the id space between client and server.
    const MASK: u32 = 0x8000_0000;

    /// The direction marker for this space.
    pub fn marker(self) -> u32 {
        match self {
            IdSpace::Client => 0,
            IdSpace::Server => Self::MASK,
        }
    }

    /// Place an arbitrary (process-global) id into this space's half.
    pub fn place(self, id: u32) -> u32 {
        (id & !Self::MASK) | self.marker()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_and_server_spaces_never_collide() {
        // A client-placed and server-placed id must never be equal, for any
        // underlying id — otherwise a server-initiated request could overwrite
        // a client stream's route on the same connection.
        for id in [
            0u32,
            1,
            42,
            0x7fff_ffff,
            0x8000_0000,
            u32::MAX / 2,
            u32::MAX,
        ] {
            assert_ne!(
                IdSpace::Client.place(id),
                IdSpace::Server.place(id),
                "id {id} collides across id spaces"
            );
        }
    }

    #[test]
    fn place_uses_the_high_bit_as_the_direction_marker() {
        assert_eq!(IdSpace::Client.place(0), 0);
        assert_eq!(IdSpace::Server.place(0), 0x8000_0000);
        // The low 31 bits are preserved; only the marker bit differs.
        assert_eq!(
            IdSpace::Client.place(0x1234_5678) & 0x7fff_ffff,
            0x1234_5678
        );
        assert_eq!(
            IdSpace::Server.place(0x1234_5678) & 0x7fff_ffff,
            0x1234_5678
        );
    }

    #[test]
    fn placed_ids_remain_unique_within_a_space() {
        let mut seen = std::collections::HashSet::new();
        for id in 0..1000u32 {
            assert!(seen.insert(IdSpace::Client.place(id)));
        }
    }
}
