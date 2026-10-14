use super::*;

#[test]
fn application_fault_codes_identify_presence_write_and_execute() {
    for (access, present, expected) in [
        (PagingAccess32::Read, false, 4),
        (PagingAccess32::Read, true, 5),
        (PagingAccess32::Write, false, 6),
        (PagingAccess32::Write, true, 7),
        (PagingAccess32::Execute, false, 20),
        (PagingAccess32::Execute, true, 21),
    ] {
        let fault = VerifiedPageFault32::application(GuestAddress(0xfffffff1), access, present);
        assert_eq!(fault.linear(), GuestAddress(0xfffffff1));
        assert_eq!(fault.access(), access);
        assert_eq!(fault.domain(), PagingDomain32::User);
        assert_eq!(fault.error_code(), expected);
    }
}
