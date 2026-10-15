pub const MEDIA_RECORD_SIZE: usize = 64;

#[derive(Clone, Copy)]
#[repr(u16)]
pub enum MediaRecordKind {
    Session = 1,
    GraphicsResource = 2,
    AudioBuffer = 3,
    AudioVoice = 4,
    GraphicsBatch = 5,
    AudioSubmission = 6,
    AudioState = 7,
    AudioVoiceState = 8,
    InputState = 9,
    InputRelative = 11,
    D3D8Device = 12,
    D3D8Batch = 13,
    WindowsWait = 14,
    WindowsGate = 15,
}

// r3mr v1: kind at 6, byte size at 8, reserved zero at 12, epoch at 16, five u64 values at 24.
pub fn encode_media_record(
    kind: MediaRecordKind,
    epoch: u64,
    values: [u64; 5],
    bytes: &mut [u8; MEDIA_RECORD_SIZE],
) {
    bytes.fill(0);
    bytes[..4].copy_from_slice(b"R3MR");
    bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&(kind as u16).to_le_bytes());
    bytes[8..12].copy_from_slice(&(MEDIA_RECORD_SIZE as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&epoch.to_le_bytes());
    for (chunk, value) in bytes[24..].chunks_exact_mut(8).zip(values) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
}
