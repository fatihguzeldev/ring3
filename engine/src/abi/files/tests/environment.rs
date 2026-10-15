use super::*;
use crate::process::FileEnvironment32;
#[test]
fn environment_codec_preserves_utf16_and_staged_lengths_without_unbounded_records() {
    let mut environment = FileEnvironment32::default();
    environment.search_path = vec![
        vec![67, 58, 92, 0xd800],
        "C:\\games".encode_utf16().collect(),
    ];
    let lengths: Vec<u32> = [
        &environment.application_directory,
        &environment.windows_directory,
        &environment.system_directory,
        &environment.temp_directory,
    ]
    .into_iter()
    .chain(environment.search_path.iter())
    .map(|p| p.len() as u32)
    .collect();
    let mut bytes = vec![0; 32 + lengths.len() * 4 + lengths.iter().sum::<u32>() as usize * 2];
    encode_file_environment(&environment, &mut bytes).unwrap();
    assert_eq!(&bytes[..4], b"R3FE");
    assert_eq!(decode_file_environment(&bytes).unwrap(), environment);
    let mut start = vec![0; 32 + lengths.len() * 4];
    encode_file_environment_start(&lengths, &mut start).unwrap();
    assert_eq!(decode_file_environment_start(&start).unwrap(), lengths);
    start[24] = 1;
    assert!(decode_file_environment_start(&start).is_err());
    let mut oversized = vec![0; 32 + 261 * 4];
    assert_eq!(
        encode_file_environment_start(&vec![3; 261], &mut oversized),
        Err(AbiError::Flags)
    );
    let mut bad = bytes.clone();
    bad[20..24].copy_from_slice(&524289u32.to_le_bytes());
    assert!(decode_file_environment(&bad).is_err());
    let mut nul = bytes.clone();
    let offset = 32 + lengths.len() * 4;
    nul[offset..offset + 2].fill(0);
    assert_eq!(decode_file_environment(&nul), Err(AbiError::Flags));
}
