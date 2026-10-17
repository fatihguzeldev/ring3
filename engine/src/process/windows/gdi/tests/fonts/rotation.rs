use super::{call, fixture};
use super::super::memory;
use crate::{
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::windows::gdi::GdiRuntime32,
    windows::{gdi::GdiApi32 as Api, user32::Point32},
};

fn prepared(escapement: i32, orientation: i32) -> (GdiRuntime32, AddressSpace, u32, u32) {
    let (mut runtime, mut ram) = fixture();
    memory::write_words(
        &mut ram,
        0x10000,
        &[40, 64, (-64i32) as u32, 1 | 32 << 16, 0, 16_384, 0, 0, 0, 0],
    )
    .unwrap();
    let bitmap = call(
        &mut runtime,
        &mut ram,
        Api::CreateDIBSection,
        &[0, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let [pointer] = memory::words(&ram, 0x10080).unwrap();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    let font = call(
        &mut runtime,
        &mut ram,
        Api::CreateFontW,
        &[
            (-12i32) as u32,
            0,
            escapement as u32,
            orientation as u32,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            3,
            0,
            0,
        ],
    )
    .unwrap();
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, font]).unwrap();
    call(&mut runtime, &mut ram, Api::SetTextAlign, &[dc, 25]).unwrap();
    call(&mut runtime, &mut ram, Api::SetTextColor, &[dc, 0xffffff]).unwrap();
    call(&mut runtime, &mut ram, Api::SetBkMode, &[dc, 1]).unwrap();
    call(&mut runtime, &mut ram, Api::MoveToEx, &[dc, 32, 32, 0]).unwrap();
    ram.write(
        GuestAddress(0x10100),
        &"Wi"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    (runtime, ram, dc, pointer)
}
fn pixels(ram: &AddressSpace, pointer: u32) -> Vec<u8> {
    let mut bytes = vec![0; 16_384];
    ram.read(GuestAddress(pointer), &mut bytes).unwrap();
    bytes
}
fn rendered(escapement: i32, orientation: i32) -> (Vec<u8>, Point32, i32) {
    let (mut runtime, mut ram, dc, pointer) = prepared(escapement, orientation);
    call(
        &mut runtime,
        &mut ram,
        Api::GetTextExtentPoint32W,
        &[dc, 0x10100, 2, 0x10140],
    )
    .unwrap();
    let [advance, _]: [u32; 2] = memory::words(&ram, 0x10140).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::TextOutW,
            &[dc, 0, 0, 0x10100, 2]
        ),
        Ok(1)
    );
    let position = runtime.model.state(dc).unwrap().current_position;
    (pixels(&ram, pointer), position, advance as i32)
}

#[test]
fn compatible_escapement_rotates_real_glyph_cells_and_current_position_in_both_axes() {
    let (upright, position, advance) = rendered(0, 0);
    assert!(upright.chunks_exact(4).any(|pixel| pixel[..3] == [255; 3]));
    assert_eq!(
        position,
        Point32 {
            x: 32 + advance,
            y: 32
        }
    );
    for (angle, expected_position) in [
        (
            900,
            Point32 {
                x: 32,
                y: 32 - advance,
            },
        ),
        (
            -900,
            Point32 {
                x: 32,
                y: 32 + advance,
            },
        ),
        (
            1800,
            Point32 {
                x: 32 - advance,
                y: 32,
            },
        ),
    ] {
        let (actual, position, measured) = rendered(angle, angle);
        assert_eq!(measured, advance);
        assert_eq!(position, expected_position);
        let mut expected = vec![0; upright.len()];
        for y in 0..64usize {
            for x in 0..64usize {
                let (dx, dy) = match angle {
                    900 => (y, 63 - x),
                    -900 => (63 - y, x),
                    1800 => (63 - x, 63 - y),
                    _ => unreachable!(),
                };
                expected[(dy * 64 + dx) * 4..(dy * 64 + dx) * 4 + 4]
                    .copy_from_slice(&upright[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]);
            }
        }
        assert_eq!(actual, expected, "escapement {angle}");
    }
    assert_eq!(rendered(3600, 0), rendered(0, 0));
    assert_eq!(rendered(0, 900), rendered(0, 0));
    assert_eq!(rendered(900, -900), rendered(900, 0));
    let (diagonal, diagonal_position, _) = rendered(450, 450);
    assert!(diagonal.chunks_exact(4).any(|pixel| pixel[..3] != [0; 3]));
    assert_ne!(diagonal, upright);
    assert!(diagonal_position.x > 32 && diagonal_position.y < 32);
}

#[test]
fn rotated_text_keeps_explicit_opaque_and_clip_rectangles_in_dc_coordinates() {
    let (mut runtime, mut ram, dc, pointer) = prepared(900, 0);
    memory::write_words(&mut ram, 0x10180, &[2, 3, 7, 9]).unwrap();
    call(&mut runtime, &mut ram, Api::SetBkColor, &[dc, 0xff]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::ExtTextOutW,
        &[dc, 0, 0, 2, 0x10180, 0x10100, 2, 0],
    )
    .unwrap();
    let bytes = pixels(&ram, pointer);
    for y in 0..64usize {
        for x in 0..64usize {
            let pixel = &bytes[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
            if (2..7).contains(&x) && (3..9).contains(&y) {
                assert_eq!(pixel, &[0, 0, 255, 0]);
            } else {
                assert_ne!(pixel, &[0, 0, 255, 0]);
            }
        }
    }
    let (mut runtime, mut ram, dc, pointer) = prepared(900, 0);
    memory::write_words(&mut ram, 0x10180, &[24, 12, 40, 30]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::ExtTextOutW,
        &[dc, 0, 0, 4, 0x10180, 0x10100, 2, 0],
    )
    .unwrap();
    let bytes = pixels(&ram, pointer);
    assert!(bytes.iter().any(|&byte| byte != 0));
    for y in 0..64usize {
        for x in 0..64usize {
            if !(24..40).contains(&x) || !(12..30).contains(&y) {
                assert_eq!(&bytes[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4], &[0; 4]);
            }
        }
    }
}

#[test]
fn rotated_dib_write_fault_preserves_pixels_and_current_position_until_retry() {
    let (mut runtime, mut ram, dc, pointer) = prepared(450, 450);
    let range = PageRange::new(GuestAddress(pointer), 4).unwrap();
    ram.protect(range, Permissions::READ).unwrap();
    let before = pixels(&ram, pointer);
    let position = runtime.model.state(dc).unwrap().current_position;
    let version = ram.transaction_version();
    assert!(
        call(
            &mut runtime,
            &mut ram,
            Api::TextOutW,
            &[dc, 0, 0, 0x10100, 2]
        )
        .is_err()
    );
    assert_eq!(pixels(&ram, pointer), before);
    assert_eq!(runtime.model.state(dc).unwrap().current_position, position);
    assert_eq!(ram.transaction_version(), version);
    ram.protect(range, Permissions::READ_WRITE).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::TextOutW,
            &[dc, 0, 0, 0x10100, 2]
        ),
        Ok(1)
    );
    assert_ne!(pixels(&ram, pointer), before);
    assert_ne!(runtime.model.state(dc).unwrap().current_position, position);
}
