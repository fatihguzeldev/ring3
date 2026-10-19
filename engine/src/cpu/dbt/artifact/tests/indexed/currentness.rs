use super::*;

#[test]
fn indexed_geometry_never_authorizes_changed_source_identity_or_decode_key() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0x90; 2]).unwrap();
    let region = compile_region(&memory, &[spec(0x1000, 2)], CompileLimits::default()).unwrap();
    assert!(region.supports_indexed_entry());
    assert!(region.wasm_bytes(&memory).is_ok());
    assert!(region.matches_decode_key(CodeDecodeKey::FLAT));
    assert!(!region.matches_decode_key(CodeDecodeKey {
        cs_base: 0x1000,
        default32: true,
    }));
    let mut other = super::memory();
    other.write(GuestAddress(0x1000), &[0x90; 2]).unwrap();
    assert_eq!(
        region.wasm_bytes(&other),
        Err(ArtifactError::CodeInvalidated)
    );
    memory.write(GuestAddress(0x1000), &[0xcc]).unwrap();
    assert!(
        region.supports_indexed_entry(),
        "the flag describes immutable IR geometry only"
    );
    assert_eq!(
        region.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
}
