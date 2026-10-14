use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

#[test]
fn source_free_and_consumed_prefix_resident_denials_can_be_repaired_without_dropping_identity() {
    for consumed in [false, true] {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        let pc = if consumed { 0x1fff } else { 0x1000 };
        memory
            .write(
                GuestAddress(pc),
                if consumed {
                    &[0xb8][..]
                } else {
                    &[0xeb, 0xfe][..]
                },
            )
            .unwrap();
        if !consumed {
            memory
                .protect(
                    PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                    Permissions::READ_WRITE,
                )
                .unwrap();
        }
        let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
        let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
        let denied = registry
            .compile_runtime_bound(
                &memory,
                GuestAddress(pc),
                CompileLimits::default(),
                7,
                cs,
                DecodeAuthority::default(),
            )
            .unwrap();
        assert_eq!(
            registry.lookup_bound(&memory, GuestAddress(pc), cs),
            Ok(denied)
        );
        if consumed {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory
                .write(GuestAddress(0x2000), &[0, 0, 0, 0, 0xeb, 0xfe])
                .unwrap();
        }
        if !consumed {
            memory
                .protect(
                    PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        assert_eq!(
            registry.lookup_bound(&memory, GuestAddress(pc), cs),
            Err(RegistryError::NotFound {
                pc: GuestAddress(pc)
            })
        );
        // a missing-byte repair does not alter a snapshot of already-consumed source.
        assert!(registry.get_raw(&memory, denied.get()).is_ok());
        let repaired = registry
            .compile_runtime_bound(
                &memory,
                GuestAddress(pc),
                CompileLimits::default(),
                7,
                cs,
                DecodeAuthority::default(),
            )
            .unwrap();
        assert_ne!(repaired, denied);
        assert_eq!(
            registry.lookup_bound(&memory, GuestAddress(pc), cs),
            Ok(repaired)
        );
        memory.write(GuestAddress(pc), &[0x90]).unwrap();
        assert_eq!(
            registry.lookup_bound(&memory, GuestAddress(pc), cs),
            Err(RegistryError::CodeInvalidated)
        );
    }
}
