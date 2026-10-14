use super::{
    AVX_ENABLED, AVX2_ENABLED, CPU_ID_LEAVES, F16C_ENABLED, FMA_ENABLED, NX_ENABLED,
    PREFETCHW_ENABLED, RDPID_ENABLED, XCR0,
};

fn leaf(function: u32, subfunction: Option<u32>) -> [u32; 4] {
    CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == function && leaf.subfunction == subfunction)
        .expect("declared cpuid leaf")
        .registers
}

#[test]
fn avx_capability_matches_enabled_components_and_standard_image_extent() {
    assert_eq!(AVX_ENABLED, leaf(1, None)[2] & (1 << 28) != 0);
    assert_eq!(XCR0, 7);
    assert_eq!(leaf(1, None)[2] & (7 << 26), 7 << 26);
    assert_eq!(leaf(13, Some(0)), [7, 832, 832, 0]);
    assert_eq!(leaf(13, Some(1)), [0; 4]);
    assert_eq!(leaf(13, Some(2)), [256, 576, 0, 0]);
}

#[test]
fn extensions_without_complete_execution_support_remain_unadvertised() {
    assert_eq!(leaf(1, None)[2] & (1 << 12) != 0, FMA_ENABLED);
    assert_eq!(leaf(1, None)[2] & (1 << 29) != 0, F16C_ENABLED);
    let structured = leaf(7, Some(0));
    // the structured feature leaf agrees with the canonical gates; avx512 stays absent.
    assert_eq!(structured, [0, 1 << 3 | 1 << 5 | 1 << 8, 1 << 22, 0]);
    assert_eq!(structured[1] & (1 << 5) != 0, AVX2_ENABLED);
    assert_eq!(AVX2_ENABLED, AVX_ENABLED);
    assert_eq!(structured[2] & (1 << 22) != 0, RDPID_ENABLED);
    assert_eq!(leaf(0, None)[0], 13);
}

#[test]
fn msr_instructions_are_present_under_fixed_user_privilege() {
    assert_eq!(leaf(1, None)[3] & (1 << 5), 1 << 5);
}

#[test]
fn execute_protection_is_advertised_and_reports_instruction_fetch_faults() {
    use crate::{
        cpu::x86::{PagingAccess32, VerifiedPageFault32},
        memory::GuestAddress,
    };

    assert_eq!(NX_ENABLED, leaf(0x8000_0001, None)[3] & (1 << 20) != 0);
    assert_eq!(leaf(0x8000_0001, None)[3] & (1 << 20), 1 << 20);
    for (present, expected) in [(false, 20), (true, 21)] {
        let fault = VerifiedPageFault32::application(
            GuestAddress(0x1000),
            PagingAccess32::Execute,
            present,
        );
        assert_eq!(fault.error_code(), expected);
    }
}

#[test]
fn ignored_data_prefetch_capability_is_explicit_without_changing_vendor_or_wt1() {
    assert_eq!(
        PREFETCHW_ENABLED,
        leaf(0x8000_0001, None)[2] & (1 << 8) != 0
    );
    assert_eq!(leaf(0x8000_0001, None), [0, 0, 0x120, 0x0810_0000]);
    assert_eq!(leaf(7, Some(0))[2] & 1, 0);
    assert_eq!(leaf(0, None), [13, 0x676e_6952, 0x2065_6e69, 0x676e_4533]);
}
