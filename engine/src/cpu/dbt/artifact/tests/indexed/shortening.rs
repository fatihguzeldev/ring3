use super::*;

#[test]
fn indexed_eligibility_is_recomputed_from_the_final_byte_limited_prefix() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0x90; 2]).unwrap();
    let minimum = compile_region(&memory, &[spec(0x1000, 1)], CompileLimits::default()).unwrap();
    let full = compile_region(&memory, &[spec(0x1000, 2)], CompileLimits::default()).unwrap();
    let budget = minimum.wasm_bytes(&memory).unwrap().len();
    assert!(full.wasm_bytes(&memory).unwrap().len() > budget);
    assert!(full.supports_indexed_entry());
    assert!(!minimum.supports_indexed_entry());
    let prepared = prepare_region(&memory, &[spec(0x1000, 2)], CompileLimits::default()).unwrap();
    let shortened = emit_entry_prepared(
        &memory,
        prepared,
        CompileLimits {
            wasm_bytes: budget,
            ..CompileLimits::default()
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(shortened.metadata().instructions, 1);
    assert!(shortened.wasm_bytes(&memory).unwrap().len() <= budget);
    assert!(!shortened.supports_indexed_entry());
    assert!(!shortened.contains_instruction(0x1001));
}
