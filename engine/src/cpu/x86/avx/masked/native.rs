use super::*;
use crate::{
    cpu::x86::{SegmentCache, simd::SimdState},
    memory::{PageRange, Permissions},
};
fn r(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
}
fn page(at: u32) -> PageRange {
    PageRange::new(GuestAddress(at), 1).unwrap()
}
fn memory() -> AddressSpace {
    let mut m = AddressSpace::new(2).unwrap();
    m.map_zeroed(page(0x1000), Permissions::ALL).unwrap();
    m.write(
        GuestAddress(0x1000),
        &std::array::from_fn::<_, 4096, _>(|i| (i * 37 + 19) as u8),
    )
    .unwrap();
    m
}
fn state() -> VectorState {
    VectorState {
        simd: SimdState {
            mxcsr: 0xe5af,
            xmm: std::array::from_fn(|r| std::array::from_fn(|i| (r * 23 + i * 17 + 7) as u8)),
        },
        upper: std::array::from_fn(|r| std::array::from_fn(|i| (r * 29 + i * 11 + 43) as u8)),
    }
}
fn forms(d: u8, mask: u8, source: u8) -> Vec<MaskedInstruction> {
    let mut forms = vec![MaskedInstruction::StoreBytes {
        source: r(source),
        mask: r(mask),
    }];
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for element in [Element::Single, Element::Double] {
            forms.push(MaskedInstruction::Load {
                width,
                element,
                destination: r(d),
                mask: r(mask),
            });
            forms.push(MaskedInstruction::Store {
                width,
                element,
                source: r(source),
                mask: r(mask),
            });
        }
    }
    forms
}
fn set_mask(s: &mut VectorState, i: MaskedInstruction, bits: u32) {
    let mut mask = [0x7f; 32];
    for lane in 0..i.width().bytes() / i.lane_bytes() {
        mask[(lane + 1) * i.lane_bytes() - 1] = if bits & (1 << lane) != 0 { 0x80 } else { 0x7f };
    }
    let index = i.mask().index();
    s.simd.xmm[index].copy_from_slice(&mask[..16]);
    s.upper[index].copy_from_slice(&mask[16..]);
}
fn cache(kind: u32, limit: u32) -> SegmentCache {
    SegmentCache::from_bits(limit | (kind << 20) | (1 << 25) | (1 << 28) | (3 << 29) | (1 << 31))
        .unwrap()
}

#[test]
fn every_mask_uses_sign_bits_and_alias_sources_are_captured_before_publication() {
    for (d, mask, source) in [(0, 1, 2), (1, 1, 2), (7, 7, 7)] {
        for i in forms(d, mask, source) {
            let lanes = i.width().bytes() / i.lane_bytes();
            let masks: Vec<u32> = if lanes == 16 {
                (0..16)
                    .map(|b| 1 << b)
                    .chain([0, 0xffff, 0xaaaa, 0x5555])
                    .collect()
            } else {
                (0..1u32 << lanes).collect()
            };
            let mut m = memory();
            let mut original_ram = [0; 4096];
            m.read(GuestAddress(0x1000), &mut original_ram).unwrap();
            for bits in masks {
                m.write(GuestAddress(0x1000), &original_ram).unwrap();
                let mut s = state();
                set_mask(&mut s, i, bits);
                let before = s;
                let mut expected = s;
                let mut expected_ram = [0; 4096];
                m.read(GuestAddress(0x1000), &mut expected_ram).unwrap();
                let mask_bytes = before.read(i.mask());
                let data = match i {
                    MaskedInstruction::Store { source, .. }
                    | MaskedInstruction::StoreBytes { source, .. } => before.read(source),
                    _ => [0; 32],
                };
                let mut value = [0; 32];
                for lane in 0..lanes {
                    if mask_bytes[(lane + 1) * i.lane_bytes() - 1] & 0x80 != 0 {
                        let start = lane * i.lane_bytes();
                        for byte in start..start + i.lane_bytes() {
                            if matches!(i, MaskedInstruction::Load { .. }) {
                                value[byte] = expected_ram[3 + byte];
                            } else {
                                expected_ram[3 + byte] = data[byte];
                            }
                        }
                    }
                }
                if let MaskedInstruction::Load { destination, .. } = i {
                    expected.simd.xmm[destination.index()].copy_from_slice(&value[..16]);
                    expected.upper[destination.index()].copy_from_slice(&value[16..]);
                }
                s.execute_masked(
                    i,
                    &SegmentContext::default(),
                    SegmentRegister::Ds,
                    0x1003,
                    &mut m,
                )
                .unwrap();
                assert_eq!(s, expected, "{i:?}/mask{bits:x}");
                let mut actual = [0; 4096];
                m.read(GuestAddress(0x1000), &mut actual).unwrap();
                assert_eq!(actual, expected_ram);
            }
        }
    }
}
#[test]
fn disabled_lanes_do_not_extend_segment_or_page_authority() {
    for i in forms(0, 1, 2) {
        let mut s = state();
        set_mask(&mut s, i, 1);
        let mut m = memory();
        let mut context = SegmentContext::default();
        context.caches[SegmentRegister::Ds.index()] = cache(3, 0x1003 + i.lane_bytes() as u32 - 1);
        s.execute_masked(i, &context, SegmentRegister::Ds, 0x1003, &mut m)
            .unwrap();
        set_mask(&mut s, i, 2);
        let before = s;
        let mut ram = [0; 4096];
        m.read(GuestAddress(0x1000), &mut ram).unwrap();
        assert_eq!(
            s.execute_masked(i, &context, SegmentRegister::Ds, 0x1003, &mut m),
            Err(MaskedError::Segment {
                segment: SegmentRegister::Ds,
                cause: SegmentAccessError::Limit
            })
        );
        assert_eq!(s, before);
        let mut after = [0; 4096];
        m.read(GuestAddress(0x1000), &mut after).unwrap();
        assert_eq!(after, ram);
        let mut s = state();
        set_mask(&mut s, i, 1);
        s.execute_masked(
            i,
            &SegmentContext::default(),
            SegmentRegister::Ds,
            0x2000 - i.lane_bytes() as u32,
            &mut m,
        )
        .unwrap();
        set_mask(&mut s, i, 2);
        let before = s;
        let (access, error_code) = if i.access() == Access::Write {
            (crate::cpu::x86::PagingAccess32::Write, 6)
        } else {
            (crate::cpu::x86::PagingAccess32::Read, 4)
        };
        assert!(matches!(
            s.execute_masked(
                i,
                &SegmentContext::default(),
                SegmentRegister::Ds,
                0x2000 - i.lane_bytes() as u32,
                &mut m
            ),
            Err(MaskedError::Paging(CpuPagingError32::PageFault(fault)))
                if fault.linear() == GuestAddress(0x2000)
                    && fault.error_code() == error_code
                    && fault.access() == access
        ));
        assert_eq!(s, before);
    }
}
#[test]
fn zero_mask_suppresses_null_segment_and_selected_offset_overflow() {
    for i in forms(0, 1, 2) {
        let mut s = state();
        set_mask(&mut s, i, 0);
        let before = s;
        let mut m = memory();
        s.execute_masked(
            i,
            &SegmentContext::default(),
            SegmentRegister::Gs,
            u32::MAX,
            &mut m,
        )
        .unwrap();
        if let MaskedInstruction::Load { destination, .. } = i {
            let mut expected = before;
            expected.simd.xmm[destination.index()] = [0; 16];
            expected.upper[destination.index()] = [0; 16];
            assert_eq!(s, expected);
        } else {
            assert_eq!(s, before);
        }
        set_mask(&mut s, i, 1);
        let before = s;
        assert_eq!(
            s.execute_masked(
                i,
                &SegmentContext::default(),
                SegmentRegister::Gs,
                u32::MAX,
                &mut m
            ),
            Err(MaskedError::Segment {
                segment: SegmentRegister::Gs,
                cause: SegmentAccessError::Unusable
            })
        );
        assert_eq!(s, before);
    }
}
#[test]
fn selected_segment_failure_precedes_paging_and_stack_identity_is_retained() {
    for i in forms(0, 1, 2) {
        let mut s = state();
        set_mask(&mut s, i, 1);
        let before = s;
        let mut m = memory();
        let mut context = SegmentContext::default();
        context.caches[SegmentRegister::Ss.index()] = cache(3, 0xfff);
        assert_eq!(
            s.execute_masked(i, &context, SegmentRegister::Ss, 0x2000, &mut m),
            Err(MaskedError::Segment {
                segment: SegmentRegister::Ss,
                cause: SegmentAccessError::Limit
            })
        );
        assert_eq!(s, before);
        let mut context = SegmentContext::default();
        context.caches[SegmentRegister::Ds.index()] = cache(1, 0xffff);
        if i.access() == Access::Write {
            assert_eq!(
                s.execute_masked(i, &context, SegmentRegister::Ds, 0x2000, &mut m),
                Err(MaskedError::Segment {
                    segment: SegmentRegister::Ds,
                    cause: SegmentAccessError::Permission
                })
            );
            assert_eq!(s, before);
        }
    }
}
#[test]
fn fs_base_and_write_only_pages_use_selected_exact_access() {
    for i in forms(0, 1, 2) {
        let mut s = state();
        set_mask(&mut s, i, 1);
        let mut m = memory();
        let mut context = SegmentContext::default();
        assert!(context.state.set_base(SegmentRegister::Fs, 0x1000));
        if i.access() == Access::Write {
            m.protect(page(0x1000), Permissions::from_bits(2).unwrap())
                .unwrap();
        }
        s.execute_masked(i, &context, SegmentRegister::Fs, 3, &mut m)
            .unwrap();
        if i.access() == Access::Read {
            let mut wrapped = SegmentContext::default();
            assert!(wrapped.state.set_base(SegmentRegister::Fs, 0xfffff000));
            s.execute_masked(i, &wrapped, SegmentRegister::Fs, 0x2003, &mut m)
                .unwrap();
        }
    }
}
#[test]
fn invalid_host_state_refuses_before_memory_or_vector_publication() {
    for i in forms(0, 1, 2) {
        let mut s = state();
        set_mask(&mut s, i, 0);
        s.simd.mxcsr = 0x10000;
        let before = s;
        let mut m = memory();
        let snapshot = m.snapshot_code(GuestAddress(0x1000), 1).unwrap();
        assert_eq!(
            s.execute_masked(
                i,
                &SegmentContext::default(),
                SegmentRegister::Ds,
                u32::MAX,
                &mut m
            ),
            Err(MaskedError::InvalidState)
        );
        assert_eq!(s, before);
        assert!(m.is_code_current(&snapshot));
    }
}
