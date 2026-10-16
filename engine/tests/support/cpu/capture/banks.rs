pub struct CapturePart {
    pub bytes: Vec<u8>,
    pub offset: usize,
    pub original_bytes: usize,
    pub original_instructions: usize,
    pub instructions: usize,
}

// literal lengths come from each authored bank, independently of the engine decoder.
pub fn bounded_parts(bytes: &[u8], lengths: &[usize]) -> Vec<CapturePart> {
    assert!(!lengths.is_empty());
    assert!(lengths.iter().all(|length| (1..=15).contains(length)));
    assert_eq!(lengths.iter().sum::<usize>(), bytes.len());
    assert_eq!(lengths.last(), Some(&2));
    assert!(bytes.ends_with(&[0xeb, 0xfe]));
    let mut offset = 0;
    let parts: Vec<_> = lengths
        .chunks(8)
        .map(|lengths| {
            let length: usize = lengths.iter().sum();
            let end = offset + length;
            let mut part_bytes = bytes[offset..end].to_vec();
            let terminal = end == bytes.len();
            if !terminal {
                part_bytes.extend([0xeb, 0xfe]);
            }
            let part = CapturePart {
                bytes: part_bytes,
                offset,
                original_bytes: length,
                original_instructions: lengths.len(),
                instructions: lengths.len() + usize::from(!terminal),
            };
            offset = end;
            part
        })
        .collect();
    assert_eq!(offset, bytes.len());
    assert_eq!(
        parts
            .iter()
            .flat_map(|part| &part.bytes[..part.original_bytes])
            .copied()
            .collect::<Vec<_>>(),
        bytes
    );
    assert_eq!(
        parts
            .iter()
            .map(|part| part.original_instructions)
            .sum::<usize>(),
        lengths.len()
    );
    parts
}
