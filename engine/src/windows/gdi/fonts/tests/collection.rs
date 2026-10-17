use super::super::{FontDatabase32, LogicalFont32};
use crate::windows::gdi::GdiError32;

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

fn private_mono() -> Vec<u8> {
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
                (b"Noto Sans Mono".to_vec(), b"Test Mono Face".to_vec()),
                (
                    "Noto Sans Mono"
                        .encode_utf16()
                        .flat_map(u16::to_be_bytes)
                        .collect(),
                    "Test Mono Face"
                        .encode_utf16()
                        .flat_map(u16::to_be_bytes)
                        .collect(),
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

fn collection(fonts: &[&[u8]]) -> Vec<u8> {
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

#[test]
fn memory_collection_installs_the_second_real_face_and_removal_retires_the_group() {
    let mono = private_mono();
    let bytes = collection(&[
        include_bytes!("../../../../../assets/fonts/NotoSans-Regular.ttf"),
        &mono,
    ]);
    assert_eq!(rustybuzz::ttf_parser::fonts_in_collection(&bytes), Some(2));
    let parsed = rustybuzz::ttf_parser::Face::parse(&bytes, 1).unwrap();
    let fixed_pitch = parsed.is_monospaced();
    assert_eq!(
        parsed.glyph_hor_advance(parsed.glyph_index('i').unwrap()),
        parsed.glyph_hor_advance(parsed.glyph_index('M').unwrap())
    );
    let mut fonts = FontDatabase32::new(4 << 20).unwrap();
    let resource = fonts.add_memory_font(&bytes).unwrap();
    let selected = LogicalFont32 {
        height: -20,
        face: "Test Mono Face".encode_utf16().collect(),
        ..Default::default()
    };
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&selected)),
        "Test Mono Face"
    );
    assert_eq!(fonts.metrics(&selected).unwrap().fixed_pitch, fixed_pitch);
    let glyph = fonts.glyph_index(&selected, 'M');
    let raster = fonts.rasterize(&selected, glyph, 65_536).unwrap();
    assert!(raster.coverage.iter().any(|value| *value != 0));
    fonts.remove_memory_font(resource).unwrap();
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&selected)),
        "Noto Sans Mono"
    );
}

#[test]
fn malformed_second_collection_face_does_not_publish_the_valid_first_face() {
    let mono = private_mono();
    let mut bytes = collection(&[
        &mono,
        include_bytes!("../../../../../assets/fonts/NotoSans-Regular.ttf"),
    ]);
    bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(rustybuzz::ttf_parser::Face::parse(&bytes, 0).is_ok());
    assert!(rustybuzz::ttf_parser::Face::parse(&bytes, 1).is_err());
    let mut fonts = FontDatabase32::new(4 << 20).unwrap();
    let font = LogicalFont32 {
        face: "Test Mono Face".encode_utf16().collect(),
        ..Default::default()
    };
    assert_eq!(
        fonts.add_memory_font(&bytes),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&font)),
        "Noto Sans Mono"
    );
    let registered = fonts.add_memory_font(&mono).unwrap();
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&font)),
        "Test Mono Face"
    );
    fonts.remove_memory_font(registered).unwrap();
}

#[test]
fn collection_source_bytes_are_charged_once_and_group_retirement_restores_budget() {
    let mono = private_mono();
    let bytes = collection(&[
        include_bytes!("../../../../../assets/fonts/NotoSans-Regular.ttf"),
        &mono,
    ]);
    let mut fonts = FontDatabase32::new(1_082_044 + bytes.len()).unwrap();
    let resource = fonts.add_memory_font(&bytes).unwrap();
    assert_eq!(fonts.add_memory_font(&mono), Err(GdiError32::Capacity));
    fonts.remove_memory_font(resource).unwrap();
    let replacement = fonts.add_memory_font(&bytes).unwrap();
    assert_ne!(replacement, resource);
    fonts.remove_memory_font(replacement).unwrap();
}
