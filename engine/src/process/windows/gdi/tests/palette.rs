use super::{call, fixture, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::gdi::{GdiApi32, GdiError32},
};

fn named(
    runtime: &mut super::GdiRuntime32,
    ram: &mut AddressSpace,
    name: &str,
    args: &[u32],
) -> Result<u32, GdiError32> {
    let api = GdiApi32::ALL
        .iter()
        .copied()
        .find(|api| api.name() == name)
        .ok_or(GdiError32::Unsupported)?;
    call(runtime, ram, api, args)
}

fn indexed_info(ram: &mut AddressSpace, address: u32, bits: u16, colors: u32) {
    memory::write_words(
        ram,
        address,
        &[
            40,
            2,
            u32::MAX,
            1 | u32::from(bits) << 16,
            0,
            4,
            0,
            0,
            colors,
            0,
        ],
    )
    .unwrap();
}

#[test]
fn logical_palette_selection_resolves_dib_indexes_once_and_truecolor_realization_preserves_rgb_table()
 {
    // native windows run 38974156091 resolves [2,0,1] and retains that dib snapshot after realization.
    let (mut runtime, mut ram) = fixture();
    ram.write(
        GuestAddress(0x10000),
        &[0, 3, 3, 0, 255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0],
    )
    .unwrap();
    let palette = named(&mut runtime, &mut ram, "CreatePalette", &[0x10000]).unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    let stock = named(&mut runtime, &mut ram, "SelectPalette", &[dc, palette, 0]).unwrap();
    assert_eq!(
        stock,
        call(&mut runtime, &mut ram, GdiApi32::GetStockObject, &[15]).unwrap()
    );
    indexed_info(&mut ram, 0x10100, 8, 3);
    ram.write(GuestAddress(0x10128), &[2, 0, 0, 0, 1, 0])
        .unwrap();
    let bitmap = call(
        &mut runtime,
        &mut ram,
        GdiApi32::CreateDIBSection,
        &[dc, 0x10100, 1, 0x10180, 0, 0],
    )
    .unwrap();
    let previous = call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    for changed in [false, true] {
        if changed {
            ram.write(GuestAddress(0x10200), &[0x11, 0x22, 0x33, 0])
                .unwrap();
            assert_eq!(
                named(
                    &mut runtime,
                    &mut ram,
                    "SetPaletteEntries",
                    &[palette, 0, 1, 0x10200]
                ),
                Ok(1)
            );
            assert_eq!(
                named(&mut runtime, &mut ram, "RealizePalette", &[dc]),
                Ok(0)
            );
        }
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::GetDIBColorTable,
                &[dc, 0, 3, 0x10240]
            ),
            Ok(3)
        );
        assert_eq!(
            memory::words::<3>(&ram, 0x10240).unwrap(),
            [0xff, 0xff0000, 0xff00]
        );
    }
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, previous],
    )
    .unwrap();
    call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[bitmap]).unwrap();
    named(&mut runtime, &mut ram, "SelectPalette", &[dc, stock, 0]).unwrap();
    call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[palette]).unwrap();
    assert_eq!(runtime.model.object_count(), 0);
}

#[test]
fn dib_pal_colors_uses_default_logical_palette_and_rejects_invalid_usage_without_publication() {
    let (mut runtime, mut ram) = fixture();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    indexed_info(&mut ram, 0x10000, 8, 3);
    ram.write(GuestAddress(0x10028), &[19, 0, 0, 0, 1, 0])
        .unwrap();
    let bitmap = call(
        &mut runtime,
        &mut ram,
        GdiApi32::CreateDIBSection,
        &[dc, 0x10000, 1, 0x10080, 0, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetDIBColorTable,
            &[dc, 0, 3, 0x10100]
        ),
        Ok(3)
    );
    assert_eq!(
        memory::words::<3>(&ram, 0x10100).unwrap(),
        [0xffffff, 0, 0x800000]
    );
    let before = (
        runtime.model.object_count(),
        ram.mapped_pages(),
        ram.transaction_version(),
    );
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateDIBSection,
            &[dc, 0x10000, 7, 0x10080, 0, 0]
        ),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(
        (
            runtime.model.object_count(),
            ram.mapped_pages(),
            ram.transaction_version()
        ),
        before
    );
}

#[test]
fn getdibits_synthesizes_output_colors_instead_of_reading_guest_table_sentinels() {
    for (bits, table, packed) in [
        (1, vec![0, 0xffffff], vec![0x40, 0, 0, 0]),
        (
            4,
            vec![
                0, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0x808080, 0xc0c0c0,
                0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
            ],
            vec![0x0f, 0, 0, 0],
        ),
    ] {
        let (mut runtime, mut ram) = fixture();
        ram.write(GuestAddress(0x10000), &[0, 0, 0, 0, 255, 255, 255, 0])
            .unwrap();
        let bitmap = call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateBitmap,
            &[2, 1, 1, 32, 0x10000],
        )
        .unwrap();
        let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
        indexed_info(&mut ram, 0x10100, bits, 0);
        ram.write(GuestAddress(0x10128), &vec![0x5a; table.len() * 4])
            .unwrap();
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::GetDIBits,
                &[dc, bitmap, 0, 1, 0x10200, 0x10100, 0]
            ),
            Ok(1)
        );
        let mut output = vec![0; table.len() * 4];
        ram.read(GuestAddress(0x10128), &mut output).unwrap();
        let actual: Vec<_> = output
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(actual, table, "synthesized {bits}-bit rgbquad table");
        let mut pixels = [0; 4];
        ram.read(GuestAddress(0x10200), &mut pixels).unwrap();
        assert_eq!(pixels.as_slice(), packed);
    }
}

#[test]
fn native_synthesized_rgb_and_pal_tables_match_all_captured_depths_and_packed_rows() {
    for row in reference::ROWS {
        let (mut runtime, mut ram) = fixture();
        ram.write(GuestAddress(0x10000), &[0, 0, 0, 0, 255, 255, 255, 0])
            .unwrap();
        let bitmap = call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateBitmap,
            &[2, 1, 1, 32, 0x10000],
        )
        .unwrap();
        let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
        let bits = row.bits;
        let usage = row.usage;
        indexed_info(&mut ram, 0x10100, bits, 0);
        ram.write(GuestAddress(0x10128), &[0x5a; 1024]).unwrap();
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::GetDIBits,
                &[dc, bitmap, 0, 1, 0x10600, 0x10100, usage]
            ),
            Ok(1)
        );
        let header = memory::words::<10>(&ram, 0x10100).unwrap();
        assert_eq!(header[5], 4);
        assert_eq!(header[8], 0);
        let mut pixels = [0; 4];
        ram.read(GuestAddress(0x10600), &mut pixels).unwrap();
        assert_eq!(pixels, row.pixels, "depth {bits}, usage {usage}");
        let size = if usage == 1 { 2 } else { 4 };
        let mut bytes = vec![0; row.table.len() * size];
        ram.read(GuestAddress(0x10128), &mut bytes).unwrap();
        let actual: Vec<u32> = bytes
            .chunks_exact(size)
            .map(|part| {
                if size == 2 {
                    u16::from_le_bytes(part.try_into().unwrap()) as u32
                } else {
                    u32::from_le_bytes(part.try_into().unwrap())
                }
            })
            .collect();
        assert_eq!(actual, row.table, "depth {bits}, usage {usage}");
    }
}

#[test]
fn palette_mutation_output_faults_and_saved_selection_preserve_canonical_ownership() {
    let (mut runtime, mut ram) = fixture();
    ram.write(
        GuestAddress(0x10000),
        &[0, 3, 2, 0, 10, 20, 30, 1, 40, 50, 60, 2],
    )
    .unwrap();
    let palette = named(&mut runtime, &mut ram, "CreatePalette", &[0x10000]).unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    let stock = named(&mut runtime, &mut ram, "SelectPalette", &[dc, palette, 1]).unwrap();
    assert_eq!(call(&mut runtime, &mut ram, GdiApi32::SaveDC, &[dc]), Ok(1));
    named(&mut runtime, &mut ram, "SelectPalette", &[dc, stock, 0]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[palette]),
        Err(GdiError32::SelectedObject)
    );
    let version = ram.transaction_version();
    assert!(
        named(
            &mut runtime,
            &mut ram,
            "SetPaletteEntries",
            &[palette, 0, 2, 0x12000]
        )
        .is_err()
    );
    assert_eq!(ram.transaction_version(), version);
    named(
        &mut runtime,
        &mut ram,
        "GetPaletteEntries",
        &[palette, 0, 2, 0x10100],
    )
    .unwrap();
    assert_eq!(
        memory::words::<2>(&ram, 0x10100).unwrap(),
        [0x011e140a, 0x023c3228]
    );
    ram.write(GuestAddress(0x11ffc), &[0x5a; 4]).unwrap();
    let version = ram.transaction_version();
    assert!(
        named(
            &mut runtime,
            &mut ram,
            "GetPaletteEntries",
            &[palette, 0, 2, 0x11ffc]
        )
        .is_err()
    );
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(memory::words::<1>(&ram, 0x11ffc).unwrap(), [0x5a5a5a5a]);
    assert_eq!(
        named(&mut runtime, &mut ram, "ResizePalette", &[palette, 4]),
        Ok(1)
    );
    assert_eq!(
        named(
            &mut runtime,
            &mut ram,
            "GetPaletteEntries",
            &[palette, 0, 0, 0]
        ),
        Ok(4)
    );
    named(
        &mut runtime,
        &mut ram,
        "GetPaletteEntries",
        &[palette, 2, 2, 0x10100],
    )
    .unwrap();
    assert_eq!(memory::words::<2>(&ram, 0x10100).unwrap(), [0, 0]);
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetObjectW,
            &[palette, 2, 0x10100]
        ),
        Ok(2)
    );
    let mut count = [0; 2];
    ram.read(GuestAddress(0x10100), &mut count).unwrap();
    assert_eq!(u16::from_le_bytes(count), 4);
    call(&mut runtime, &mut ram, GdiApi32::RestoreDC, &[dc, u32::MAX]).unwrap();
    named(&mut runtime, &mut ram, "SelectPalette", &[dc, stock, 0]).unwrap();
    call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[palette]).unwrap();
    assert_eq!(
        named(
            &mut runtime,
            &mut ram,
            "GetPaletteEntries",
            &[palette, 0, 0, 0]
        ),
        Err(GdiError32::InvalidHandle)
    );
    assert_eq!(
        named(
            &mut runtime,
            &mut ram,
            "SetPaletteEntries",
            &[stock, 0, 1, 0x12000]
        ),
        Ok(0)
    );
}

#[test]
fn palette_storage_shares_graphics_budget_and_retirement_releases_only_owned_entries() {
    let (mut runtime, mut ram) = fixture();
    ram.map_zeroed(
        crate::memory::PageRange::new(GuestAddress(0x12000), 14).unwrap(),
        crate::memory::Permissions::READ_WRITE,
    )
    .unwrap();
    memory::write_words(&mut ram, 0x10000, &[0x3e80_0300]).unwrap();
    let palette = named(&mut runtime, &mut ram, "CreatePalette", &[0x10000]).unwrap();
    assert_eq!(
        named(
            &mut runtime,
            &mut ram,
            "GetPaletteEntries",
            &[palette, 0, 0, 0]
        ),
        Ok(16000)
    );
    let before = (runtime.model.object_count(), ram.transaction_version());
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateBitmap,
            &[512, 1, 1, 32, 0]
        ),
        Err(GdiError32::Capacity)
    );
    assert_eq!(
        (runtime.model.object_count(), ram.transaction_version()),
        before
    );
    call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[palette]).unwrap();
    assert!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateBitmap,
            &[512, 1, 1, 32, 0]
        )
        .is_ok()
    );
}

mod reference;
