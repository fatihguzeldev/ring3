use super::*;

pub fn vertex(position: [f32; 4], rgba8: u32) -> Vec<u8> {
    position
        .into_iter()
        .map(f32::to_bits)
        .chain([rgba8])
        .flat_map(u32::to_le_bytes)
        .collect()
}

pub fn triangle() -> Vec<u8> {
    [
        ([-0.5, -0.5, 0.5, 1.0], 0xff0000ff),
        ([0.5, -0.5, 0.5, 1.0], 0xff00ff00),
        ([0.0, 0.5, 0.5, 1.0], 0xffff0000),
    ]
    .into_iter()
    .flat_map(|(position, color)| vertex(position, color))
    .collect()
}

pub fn context(resources: u32, resource_bytes: u32, batch_bytes: u32) -> GraphicsContext {
    GraphicsContext::new(
        7,
        GraphicsLimits {
            resources,
            resource_bytes,
            batch_bytes,
        },
    )
    .unwrap()
}

pub fn u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
pub fn u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
pub fn id(bytes: &[u8]) -> ResourceId {
    ResourceId {
        epoch: u64(bytes, 0),
        slot: u32(bytes, 8),
        generation: u32(bytes, 12),
    }
}

pub fn records(bytes: &[u8], batch: BatchId) -> Vec<(u16, &[u8])> {
    assert_eq!(&bytes[..8], b"R3GB\x01\0\x01\0");
    assert_eq!(u32(bytes, 8) as usize, bytes.len());
    assert_eq!(u64(bytes, 16), batch.epoch);
    assert_eq!(u64(bytes, 24), batch.sequence);
    assert_eq!(&bytes[32..40], &[0; 8]);
    let mut records = Vec::new();
    let mut at = 40;
    while at < bytes.len() {
        assert!(at + 8 <= bytes.len());
        assert_eq!(&bytes[at + 2..at + 4], &[0, 0]);
        let size = u32(bytes, at + 4) as usize;
        assert!(size >= 8 && size.is_multiple_of(4) && at + size <= bytes.len());
        records.push((
            u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()),
            &bytes[at + 8..at + size],
        ));
        at += size;
    }
    assert_eq!(at, bytes.len());
    assert_eq!(records.len(), u32(bytes, 12) as usize);
    records
}
