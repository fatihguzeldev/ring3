use super::*;
fn node(key: &str, text: bool, value: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0; 6];
    out[2..4].copy_from_slice(&((value.len() / if text { 2 } else { 1 }) as u16).to_le_bytes());
    out[4..6].copy_from_slice(&u16::from(text).to_le_bytes());
    for c in key.encode_utf16().chain([0]) {
        out.extend_from_slice(&c.to_le_bytes());
    }
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out.extend_from_slice(value);
    for child in children {
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out.extend_from_slice(child);
    }
    let length = out.len() as u16;
    out[..2].copy_from_slice(&length.to_le_bytes());
    out
}
pub(crate) fn fixture() -> Vec<u8> {
    let text = "製品"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let string = node("ProductName", true, &text, &[]);
    let strings = node(
        "StringFileInfo",
        true,
        &[],
        &[node("041104b0", true, &[], &[string])],
    );
    let variables = node(
        "VarFileInfo",
        true,
        &[],
        &[node("Translation", false, &[0x11, 4, 0xb0, 4], &[])],
    );
    let mut fixed = vec![0; 52];
    fixed[..4].copy_from_slice(&0xfeef_04bdu32.to_le_bytes());
    fixed[8..12].copy_from_slice(&0x0002_0003u32.to_le_bytes());
    node("VS_VERSION_INFO", false, &fixed, &[strings, variables])
}
#[test]
fn queries_borrow_exact_binary_and_counted_utf16_values_inside_original_block() {
    let bytes = fixture();
    let block = VersionBlock32::parse(&bytes).unwrap();
    let root = block.query(&[b'\\' as u16]).unwrap();
    assert_eq!(root.length, 52);
    assert!(!root.text);
    let translation = block
        .query(
            &"\\varfileinfo\\translation"
                .encode_utf16()
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        &bytes[translation.offset..translation.offset + translation.length],
        &[0x11, 4, 0xb0, 4]
    );
    let value = block
        .query(
            &"\\StringFileInfo\\041104b0\\ProductName"
                .encode_utf16()
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert!(value.text);
    assert_eq!(value.length, 3);
    assert_eq!(
        &bytes[value.offset..value.offset + 6],
        &[0xfd, 0x88, 0xc1, 0x54, 0, 0]
    );
    assert_eq!(
        block.query(
            &"\\StringFileInfo\\040904e4\\ProductName"
                .encode_utf16()
                .collect::<Vec<_>>()
        ),
        Err(VersionError32::NotFound)
    );
}
#[test]
fn malformed_parent_lengths_signature_and_truncated_values_are_rejected() {
    let bytes = fixture();
    for n in [0, 1, 7, bytes.len() - 1] {
        assert!(VersionBlock32::parse(&bytes[..n]).is_err());
    }
    let mut bad = bytes.clone();
    bad[40] = 0;
    assert_eq!(
        VersionBlock32::parse(&bad).err(),
        Some(VersionError32::InvalidData)
    );
    assert_eq!(VersionApi32::GetFileVersionInfoW.stack_words(), 4);
    assert_eq!(VersionApi32::VerQueryValueA.stack_words(), 4);
    assert_eq!(VersionApi32::from_id(0x001b_0007), None);
}
