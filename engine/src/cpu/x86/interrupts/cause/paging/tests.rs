use super::*;

#[test]
fn supplied_legacy_page_metadata_has_exact_codes_and_no_software_fault_conversion() {
    use LegacyPageViolation32::*;
    use PagingAccess32::*;
    use PagingDomain32::*;
    let profile = PageFaultProfile32::legacy32(None);
    for (access, domain, violation, expected) in [
        (Read, Supervisor, NotPresent, 0),
        (Write, Supervisor, NotPresent, 2),
        (Execute, Supervisor, NotPresent, 0),
        (Read, User, NotPresent, 4),
        (Write, User, NotPresent, 6),
        (Execute, User, NotPresent, 4),
        (Read, User, SupervisorPage, 5),
        (Write, User, SupervisorPage, 7),
        (Execute, User, SupervisorPage, 5),
        (Write, User, ReadOnlyPage, 7),
    ] {
        let fault =
            VerifiedPageFault32::new(profile, GuestAddress(0xfffffff1), access, domain, violation)
                .unwrap();
        assert_eq!(fault.error_code(), expected);
        assert_eq!(fault.linear(), GuestAddress(0xfffffff1));
        assert_eq!(
            (
                fault.profile(),
                fault.access(),
                fault.domain(),
                fault.violation()
            ),
            (profile, access, domain, violation)
        );
        assert_eq!(fault.error_code() & !7, 0);
    }
    for wp in [None, Some(false), Some(true)] {
        let result = VerifiedPageFault32::new(
            PageFaultProfile32::legacy32(wp),
            GuestAddress(3),
            Write,
            Supervisor,
            ReadOnlyPage,
        );
        assert_eq!(
            result.map(|fault| fault.error_code()),
            if wp == Some(true) {
                Ok(3)
            } else {
                Err(InvalidPageFault32)
            }
        );
    }
    for (access, domain, violation) in [
        (Read, User, ReadOnlyPage),
        (Execute, User, ReadOnlyPage),
        (Read, Supervisor, SupervisorPage),
        (Write, Supervisor, SupervisorPage),
        (Execute, Supervisor, SupervisorPage),
        (Read, Supervisor, ReadOnlyPage),
        (Execute, Supervisor, ReadOnlyPage),
    ] {
        assert_eq!(
            VerifiedPageFault32::new(
                PageFaultProfile32::legacy32(Some(true)),
                GuestAddress(0),
                access,
                domain,
                violation
            ),
            Err(InvalidPageFault32)
        );
    }
}
