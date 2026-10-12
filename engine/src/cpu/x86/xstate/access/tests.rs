use super::{MemorySpan, RESTORE_SPANS, SAVE_SPANS};
use crate::cpu::x86::{SegmentAccess, SegmentCache};

fn selected(spans: &[MemorySpan], request: u32) -> Vec<(u32, u32)> {
    spans
        .iter()
        .filter(|span| span.selection == 0 || span.selection & request != 0)
        .map(|span| (span.offset, span.width))
        .collect()
}

#[test]
fn standard_spans_preserve_component_holes_and_unconditional_header() {
    assert_eq!(selected(&SAVE_SPANS, 0), [(512, 8)]);
    assert_eq!(selected(&RESTORE_SPANS, 0), [(512, 24)]);
    assert_eq!(selected(&SAVE_SPANS, 1), [(512, 8), (0, 24), (32, 128)]);
    assert_eq!(selected(&RESTORE_SPANS, 1), [(512, 24), (0, 24), (32, 128)]);
    assert_eq!(selected(&SAVE_SPANS, 2), [(512, 8), (24, 8), (160, 128)]);
    assert_eq!(
        selected(&RESTORE_SPANS, 2),
        [(512, 24), (24, 4), (160, 128)]
    );
    for spans in [&SAVE_SPANS, &RESTORE_SPANS] {
        assert_eq!(selected(spans, 0xffff_fffc), selected(spans, 0));
        assert_eq!(selected(spans, u32::MAX), selected(spans, 3));
        for span in spans {
            assert!(span.width != 0);
        }
    }
}

fn authorized(
    spans: &[MemorySpan],
    request: u32,
    base: u32,
    bits: u32,
    access: SegmentAccess,
) -> bool {
    let cache = SegmentCache::from_bits(bits).unwrap();
    selected(spans, request).into_iter().all(|(offset, width)| {
        base.checked_add(offset)
            .is_some_and(|start| cache.check_span(start, width, access).is_ok())
    })
}

#[test]
fn empty_request_checks_header_without_requiring_an_expand_down_payload() {
    // the operand base is below the lower bound; its selected header is above it.
    for (spans, access, end) in [
        (&SAVE_SPANS, SegmentAccess::Write, 0x5207),
        (&RESTORE_SPANS, SegmentAccess::Read, 0x5217),
    ] {
        assert!(authorized(spans, 0, 0x5000, 0xf270_51ff, access));
        assert!(!authorized(spans, 1, 0x5000, 0xf270_51ff, access));
        assert!(!authorized(spans, 2, 0x5000, 0xf270_51ff, access));
        assert!(authorized(spans, 3, 0x5000, 0xf230_0000 | end, access));
        assert!(!authorized(
            spans,
            3,
            0x5000,
            0xf230_0000 | (end - 1),
            access
        ));
    }
}

#[test]
fn field_arithmetic_does_not_wrap_after_address_size_truncation() {
    for (spans, access) in [
        (&SAVE_SPANS, SegmentAccess::Write),
        (&RESTORE_SPANS, SegmentAccess::Read),
    ] {
        assert!(!authorized(spans, 0, 0xffff_fe00, 0xf33f_ffff, access));
        assert!(authorized(spans, 3, 0xffc0, 0xf230_ffff + 0x200, access));
        assert!(!authorized(spans, 0, 0xffc0, 0xf230_ffff, access));
    }
}

#[test]
fn save_write_authority_is_stronger_than_its_header_read() {
    assert!(!authorized(
        &SAVE_SPANS,
        0,
        0x5000,
        0xf31f_ffff,
        SegmentAccess::Write
    ));
    assert!(!authorized(
        &SAVE_SPANS,
        3,
        0x5000,
        0xf3bf_ffff,
        SegmentAccess::Write
    ));
    assert!(!authorized(
        &RESTORE_SPANS,
        0,
        0x5000,
        0xf39f_ffff,
        SegmentAccess::Read
    ));
    assert!(authorized(
        &RESTORE_SPANS,
        3,
        0x5000,
        0xf3bf_ffff,
        SegmentAccess::Read
    ));
}
