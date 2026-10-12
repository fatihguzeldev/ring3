use super::support_pe32 as pe;

use super::support_pe32_relocation as reloc;

use ring3_engine::{
    loader::{self, LoadError},
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
};

const ORIGINAL: &[u8] =
    include_bytes!("../../../process/images/relocation/fixtures/p2-pe32-relocatable/relocations.exe");
const ACTUAL: u32 = 0x0050_0000;
const GATE: u32 = 0x7000_0000;

fn linked_image() -> Vec<u8> {
    let mut bytes = reloc::full_image();
    bytes[pe::DATA_RAW + 0x100..pe::DATA_RAW + 0x240].fill(0);
    for (index, rva, size) in [(1, 0x3100, 40), (12, 0x3180, 8)] {
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, rva);
        pe::put32(&mut bytes, pe::OPTIONAL + 100 + index * 8, size);
    }
    for (index, value) in [0x3140, 0, 0, 0x31c0, 0x3180].into_iter().enumerate() {
        pe::put32(&mut bytes, pe::DATA_RAW + 0x100 + index * 4, value);
    }
    for offset in [0x140, 0x180] {
        pe::put32(&mut bytes, pe::DATA_RAW + offset, 0x3200);
    }
    bytes[pe::DATA_RAW + 0x1c0..pe::DATA_RAW + 0x1cd].copy_from_slice(b"KERNEL32.dll\0");
    bytes[pe::DATA_RAW + 0x202..pe::DATA_RAW + 0x214].copy_from_slice(b"GetCurrentProcess\0");
    bytes
}

#[test]
fn original_relocations_and_isolated_declarations_load_across_profiles() {
    let mut admissions = Vec::new();
    for actual in [0x0040_0000, 0x0050_0000, 0x0030_0000] {
        admissions.push((
            format!("unchanged original at {actual:#010x}"),
            loader::load_pe32_at(ORIGINAL, actual, 5).map(|_| ()),
        ));
    }

    for dynamic in [true, false] {
        let mut fixed = pe::image();
        let mut relocated = reloc::full_image();
        let mut linked = linked_image();
        for bytes in [&mut fixed, &mut relocated, &mut linked] {
            if dynamic {
                pe::put16(bytes, pe::OPTIONAL + 70, 0x0140);
            } else {
                pe::put32(bytes, pe::section(1) + 36, pe::RW | 0x0200_0000);
            }
        }
        let declaration = if dynamic {
            "dynamic-base-only"
        } else {
            "discardable-only"
        };
        admissions.push((
            format!("fixed {declaration}"),
            loader::load_pe32(&fixed, 4).map(|_| ()),
        ));
        admissions.push((
            format!("relocated {declaration}"),
            loader::load_pe32_at(&relocated, ACTUAL, 6).map(|_| ()),
        ));
        admissions.push((
            format!("linked-v8 {declaration}"),
            loader::load_pe32_linked_v8_at(&linked, ACTUAL, GATE, 7).map(|_| ()),
        ));
    }

    assert_eq!(admissions.len(), 9);
    for (label, result) in &admissions {
        eprintln!("{label}: {result:?}");
    }
    let failures: Vec<_> = admissions
        .into_iter()
        .filter(|(_, result)| result.is_err())
        .collect();
    assert!(failures.is_empty(), "admission failures: {failures:?}");
}

#[test]
fn unchanged_original_retains_five_pages_fixups_and_profile_boundaries_at_three_bases() {
    assert_eq!(ORIGINAL.len(), 6144);
    assert_eq!(&ORIGINAL[512..515], &[0x31, 0xc0, 0xc3]);
    assert_eq!(
        loader::load_pe32(ORIGINAL, 5).err(),
        Some(LoadError::Unsupported)
    );
    for base in [0x0040_0000_u32, 0x0050_0000, 0x0030_0000] {
        assert_eq!(
            loader::load_pe32_at(ORIGINAL, base, 4).err(),
            Some(LoadError::Capacity)
        );
        assert_eq!(
            loader::load_pe32_linked_v8_at(ORIGINAL, base, GATE, 6).err(),
            Some(LoadError::Unsupported)
        );
        let input = ORIGINAL.to_vec();
        let (memory, image) = loader::load_pe32_at(&input, base, 5).unwrap().into_parts();
        assert_eq!(
            (
                image.image_base,
                image.image_size,
                image.entry_point,
                image.mapped_pages
            ),
            (base, 0x5000, base + 0x1000, 5)
        );
        assert_eq!(memory.capacity_pages(), 5);
        assert_eq!(memory.mapped_pages(), 5);
        let mut expected = vec![0; 0x5000];
        expected[..512].copy_from_slice(&ORIGINAL[..512]);
        expected[0x1000..0x1200].copy_from_slice(&ORIGINAL[512..1024]);
        expected[0x2000..0x3200].copy_from_slice(&ORIGINAL[1024..5632]);
        expected[0x4000..0x4200].copy_from_slice(&ORIGINAL[5632..6144]);
        for rva in [0x2008, 0x200c, 0x3010] {
            expected[rva..rva + 4].copy_from_slice(&(base + 0x2000).to_le_bytes());
        }
        let mut actual = vec![0; 0x5000];
        memory.read(GuestAddress(base), &mut actual).unwrap();
        assert_eq!(actual, expected, "all five retained pages at {base:#010x}");
        assert_eq!(input, ORIGINAL);
        for (page, allowed) in [
            (0, [true, false, false]),
            (1, [true, false, true]),
            (2, [true, true, false]),
            (3, [true, true, false]),
            (4, [true, false, false]),
        ] {
            let address = GuestAddress(base + page * 4096);
            for (access, allowed) in [Access::Read, Access::Write, Access::Execute]
                .into_iter()
                .zip(allowed)
            {
                if allowed {
                    assert!(memory.resolve(address, access).is_ok());
                } else {
                    assert_eq!(
                        memory.resolve(address, access),
                        Err(MemoryError::Fault(MemoryFault {
                            address,
                            access,
                            reason: FaultReason::Permission,
                        }))
                    );
                }
            }
        }
        let address = GuestAddress(base + 0x5000);
        assert_eq!(
            memory.resolve(address, Access::Read),
            Err(MemoryError::Fault(MemoryFault {
                address,
                access: Access::Read,
                reason: FaultReason::Unmapped,
            }))
        );
    }
}
