use super::*;
use crate::{
    loader::SystemExportTarget32,
    windows::{CallingConvention32, text::CodePage32},
};

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn catalog_is_closed_and_private_dialog_procedures_are_not_named_exports() {
    let module = commdlg32_module(0x70000000);
    assert_eq!(module.exports.len(), 9);
    for (at, api) in ComdlgApi32::ALL.iter().enumerate() {
        assert_eq!(api.id(), 0x00210001 + at as u32);
        assert_eq!(ComdlgApi32::from_id(api.id()), Some(*api));
        assert_eq!(api.convention(), CallingConvention32::Stdcall);
        assert_eq!(module.exports[at].name.as_deref(), Some(api.name()));
    }
    assert_eq!(ComdlgApi32::from_id(0x00210008), None);
    for (export, id) in module.exports[7..].iter().zip([0x0021ff01, 0x0021ff02]) {
        assert_eq!(export.name, None);
        assert_eq!(export.target, SystemExportTarget32::PrivateGate(id));
    }
    assert_eq!(ComdlgApi32::GetOpenFileNameW.stack_words(), 1);
    assert_eq!(ComdlgApi32::GetFileTitleA.stack_words(), 3);
    assert_eq!(ComdlgApi32::CommDlgExtendedError.stack_words(), 0);
}

#[test]
fn desktop_layout_keeps_nt4_size_and_windows2000_tail_separate() {
    for size in [76usize, 88] {
        let mut bytes = vec![0; size];
        bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[28..32].copy_from_slice(&0x11223344u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&[3, 0, 9, 0]);
        bytes[68..72].copy_from_slice(&0x5678u32.to_le_bytes());
        if size == 88 {
            bytes[84..88].copy_from_slice(&1u32.to_le_bytes());
        }
        let input = OpenFileName32::parse(&bytes).unwrap();
        assert_eq!(
            (
                input.size,
                input.file,
                input.file_offset,
                input.extension_offset,
                input.hook
            ),
            (size as u32, 0x11223344, 3, 9, 0x5678)
        );
        assert_eq!(input.flags_ex, u32::from(size == 88));
    }
    let mut bytes = [0; 88];
    bytes[..4].copy_from_slice(&84u32.to_le_bytes());
    assert_eq!(
        OpenFileName32::parse(&bytes),
        Err(ComdlgError32::Dialog(DialogError32::StructSize))
    );
}

#[test]
fn filter_pairs_preserve_order_semicolon_patterns_and_custom_index_zero() {
    let entries =
        FileFilters32::parse(&units("Images\0*.bmp;*.ico\0All files\0*.*\0\0"), 8).unwrap();
    let filters = FileFilters32 {
        entries,
        custom: None,
    };
    assert_eq!(filters.entries[0].label, units("Images"));
    assert!(filters.selected(0).unwrap().matches(&units("picture.ICO")));
    assert!(!filters.selected(1).unwrap().matches(&units("game.exe")));
    assert!(filters.selected(2).unwrap().matches(&units("README")));
    assert!(FileFilters32::parse(&units("Missing pattern\0\0"), 8).is_err());
    assert!(FileFilters32::parse(&units("A\0*\0\0"), 0).is_err());
}

#[test]
fn ansi_file_offsets_count_encoded_bytes_and_explorer_multi_selection_keeps_nul_boundaries() {
    let cp = CodePage32::from_id(932).unwrap();
    let path = units("C:\\日本\\a.txt");
    let wide = FileSelection32::single(&path, true, cp, 128).unwrap();
    let ansi = FileSelection32::single(&path, false, cp, 128).unwrap();
    assert_eq!((wide.file_offset, wide.extension_offset), (6, 8));
    assert_eq!((ansi.file_offset, ansi.extension_offset), (8, 10));
    assert_eq!(
        cp.decode(&ansi.bytes[..ansi.bytes.len() - 1], true)
            .unwrap(),
        path
    );
    let multi = FileSelection32::multiple(
        &units("C:\\日本"),
        &[units("a.txt"), units("b.bmp")],
        false,
        cp,
        128,
    )
    .unwrap();
    assert_eq!((multi.file_offset, multi.extension_offset), (8, 0));
    assert_eq!(multi.bytes[multi.bytes.len() - 2..], [0, 0]);
    assert_eq!(multi.units as usize, multi.bytes.len());
    assert!(FileSelection32::single(&path, false, cp, 4).is_err());
}
