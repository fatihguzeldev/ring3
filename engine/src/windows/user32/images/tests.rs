use super::files::BitmapFile32;
use super::*;
use crate::windows::user32::User32Error32 as E;

fn icon_group(entries: &[(u8, u8, u16, u16)]) -> Vec<u8> {
    let mut bytes = vec![0, 0, 1, 0];
    bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for &(width, height, depth, id) in entries {
        bytes.extend_from_slice(&[width, height, 0, 0]);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&depth.to_le_bytes());
        bytes.extend_from_slice(&128u32.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes
}

#[test]
fn group_icon_selection_preserves_first_size_and_prefers_nearest_larger_tie_then_depth() {
    let bytes = icon_group(&[
        (16, 16, 4, 11),
        (32, 32, 8, 12),
        (32, 32, 32, 13),
        (48, 48, 32, 14),
    ]);
    let group = ImageDirectory32::resource(&bytes, ImageKind32::Icon).unwrap();
    assert_eq!(
        group.select(None, 32).unwrap().source,
        ImageSource32::Resource(11)
    );
    assert_eq!(
        group
            .select(
                Some(ImageExtent32 {
                    width: 40,
                    height: 40
                }),
                32
            )
            .unwrap()
            .source,
        ImageSource32::Resource(14)
    );
    assert_eq!(
        group
            .select(
                Some(ImageExtent32 {
                    width: 32,
                    height: 32
                }),
                16
            )
            .unwrap()
            .source,
        ImageSource32::Resource(12)
    );
    assert_eq!(
        group
            .select(
                Some(ImageExtent32 {
                    width: 8,
                    height: 8
                }),
                1
            )
            .unwrap()
            .source,
        ImageSource32::Resource(11)
    );
}

#[test]
fn group_cursor_uses_word_dimensions_and_resource_id_without_treating_them_as_hotspots() {
    let bytes = [0, 0, 2, 0, 1, 0, 0, 1, 32, 0, 1, 0, 1, 0, 52, 0, 0, 0, 9, 0];
    let group = ImageDirectory32::resource(&bytes, ImageKind32::Cursor).unwrap();
    let image = group.select(None, 32).unwrap();
    assert_eq!(
        image.extent,
        ImageExtent32 {
            width: 256,
            height: 32
        }
    );
    assert_eq!(image.source, ImageSource32::Resource(9));
    assert_eq!(image.depth, 1);
    assert!(ImageDirectory32::resource(&bytes, ImageKind32::Icon).is_err());
}

#[test]
fn file_directory_keeps_cursor_hotspot_and_checks_selected_payload_bounds() {
    let mut bytes = vec![0, 0, 2, 0, 1, 0, 2, 2, 0, 0, 1, 0, 0, 0];
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&22u32.to_le_bytes());
    bytes.resize(62, 0);
    bytes[22..26].copy_from_slice(&40u32.to_le_bytes());
    bytes[34..36].copy_from_slice(&1u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&32u16.to_le_bytes());
    let directory = ImageDirectory32::file(&bytes, ImageKind32::Cursor).unwrap();
    let image = directory.select(None, 32).unwrap();
    assert_eq!(
        image.source,
        ImageSource32::File {
            offset: 22,
            length: 40,
            hotspot: Some((1, 0))
        }
    );
    assert_eq!(image.depth, 32);
    bytes[14..18].copy_from_slice(&41u32.to_le_bytes());
    assert!(ImageDirectory32::file(&bytes, ImageKind32::Cursor).is_err());
}

#[test]
fn malformed_group_headers_and_partial_entries_are_rejected_without_selection() {
    for bytes in [
        &[][..],
        &[0, 0, 1, 0, 0, 0],
        &[0, 0, 1, 0, 1, 0],
        &[1, 0, 1, 0, 1, 0],
    ] {
        assert!(ImageDirectory32::resource(bytes, ImageKind32::Icon).is_err());
    }
    let mut bytes = icon_group(&[(16, 16, 32, 1)]);
    bytes[9] = 1;
    assert!(ImageDirectory32::resource(&bytes, ImageKind32::Icon).is_err());
}

#[test]
fn load_and_copy_resolve_zero_dimensions_without_conflating_shared_or_delete_flags() {
    let native = ImageExtent32 {
        width: 19,
        height: 23,
    };
    let defaults = ImageExtent32 {
        width: 32,
        height: 32,
    };
    let load = LoadImage32::new(ImageKind32::Cursor, 0, 0, 0).unwrap();
    assert_eq!(load.extent(native, defaults), native);
    assert_eq!(
        LoadImage32::new(ImageKind32::Cursor, 0, 0, 0x40)
            .unwrap()
            .extent(native, defaults),
        defaults
    );
    let copy = CopyImage32::new(ImageKind32::Icon, 0, 0, 0xc).unwrap();
    assert!(copy.return_original());
    assert!(copy.delete_original());
    assert_eq!(copy.extent(native, defaults), native);
    assert_eq!(
        LoadImage32::new(ImageKind32::Bitmap, -1, 0, 0),
        Err(E::Win32(87))
    );
    assert_eq!(
        CopyImage32::new(ImageKind32::Icon, 16, 16, 0x2000),
        Err(E::Win32(87))
    );
}

#[test]
fn bitmap_file_preserves_explicit_pixel_offset_and_cursor_resource_preserves_hotspot() {
    let mut bytes = vec![0; 14 + 40 + 16 + 8];
    bytes[..2].copy_from_slice(b"BM");
    bytes[10..14].copy_from_slice(&70u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[70..78].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let file = BitmapFile32::parse(&bytes).unwrap();
    assert_eq!(file.dib.as_ptr(), bytes[14..].as_ptr());
    assert_eq!(file.pixels, &[1, 2, 3, 4, 5, 6, 7, 8]);
    bytes[10..14].copy_from_slice(&53u32.to_le_bytes());
    assert!(BitmapFile32::parse(&bytes).is_err());
    let (hotspot, dib) = cursor_resource(&[3, 0, 4, 0, 40, 0, 0, 0]).unwrap();
    assert_eq!(hotspot, (3, 4));
    assert_eq!(dib, &[40, 0, 0, 0]);
}

#[test]
fn authored_system_images_have_real_color_and_transparency_planes_and_cursor_hotspots() {
    let arrow = StockImage32::load(ImageKind32::Cursor, 32512, &Default::default()).unwrap();
    assert_eq!(
        arrow.extent,
        ImageExtent32 {
            width: 32,
            height: 32
        }
    );
    assert_eq!(arrow.hotspot, (0, 0));
    assert_eq!(arrow.info.len(), 40 + 32 * 32 * 4 + 32 * 4);
    assert_eq!(&arrow.info[8..12], &64u32.to_le_bytes());
    let mask = &arrow.info[40 + 32 * 32 * 4..];
    let transparent = |x: usize, y: usize| mask[(31 - y) * 4 + x / 8] & (0x80 >> (x % 8)) != 0;
    assert!(transparent(0, 0));
    assert!(!transparent(2, 2));
    assert!(transparent(31, 31));
    let wait = StockImage32::load(ImageKind32::Cursor, 32514, &Default::default()).unwrap();
    assert_eq!(wait.hotspot, (16, 16));
    assert_ne!(arrow.info, wait.info);
    let warning = StockImage32::load(ImageKind32::Icon, 32515, &Default::default()).unwrap();
    assert!(
        warning.info[40..40 + 4096]
            .chunks_exact(4)
            .any(|pixel| pixel == [0, 255, 255, 0])
    );
    assert!(StockImage32::load(ImageKind32::Cursor, 0, &Default::default()).is_err());
    let checks = StockImage32::load(ImageKind32::Bitmap, 32759, &Default::default()).unwrap();
    assert_eq!(
        checks.extent,
        ImageExtent32 {
            width: 52,
            height: 39
        }
    );
    assert_eq!(checks.info.len(), 40 + 52 * 39 * 4);
    let ibeam = StockImage32::load(ImageKind32::Cursor, 32513, &Default::default()).unwrap();
    assert_eq!(ibeam.hotspot, (8, 9));
    assert_eq!(
        u16::from_le_bytes(ibeam.info[14..16].try_into().unwrap()),
        1
    );
}

fn cursor_group(entries: &[(u16, u16, u16, u16)]) -> Vec<u8> {
    let mut bytes = vec![0, 0, 2, 0];
    bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for &(width, height, depth, id) in entries {
        for word in [width, height, 1, depth] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&128u32.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes
}

#[test]
fn native_cursor_selection_prefers_depth_then_nearest_width_and_ignores_group_height() {
    let bytes = cursor_group(&[(16, 32, 1, 101), (32, 32, 32, 102), (32, 64, 32, 103)]);
    let group = ImageDirectory32::resource(&bytes, ImageKind32::Cursor).unwrap();
    for desired in [None, Some(16), Some(24), Some(32), Some(48), Some(64)] {
        let extent = desired.map(|size| ImageExtent32 {
            width: size,
            height: size,
        });
        for (depth, id) in [(32, 102), (1, 101)] {
            assert_eq!(
                group.select(extent, depth).unwrap().source,
                ImageSource32::Resource(id)
            );
        }
    }
    for entries in [
        [(16, 32, 32, 201), (48, 96, 32, 202), (32, 64, 32, 203)],
        [(48, 96, 32, 202), (16, 32, 32, 201), (32, 64, 32, 203)],
    ] {
        let bytes = cursor_group(&entries);
        let group = ImageDirectory32::resource(&bytes, ImageKind32::Cursor).unwrap();
        assert_eq!(
            group.select(None, 32).unwrap().source,
            ImageSource32::Resource(entries[0].3)
        );
        for (size, id) in [(16, 201), (24, 203), (32, 203), (48, 202), (64, 202)] {
            for depth in [1, 32] {
                assert_eq!(
                    group
                        .select(
                            Some(ImageExtent32 {
                                width: size,
                                height: size
                            }),
                            depth
                        )
                        .unwrap()
                        .source,
                    ImageSource32::Resource(id)
                );
            }
        }
    }
}
