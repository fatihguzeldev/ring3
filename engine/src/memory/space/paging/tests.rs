use super::*;
use crate::memory::{FaultReason, MemoryFault};

const USER_RW: PagingCertificate32 = PagingCertificate32::Present {
    user: true,
    writable: true,
};
fn range(page: u32) -> PageRange {
    PageRange::new(GuestAddress(page), 1).unwrap()
}
fn fixture() -> AddressSpace {
    let mut memory = AddressSpace::new(4).unwrap();
    memory.enable_paging_certificates32().unwrap();
    memory
        .map_zeroed(range(0), crate::memory::Permissions::ALL)
        .unwrap();
    memory
        .map_zeroed(range(0x1000), crate::memory::Permissions::ALL)
        .unwrap();
    memory
        .map_zeroed(range(0xfffff000), crate::memory::Permissions::ALL)
        .unwrap();
    memory
}
fn check(
    memory: &AddressSpace,
    at: u32,
    length: usize,
    access: PagingAccess32,
    domain: PagingDomain32,
    wp: Option<bool>,
) -> Result<(), PagingCheckError32> {
    let backing = match access {
        PagingAccess32::Read => PagingBacking32::Read,
        PagingAccess32::Write => PagingBacking32::Write,
        PagingAccess32::Execute => PagingBacking32::Execute,
    };
    memory.cpu_paging_check32(GuestAddress(at), length, access, domain, wp, backing)
}
fn violation(result: Result<(), PagingCheckError32>) -> PagingFaultFacts32 {
    let Err(PagingCheckError32::Fault(facts)) = result else {
        panic!("expected certified paging facts, got {result:?}")
    };
    facts
}

#[test]
fn unknown_stops_and_all_four_present_rights_are_independent_of_software_rwx() {
    let mut memory = fixture();
    assert_eq!(
        check(
            &memory,
            0x1001,
            4,
            PagingAccess32::Read,
            PagingDomain32::User,
            None
        ),
        Err(PagingCheckError32::Uncertified(GuestAddress(0x1001)))
    );
    for (user, writable, access, expected) in [
        (
            false,
            false,
            PagingAccess32::Read,
            Some(LegacyPageViolation32::SupervisorPage),
        ),
        (
            false,
            true,
            PagingAccess32::Write,
            Some(LegacyPageViolation32::SupervisorPage),
        ),
        (true, false, PagingAccess32::Read, None),
        (
            true,
            false,
            PagingAccess32::Write,
            Some(LegacyPageViolation32::ReadOnlyPage),
        ),
        (true, false, PagingAccess32::Execute, None),
        (true, true, PagingAccess32::Write, None),
    ] {
        memory
            .certify_paging_range32(
                range(0x1000),
                PagingCertificate32::Present { user, writable },
            )
            .unwrap();
        let result = check(&memory, 0x1001, 4, access, PagingDomain32::User, None);
        if let Some(expected) = expected {
            let facts = violation(result);
            assert_eq!(
                (
                    facts.identity(),
                    facts.linear(),
                    facts.access(),
                    facts.domain(),
                    facts.violation()
                ),
                (
                    memory.identity(),
                    GuestAddress(0x1001),
                    access,
                    PagingDomain32::User,
                    expected
                )
            );
        } else {
            assert_eq!(result, Ok(()));
        }
    }
    // a mapped software-readable page can still be certified architecturally not present.
    memory
        .certify_paging_range32(range(0x1000), PagingCertificate32::NotPresent)
        .unwrap();
    let before = memory.backing.to_vec();
    assert_eq!(
        violation(check(
            &memory,
            0x1001,
            4,
            PagingAccess32::Read,
            PagingDomain32::User,
            None
        ))
        .violation(),
        LegacyPageViolation32::NotPresent
    );
    assert_eq!(memory.backing.as_ref(), before);
    let mut diagnostic = [0; 4];
    memory.read(GuestAddress(0x1001), &mut diagnostic).unwrap();
}

#[test]
fn supervisor_rights_use_explicit_wp_and_rmw_keeps_architectural_write_intent() {
    let mut memory = fixture();
    memory
        .certify_paging_range32(
            range(0x1000),
            PagingCertificate32::Present {
                user: true,
                writable: false,
            },
        )
        .unwrap();
    for wp in [None, Some(false), Some(true)] {
        assert_eq!(
            check(
                &memory,
                0x1000,
                4,
                PagingAccess32::Read,
                PagingDomain32::Supervisor,
                wp
            ),
            Ok(())
        );
        let write = check(
            &memory,
            0x1000,
            4,
            PagingAccess32::Write,
            PagingDomain32::Supervisor,
            wp,
        );
        match wp {
            None => assert_eq!(
                write,
                Err(PagingCheckError32::UnknownWriteProtect(GuestAddress(
                    0x1000
                )))
            ),
            Some(false) => assert_eq!(write, Ok(())),
            Some(true) => assert_eq!(
                violation(write).violation(),
                LegacyPageViolation32::ReadOnlyPage
            ),
        }
    }
    let facts = violation(memory.cpu_paging_check32(
        GuestAddress(0x1000),
        4,
        PagingAccess32::Write,
        PagingDomain32::User,
        None,
        PagingBacking32::Read,
    ));
    assert_eq!(facts.access(), PagingAccess32::Write);
    memory
        .certify_paging_range32(range(0x1000), PagingCertificate32::NotPresent)
        .unwrap();
    assert_eq!(
        violation(memory.cpu_paging_check32(
            GuestAddress(0x1000),
            4,
            PagingAccess32::Write,
            PagingDomain32::User,
            None,
            PagingBacking32::Read,
        ))
        .access(),
        PagingAccess32::Write
    );
}

#[test]
fn wrap_and_full_span_stop_at_first_selected_page_or_backing_failure() {
    let mut memory = fixture();
    memory
        .certify_paging_range32(range(0xfffff000), USER_RW)
        .unwrap();
    memory
        .certify_paging_range32(range(0), PagingCertificate32::NotPresent)
        .unwrap();
    let facts = violation(check(
        &memory,
        0xfffffffe,
        4,
        PagingAccess32::Write,
        PagingDomain32::User,
        None,
    ));
    assert_eq!(facts.linear(), GuestAddress(0));
    memory
        .certify_paging_range32(range(0xfffff000), PagingCertificate32::Unknown)
        .unwrap();
    assert_eq!(
        check(
            &memory,
            0xfffffffe,
            4,
            PagingAccess32::Write,
            PagingDomain32::User,
            None
        ),
        Err(PagingCheckError32::Uncertified(GuestAddress(0xfffffffe)))
    );
    memory
        .protect(range(0xfffff000), crate::memory::Permissions::EXECUTE)
        .unwrap();
    memory
        .certify_paging_range32(range(0xfffff000), USER_RW)
        .unwrap();
    assert_eq!(
        check(
            &memory,
            0xfffffffe,
            4,
            PagingAccess32::Read,
            PagingDomain32::User,
            None
        ),
        Err(PagingCheckError32::Memory(MemoryError::Fault(
            MemoryFault {
                address: GuestAddress(0xfffffffe),
                access: Access::Read,
                reason: FaultReason::Permission,
            }
        )))
    );
    memory.certify_paging_range32(range(0), USER_RW).unwrap();
    if let Ok(full_ring) = usize::try_from(1_u64 << 32) {
        assert_eq!(
            check(
                &memory,
                0,
                full_ring,
                PagingAccess32::Read,
                PagingDomain32::User,
                None
            ),
            Err(PagingCheckError32::Uncertified(GuestAddress(0x1000)))
        );
    }
}

#[test]
fn mapping_permission_mutations_revoke_certificates_but_data_writes_do_not() {
    let mut memory = fixture();
    for mutation in 0..3 {
        memory
            .certify_paging_range32(range(0x1000), USER_RW)
            .unwrap();
        match mutation {
            0 => memory
                .protect(range(0x1000), crate::memory::Permissions::ALL)
                .unwrap(),
            1 => memory
                .protect_with_word_write32(
                    range(0x1000),
                    crate::memory::Permissions::ALL,
                    crate::memory::WordWrite32 {
                        address: GuestAddress(0),
                        value: 0x12345678,
                    },
                )
                .unwrap(),
            _ => {
                memory.unmap(range(0x1000)).unwrap();
                memory
                    .map_zeroed(range(0x1000), crate::memory::Permissions::ALL)
                    .unwrap();
            }
        }
        assert_eq!(
            check(
                &memory,
                0x1000,
                1,
                PagingAccess32::Read,
                PagingDomain32::User,
                None
            ),
            Err(PagingCheckError32::Uncertified(GuestAddress(0x1000)))
        );
    }
    memory
        .certify_paging_range32(range(0x1000), USER_RW)
        .unwrap();
    memory.write(GuestAddress(0x1000), &[0x44]).unwrap();
    assert_eq!(
        check(
            &memory,
            0x1000,
            1,
            PagingAccess32::Read,
            PagingDomain32::User,
            None
        ),
        Ok(())
    );
    memory
        .certify_paging_range32(range(0x2000), PagingCertificate32::NotPresent)
        .unwrap();
    memory
        .map_zeroed(range(0x2000), crate::memory::Permissions::ALL)
        .unwrap();
    assert_eq!(
        check(
            &memory,
            0x2000,
            1,
            PagingAccess32::Read,
            PagingDomain32::User,
            None
        ),
        Err(PagingCheckError32::Uncertified(GuestAddress(0x2000)))
    );
}

#[test]
fn certificate_updates_participate_in_code_identity_and_fail_atomically_at_version_exhaustion() {
    let mut memory = fixture();
    memory
        .certify_paging_range32(range(0x1000), USER_RW)
        .unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0x1000), 2).unwrap();
    memory
        .certify_paging_range32(
            range(0x1000),
            PagingCertificate32::Present {
                user: true,
                writable: false,
            },
        )
        .unwrap();
    assert!(!memory.is_code_current(&snapshot));
    let before = memory.paging_certificates.as_ref().unwrap().to_vec();
    let backing = memory.backing.to_vec();
    let version = memory.code_validation_version();
    memory.exhaust_versions_for_test();
    assert_eq!(
        memory.certify_paging_range32(range(0x1000), USER_RW),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(
        memory.protect(range(0x1000), crate::memory::Permissions::ALL),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(
        memory.unmap(range(0x1000)),
        Err(MemoryError::VersionExhausted)
    );
    assert_eq!(
        memory.paging_certificates.as_ref().unwrap().as_ref(),
        before
    );
    assert_eq!(memory.backing.as_ref(), backing);
    assert_eq!(memory.code_validation_version(), version);
    assert!(matches!(
        memory.certify_paging_range32(range(0x2000), USER_RW),
        Err(MemoryError::NotMapped { .. })
    ));
    assert_eq!(
        memory.paging_certificates.as_ref().unwrap().as_ref(),
        before
    );
}

#[test]
fn default_address_space_keeps_strict_host_semantics_without_allocating_or_certifying_paging() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(range(0x1000), crate::memory::Permissions::ALL)
        .unwrap();
    assert!(memory.paging_certificates.is_none());
    assert_eq!(
        check(
            &memory,
            0x1000,
            4,
            PagingAccess32::Read,
            PagingDomain32::User,
            None
        ),
        Ok(())
    );
    assert_eq!(
        memory.certify_paging_range32(range(0x1000), USER_RW),
        Err(MemoryError::InvalidRange)
    );
    assert!(memory.paging_certificates.is_none());
}
