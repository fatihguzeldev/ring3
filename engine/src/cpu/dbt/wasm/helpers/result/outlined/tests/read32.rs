use super::*;

#[test]
fn generated_x87_read32_matches_inline_faults_scratch_and_live_context() {
    let input = format!(
        "{{\"module\":\"{}\",\"segments\":{},\"bases\":{}}}",
        hex(&differential_module(true)),
        SEGMENTS_OFFSET,
        SEGMENT_BASES_RECORD_OFFSET,
    );
    let output = node(include_str!("../fixtures/runtime/read32.mjs"), &input);
    assert!(output.starts_with("x87 read32 inline/outlined differential controls passed ("));
    print!("{output}");
}

#[test]
fn read32_outline_requires_the_x87_ordinary_read_contract() {
    for (length, access, smc, paging, intent, has_helper) in [
        (0, 0, false, true, None, true),
        (2, 1, false, true, None, true),
        (8, 1, false, true, None, true),
        (10, 1, false, true, None, true),
        (4, 2, false, true, None, true),
        (4, 3, false, true, None, true),
        (4, 1, true, true, None, true),
        (4, 1, false, false, None, true),
        (4, 1, false, true, Some(1), true),
        (4, 1, false, true, Some(2), true),
        (4, 1, false, true, None, false),
    ] {
        let context = ResultContext {
            paging_configured: paging,
            paging_access_mask: intent,
            zero_span_validator: None,
            x87_read32_validator: has_helper.then_some(42),
        };
        let mut actual = Function::new([(36, ValType::I32)]);
        let mut retained = Function::new([(36, ValType::I32)]);
        validate_x87(&mut actual.instructions(), length, access, smc, 0, context);
        validate(
            &mut retained.instructions(),
            length,
            access,
            smc,
            0,
            context,
        );
        let mut before = Vec::new();
        let mut after = Vec::new();
        retained.encode(&mut before);
        actual.encode(&mut after);
        assert_eq!(
            after, before,
            "nonmatching x87 contract stays byte-identical"
        );
    }
    let context = ResultContext {
        paging_configured: true,
        paging_access_mask: None,
        zero_span_validator: None,
        x87_read32_validator: Some(42),
    };
    let mut ordinary = Function::new([(36, ValType::I32)]);
    let mut retained = Function::new([(36, ValType::I32)]);
    validate(&mut ordinary.instructions(), 4, 1, false, 0, context);
    validate_span(
        &mut retained.instructions(),
        4,
        1,
        false,
        Validation::Ordinary { mask: None },
        0,
        context,
    );
    let mut before = Vec::new();
    let mut after = Vec::new();
    retained.encode(&mut before);
    ordinary.encode(&mut after);
    assert_eq!(
        after, before,
        "non-x87 ordinary reads keep their original validator"
    );
}
