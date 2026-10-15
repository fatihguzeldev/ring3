use super::*;
use crate::process::{OverlayEntry32, OverlayKind32};
#[test]
fn long_overlay_metadata_retains_total_path_and_short_wire_stays_literal() {
    let entry = OverlayEntry32 {
        epoch: 7,
        revision: 9,
        path: vec![97; 3000],
        info: None,
        kind: OverlayKind32::Deleted,
    };
    let mut bytes = [0; 96];
    encode_file_overlay_metadata(Some(&entry), &mut bytes).unwrap();
    assert_eq!(
        &bytes[..16],
        &[82, 51, 70, 79, 1, 0, 1, 0, 96, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(&bytes[52..64], &[3, 0, 0, 0, 184, 11, 0, 0, 1, 0, 0, 0]);
    let short = OverlayEntry32 {
        path: vec![0xd800, 98],
        ..entry
    };
    let mut bytes = [0; 100];
    encode_file_overlay(Some(&short), &mut bytes).unwrap();
    assert_eq!(&bytes[56..64], &[2, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&bytes[96..], &[0, 216, 98, 0]);
    assert_eq!(
        encode_file_overlay(Some(&short), &mut [0; 96]),
        Err(AbiError::Length)
    );
}
