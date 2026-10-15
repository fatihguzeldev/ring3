use super::*;

#[test]
fn each_family_preserves_every_unselected_state_family() {
    let source = state();
    let mut base = source;
    base.state.registers.fill(0);
    base.state.eip = 0x87654321;
    base.state.eflags = 0x246;
    base.x87 = X87State::default();
    base.simd = SimdState::default();
    base.debug = [0; 6];
    for (family, integer, control, floating, extended, debug) in [
        (CONTEXT_INTEGER32, true, false, false, false, false),
        (CONTEXT_CONTROL32, false, true, false, false, false),
        (CONTEXT_FLOATING32, false, false, true, false, false),
        (CONTEXT_EXTENDED32, false, false, true, true, false),
        (CONTEXT_DEBUG32, false, false, false, false, true),
    ] {
        let context = Win32Context32::capture(family, &source).unwrap();
        let actual = context.apply(&base, base.segments).unwrap();
        for index in [0, 1, 2, 3, 6, 7] {
            assert_eq!(
                actual.state.registers[index],
                if integer {
                    source.state.registers[index]
                } else {
                    base.state.registers[index]
                }
            );
        }
        for index in [4, 5] {
            assert_eq!(
                actual.state.registers[index],
                if control {
                    source.state.registers[index]
                } else {
                    base.state.registers[index]
                }
            );
        }
        assert_eq!(
            actual.state.eip,
            if control {
                source.state.eip
            } else {
                base.state.eip
            }
        );
        assert_eq!(actual.x87, if floating { source.x87 } else { base.x87 });
        assert_eq!(actual.simd, if extended { source.simd } else { base.simd });
        assert_eq!(actual.debug, if debug { source.debug } else { base.debug });
        assert_eq!(actual.segments, base.segments);
    }
}
#[test]
fn extended_state_wins_over_legacy_float_and_privileged_flags_stay_with_base() {
    let source = state();
    let context = Win32Context32::capture(CONTEXT_ALL32, &source).unwrap();
    let mut bytes = *context.bytes();
    write(&mut bytes, 28, 0x1234);
    write(&mut bytes, 192, u32::MAX);
    let decoded = Win32Context32::from_bytes(&bytes).unwrap();
    let applied = decoded.apply(&source, source.segments).unwrap();
    assert_eq!(applied.x87, source.x87);
    assert_eq!(applied.state.eflags, 0x00250fd7);
    assert_eq!(
        applied.state.eflags & (0x3000 | 0x4000 | 0x20000 | 0x180000),
        0
    );
}
#[test]
fn selected_segments_require_resolved_selector_authority_and_preserve_unselected_caches() {
    let source = state();
    let context = Win32Context32::capture(CONTEXT_SEGMENTS32, &source).unwrap();
    let mut bytes = *context.bytes();
    write(&mut bytes, 144, 0x63);
    let context = Win32Context32::from_bytes(&bytes).unwrap();
    assert_eq!(
        context.requested_selectors(&source),
        [0x23, 0x1b, 0x23, 0x23, 0x63, 0]
    );
    assert_eq!(
        context.apply(&source, source.segments),
        Err(ContextError32::InvalidSegment)
    );
    let mut resolved = source.segments;
    resolved.state.selectors[4] = 0x63;
    resolved.state.bases[4] = 0x12340000;
    resolved.state.bases[1] = 0x99990000;
    let applied = context.apply(&source, resolved).unwrap();
    assert_eq!(applied.segments.state.bases[4], 0x12340000);
    assert_eq!(
        applied.segments.state.bases[1],
        source.segments.state.bases[1]
    );
    assert_eq!(applied.state, source.state);
}
