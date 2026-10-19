use super::*;
use crate::memory::{PageRange, Permissions};

fn prepared() -> (AddressSpace, PreparedRegion) {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x90, 0xeb, 0xfe])
        .unwrap();
    memory.write(GuestAddress(0x1020), &[0x0f, 0x0b]).unwrap();
    let region = prepare_resident_region(
        &memory,
        &[
            BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 3,
            },
            BlockSpec {
                entry: GuestAddress(0x1020),
                byte_length: 2,
            },
        ],
        CompileLimits::default(),
        &[GateSpec {
            entry: GuestAddress(0x1020),
            id: 1,
        }],
        None,
        crate::cpu::x86::decode::DecodeAuthority::default(),
    )
    .unwrap();
    (memory, region)
}

#[test]
fn fresh_private_rw_preserves_ordinary_and_gate_memos_but_not_identity_or_real_source_changes() {
    for changed in [0x1000, 0x1020] {
        let (mut memory, region) = prepared();
        assert!(region.is_current(&memory));
        let source = memory.code_source_version();
        assert_eq!(region.validated_version.load(Ordering::Relaxed), source);
        let transaction = memory.transaction_version();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        assert_eq!(memory.transaction_version(), transaction + 1);
        assert_eq!(memory.code_validation_version(), transaction + 1);
        assert_eq!(memory.code_source_version(), source);
        assert!(region.is_current(&memory));
        assert_eq!(region.validated_version.load(Ordering::Relaxed), source);
        let authority = memory.code_validation_version();
        memory.write(GuestAddress(0x2000), &[0x55]).unwrap();
        assert_eq!(memory.code_validation_version(), authority);
        assert_eq!(memory.code_source_version(), source);
        assert!(region.is_current(&memory));
        let (foreign, _) = prepared();
        assert!(
            !region.is_current(&foreign),
            "matching source bytes never replace address-space identity"
        );
        memory.write(GuestAddress(changed), &[0x90]).unwrap();
        assert!(memory.code_source_version() > source);
        assert_eq!(
            memory.code_source_version(),
            memory.code_validation_version()
        );
        assert!(!region.is_current(&memory));
    }
    let (mut memory, region) = prepared();
    assert!(region.is_current(&memory));
    let source = memory.code_source_version();
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    assert!(memory.code_source_version() > source);
    assert!(!region.is_current(&memory));
}
