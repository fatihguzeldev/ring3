use super::*;
use crate::{
    cpu::x86::simd::SimdState,
    memory::{PageRange, Permissions},
};

#[test]
fn default_kernel_observes_application_permissions_but_zero_mask_has_no_access() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::NONE,
        )
        .unwrap();
    let instruction = MaskedInstruction::Load {
        width: VectorWidth::Xmm,
        element: Element::Single,
        destination: VectorRegister::new(0).unwrap(),
        mask: VectorRegister::new(1).unwrap(),
    };
    let mut vectors = VectorState {
        simd: SimdState::default(),
        upper: [[0x55; 16]; 8],
    };
    vectors.simd.xmm[0] = [0xa5; 16];
    vectors.simd.xmm[1][3] = 0x80;
    let before = vectors;
    assert!(matches!(
        vectors.execute_masked(
            instruction,
            &SegmentContext::default(),
            SegmentRegister::Ds,
            0x1000,
            &mut memory,
        ),
        Err(MaskedError::Paging(CpuPagingError32::PageFault(fault))) if fault.linear() == GuestAddress(0x1000) && fault.error_code() == 5
    ));
    assert_eq!(vectors, before);
    vectors.simd.xmm[1] = [0; 16];
    vectors.upper[1] = [0; 16];
    vectors
        .execute_masked(
            instruction,
            &SegmentContext::default(),
            SegmentRegister::Ss,
            u32::MAX,
            &mut memory,
        )
        .unwrap();
    assert_eq!(vectors.read(VectorRegister::new(0).unwrap()), [0; 32]);
}
