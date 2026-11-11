use crate::{
    ServerPacket, VarInt,
    ser::{NetworkReadExt, ReadingError},
};
use pumpkin_data::packet::serverbound::play::DEBUG_SUBSCRIPTION_REQUEST;
use pumpkin_macros::java_packet;
use pumpkin_util::version::JavaMinecraftVersion;

/// The set of debug values a client wants from now on.
///
/// Vanilla `ServerboundDebugSubscriptionRequestPacket`: a set of `debug_subscription` registry
/// ids, read as a `VarInt` count of at most 32 and then the ids. Each request replaces the
/// previous set; an empty one ends every subscription.
#[java_packet(DEBUG_SUBSCRIPTION_REQUEST)]
pub struct SDebugSubscriptionRequest {
    pub subscriptions: Vec<VarInt>,
}

impl SDebugSubscriptionRequest {
    /// `minecraft:dedicated_server_tick_time`, the first registered debug subscription.
    pub const TICK_TIME: i32 = 0;
    /// Vanilla reads the set with `ByteBufCodecs.collection(.., 32)`.
    pub const MAX_SUBSCRIPTIONS: usize = 32;
}

impl<'a> ServerPacket<'a> for SDebugSubscriptionRequest {
    fn read(bytebuf: &mut &'a [u8], _version: &JavaMinecraftVersion) -> Result<Self, ReadingError> {
        let count = bytebuf.get_var_int()?.0;
        let count = usize::try_from(count)
            .ok()
            .filter(|count| *count <= Self::MAX_SUBSCRIPTIONS)
            .ok_or_else(|| {
                ReadingError::TooLarge(format!(
                    "{count} debug subscriptions (at most {})",
                    Self::MAX_SUBSCRIPTIONS
                ))
            })?;
        let mut subscriptions = Vec::with_capacity(count);
        for _ in 0..count {
            subscriptions.push(bytebuf.get_var_int()?);
        }
        Ok(Self { subscriptions })
    }
}

impl crate::ClientPacket for SDebugSubscriptionRequest {
    fn write_packet_data(
        &self,
        mut write: impl std::io::Write,
        _version: &JavaMinecraftVersion,
    ) -> Result<(), crate::ser::WritingError> {
        use crate::ser::NetworkWriteExt;
        let count = i32::try_from(self.subscriptions.len())
            .map_err(|_| crate::ser::WritingError::Message("too many subscriptions".into()))?;
        write.write_var_int(&VarInt(count))?;
        for subscription in &self.subscriptions {
            write.write_var_int(subscription)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SDebugSubscriptionRequest;
    use crate::ServerPacket;
    use pumpkin_util::version::JavaMinecraftVersion;

    fn read(bytes: &[u8]) -> Option<Vec<i32>> {
        let mut buf = bytes;
        SDebugSubscriptionRequest::read(&mut buf, &JavaMinecraftVersion::V_26_3)
            .ok()
            .map(|packet| packet.subscriptions.iter().map(|id| id.0).collect())
    }

    #[test]
    fn reads_the_set_vanilla_sends() {
        // The F3 tick chart: one subscription, id 0 (dedicated_server_tick_time).
        assert_eq!(read(&[1, 0]), Some(vec![0]));
        // Closing it: the empty set.
        assert_eq!(read(&[0]), Some(Vec::new()));
        assert_eq!(read(&[2, 0, 5]), Some(vec![0, 5]));
    }

    #[test]
    fn refuses_more_than_vanilla_allows() {
        let mut most = vec![32];
        most.extend([0; 32]);
        assert_eq!(read(&most).map(|ids| ids.len()), Some(32));
        let mut too_many = vec![33];
        too_many.extend([0; 33]);
        assert_eq!(read(&too_many), None);
        // A negative count (VarInt -1).
        assert_eq!(read(&[0xff, 0xff, 0xff, 0xff, 0x0f]), None);
        // A count larger than the bytes that follow.
        assert_eq!(read(&[2, 0]), None);
    }
}
