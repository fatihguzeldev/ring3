use crate::input::dinput::haptics::HapticFrame32;

pub const HAPTIC_FRAME_SIZE: usize = 80;

pub fn encode_haptic_frame(frame: &HapticFrame32, bytes: &mut [u8; HAPTIC_FRAME_SIZE]) {
    bytes.fill(0);
    bytes[..4].copy_from_slice(b"R3HF");
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&(HAPTIC_FRAME_SIZE as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&frame.epoch.to_le_bytes());
    bytes[24..32].copy_from_slice(&frame.sequence.to_le_bytes());
    bytes[32..40].copy_from_slice(&frame.timestamp_ns.to_le_bytes());
    bytes[40..44].copy_from_slice(&u32::from(frame.slot).to_le_bytes());
    for (bytes, force) in bytes[48..].chunks_exact_mut(4).zip(frame.forces) {
        bytes.copy_from_slice(&force.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_packet_preserves_signed_forces_and_reserved_words() {
        let frame = HapticFrame32 {
            epoch: 0x0807060504030201,
            sequence: 0x1817161514131211,
            timestamp_ns: 0x2827262524232221,
            slot: 7,
            forces: [-10000, 0, 10000, -1, 1, 0, 0, 0],
        };
        let mut bytes = [0xa5; HAPTIC_FRAME_SIZE];
        encode_haptic_frame(&frame, &mut bytes);
        assert_eq!(
            &bytes[..16],
            &[82, 51, 72, 70, 1, 0, 0, 0, 80, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            &bytes[16..40],
            &[
                1, 2, 3, 4, 5, 6, 7, 8, 17, 18, 19, 20, 21, 22, 23, 24, 33, 34, 35, 36, 37, 38, 39,
                40
            ]
        );
        assert_eq!(&bytes[40..48], &[7, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            &bytes[48..68],
            &[
                240, 216, 255, 255, 0, 0, 0, 0, 16, 39, 0, 0, 255, 255, 255, 255, 1, 0, 0, 0
            ]
        );
        assert_eq!(&bytes[68..], &[0; 12]);
    }
}
