use super::*;
use crate::process::{AssetRef32, FileMountStart32, FileTimes32};
#[test]
fn mount_start_is_fixed_bounded_and_legacy_mount_bytes_are_unchanged() {
    let start = FileMountStart32 {
        units: 32767,
        asset: Some(AssetRef32 {
            id: 0x1122334455667788,
            length: 1u64 << 33,
        }),
        attributes: 0x80,
        times: FileTimes32 {
            created: 1,
            accessed: 2,
            written: 3,
        },
    };
    let mut bytes = [0; 64];
    encode_file_mount_start(start, &mut bytes).unwrap();
    assert_eq!(
        &bytes[..16],
        &[82, 51, 70, 66, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(&bytes[36..40], &32767u32.to_le_bytes());
    assert_eq!(decode_file_mount_start(&bytes).unwrap(), start);
    bytes[36..40].copy_from_slice(&32768u32.to_le_bytes());
    assert_eq!(decode_file_mount_start(&bytes), Err(AbiError::Flags));
    let mount = FileMount32 {
        path: "C:\\short".encode_utf16().collect(),
        asset: start.asset,
        attributes: 0x80,
        times: start.times,
    };
    let mut bytes = vec![0; 64 + mount.path.len() * 2];
    encode_file_mount(&mount, &mut bytes).unwrap();
    assert_eq!(&bytes[..4], b"R3FM");
    let decoded = decode_file_mount(&bytes).unwrap();
    assert_eq!(decoded.path, mount.path);
    assert_eq!(decoded.asset, mount.asset);
}
