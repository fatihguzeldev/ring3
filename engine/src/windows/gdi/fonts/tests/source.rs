fn word(bytes: &[u8], offset: usize) -> usize {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, part| {
        let mut word = [0; 4];
        word[..part.len()].copy_from_slice(part);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

pub(super) fn private_mono() -> Vec<u8> {
    private_named("Test Mono Face")
}
pub(super) fn private_named(name: &str) -> Vec<u8> {
    assert_eq!(name.len(), 14);
    assert!(name.is_ascii());
    let mut bytes = include_bytes!("../../../../../assets/fonts/NotoSansMono-Regular.ttf").to_vec();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let mut head = 0;
    for index in 0..count {
        let entry = 12 + index * 16;
        let offset = word(&bytes, entry + 8);
        let length = word(&bytes, entry + 12);
        if &bytes[entry..entry + 4] == b"head" {
            head = offset;
        }
        if &bytes[entry..entry + 4] == b"name" {
            for (old, new) in [
                (b"Noto Sans Mono".to_vec(), name.as_bytes().to_vec()),
                (
                    "Noto Sans Mono"
                        .encode_utf16()
                        .flat_map(u16::to_be_bytes)
                        .collect(),
                    name.encode_utf16().flat_map(u16::to_be_bytes).collect(),
                ),
            ] {
                assert_eq!(old.len(), new.len());
                for at in offset..offset + length - old.len() + 1 {
                    if bytes[at..at + old.len()] == old {
                        bytes[at..at + old.len()].copy_from_slice(&new);
                    }
                }
            }
            let value = checksum(&bytes[offset..offset + length]);
            bytes[entry + 4..entry + 8].copy_from_slice(&value.to_be_bytes());
        }
    }
    assert_ne!(head, 0);
    bytes[head + 8..head + 12].fill(0);
    let adjustment = 0xb1b0_afbau32.wrapping_sub(checksum(&bytes));
    bytes[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    bytes
}

pub(super) fn collection(fonts: &[&[u8]]) -> Vec<u8> {
    let mut bytes = b"ttcf\0\x01\0\0".to_vec();
    bytes.extend_from_slice(&(fonts.len() as u32).to_be_bytes());
    bytes.resize(12 + fonts.len() * 4, 0);
    for (index, font) in fonts.iter().enumerate() {
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        let start = bytes.len();
        bytes[12 + index * 4..16 + index * 4].copy_from_slice(&(start as u32).to_be_bytes());
        bytes.extend_from_slice(font);
        let tables = u16::from_be_bytes(font[4..6].try_into().unwrap()) as usize;
        for table in 0..tables {
            let entry = 12 + table * 16;
            let offset = word(font, entry + 8);
            bytes[start + entry + 8..start + entry + 12]
                .copy_from_slice(&((start + offset) as u32).to_be_bytes());
        }
    }
    bytes
}
