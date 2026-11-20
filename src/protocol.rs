use crc32fast::Hasher;
use uuid::Uuid;

/// 16-bit UUID `00fa` expanded to standard 128-bit Bluetooth Base UUID.
pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x000000fa_0000_1000_8000_00805f9b34fb);
/// 16-bit UUID `fa02` expanded to standard 128-bit Bluetooth Base UUID.
pub const WRITE_CHAR_UUID: Uuid = Uuid::from_u128(0x0000fa02_0000_1000_8000_00805f9b34fb);

/// Block size used by `AppConfig.ledFrameSize` (4096 bytes).
pub const FRAME_BLOCK_SIZE: usize = 4096;

/// Temporary direct-play slot ID (`isDown == false` sets `payload[14] = 101`).
pub const DIRECT_PLAY_SLOT: u8 = 101;

/// Maximum playlist slots supported by `ChannelIndex` (`1..=100`).
pub const MAX_PLAYLIST_SLOTS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckStatus {
    InvalidCommand,
    ContinueNextBlock,
    OutOfMemory,
    Completed,
    Unknown(u8),
}

/// Computes standard CRC32 over the full GIF payload.
pub fn compute_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Builds a single GIF data block frame matching `SendCore.payloadChannel` (`type = 3`).
///
/// `slot` is `1..=100` when saving to device memory/playlist, or `DIRECT_PLAY_SLOT` (101)
/// for direct temporary playback.
pub fn build_gif_block_packet(
    block: &[u8],
    total_len: usize,
    crc32: u32,
    is_continuation: bool,
    slot: u8,
) -> Vec<u8> {
    let frame_len = (block.len() + 15) as u16;
    let option = if is_continuation { 0x02 } else { 0x00 };

    let mut packet = Vec::with_capacity(15 + block.len());
    packet.extend_from_slice(&frame_len.to_le_bytes()); // [0..2]
    packet.extend_from_slice(&[0x03, 0x00]); // [2..4] TYPE_GIF
    packet.push(option); // [4] 0 = first, 2 = continuation
    packet.extend_from_slice(&(total_len as u32).to_le_bytes()); // [5..9] total GIF size
    packet.extend_from_slice(&crc32.to_le_bytes()); // [9..13] CRC32 of full GIF
    packet.push(0x02); // [13] GIF marker
    packet.push(slot); // [14] 1..=100 or 101
    packet.extend_from_slice(block); // [15..]
    packet
}

/// Pre-check command sent before uploading a single GIF on MCU > 4 (`AnimFragment.itemClick`).
pub fn build_precheck_slot_cmd(slot: u8) -> [u8; 7] {
    [0x07, 0x00, 0x08, 0x80, 0x01, 0x00, slot]
}

/// Playlist header command sent before uploading queued items (`ChannelListActivity.SendChannel`).
pub fn build_playlist_header_cmd(slots: &[u8]) -> Vec<u8> {
    let total_len = (6 + slots.len()) as u8;
    let mut cmd = Vec::with_capacity(6 + slots.len());
    cmd.push(total_len);
    cmd.push(0x00);
    cmd.push(0x08);
    cmd.push(0x80);
    cmd.push(slots.len() as u8);
    cmd.push(0x00);
    cmd.extend_from_slice(slots);
    cmd
}

/// Deletes specific channel slots from device memory (`SendCore.sendChannelDelIndex`).
pub fn build_delete_slots_cmd(slots: &[u8]) -> Vec<u8> {
    let total_len = (6 + slots.len()) as u16;
    let count = slots.len() as u16;
    let mut cmd = Vec::with_capacity(6 + slots.len());
    cmd.extend_from_slice(&total_len.to_le_bytes());
    cmd.extend_from_slice(&[0x02, 0x01]);
    cmd.extend_from_slice(&count.to_le_bytes());
    cmd.extend_from_slice(slots);
    cmd
}

/// Clears all stored channels from device memory (`BaseSend.deleteAllData`).
pub fn build_delete_all_cmd() -> [u8; 4] {
    [0x04, 0x00, 0x03, 0x80]
}

/// Turns the LED matrix on or off (`BaseSend.sendLedOnOff`).
pub fn build_power_cmd(on: bool) -> [u8; 5] {
    [0x05, 0x00, 0x07, 0x01, u8::from(on)]
}

/// Sets LED brightness (`1..=100`) (`BaseSend.setLedLight`).
pub fn build_brightness_cmd(brightness: u8) -> [u8; 5] {
    [0x05, 0x00, 0x04, 0x80, brightness.clamp(1, 100)]
}

/// Parses a 5-byte notification ACK from the panel.
pub fn parse_ack(data: &[u8]) -> Option<AckStatus> {
    if data.len() == 5 {
        Some(match data[4] {
            0 => AckStatus::InvalidCommand,
            1 => AckStatus::ContinueNextBlock,
            2 => AckStatus::OutOfMemory,
            3 => AckStatus::Completed,
            other => AckStatus::Unknown(other),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gif_block_packet_layout() {
        let full = b"GIF89a_test_payload";
        let crc = compute_crc32(full);
        let pkt = build_gif_block_packet(&full[..6], full.len(), crc, false, 101);
        assert_eq!(pkt.len(), 15 + 6);
        assert_eq!(u16::from_le_bytes([pkt[0], pkt[1]]), 21);
        assert_eq!(&pkt[2..4], &[0x03, 0x00]);
        assert_eq!(pkt[4], 0x00);
        assert_eq!(
            u32::from_le_bytes([pkt[5], pkt[6], pkt[7], pkt[8]]),
            full.len() as u32
        );
        assert_eq!(u32::from_le_bytes([pkt[9], pkt[10], pkt[11], pkt[12]]), crc);
        assert_eq!(pkt[13], 0x02);
        assert_eq!(pkt[14], 101);
        assert_eq!(&pkt[15..], b"GIF89a");
    }

    #[test]
    fn test_playlist_and_control_cmds() {
        assert_eq!(build_playlist_header_cmd(&[1, 2, 3]), vec![9, 0, 8, 0x80, 3, 0, 1, 2, 3]);
        assert_eq!(build_delete_all_cmd(), [4, 0, 3, 0x80]);
        assert_eq!(build_power_cmd(true), [5, 0, 7, 1, 1]);
        assert_eq!(build_brightness_cmd(75), [5, 0, 4, 0x80, 75]);
        assert_eq!(parse_ack(&[5, 0, 3, 0, 1]), Some(AckStatus::ContinueNextBlock));
        assert_eq!(parse_ack(&[5, 0, 3, 0, 3]), Some(AckStatus::Completed));
    }
}
