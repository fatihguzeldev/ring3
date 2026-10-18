use super::*;

fn initialized() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x2000] {
        memory.map_zeroed(range(address), Permissions::ALL).unwrap();
    }
    memory
        .write(GuestAddress(0x1ffe), &[0x11, 0x22, 0x33, 0x44])
        .unwrap();
    memory
}

#[test]
fn checked_read_keeps_write_intent_and_backing_read_fault_order() {
    let mut memory = initialized();
    let mut output = [0xa5; 4];
    memory.protect(range(0x1000), Permissions::READ).unwrap();
    let facts = fault(memory.cpu_paging_read32(GuestAddress(0x1ffe), &mut output, Access::Write));
    assert_eq!(
        (facts.linear(), facts.access(), facts.present()),
        (GuestAddress(0x1ffe), Access::Write, true)
    );
    assert_eq!(output, [0xa5; 4]);

    memory
        .protect(range(0x1000), Permissions::READ_WRITE)
        .unwrap();
    for permission in [Permissions::from_bits(2).unwrap(), Permissions::READ] {
        memory.protect(range(0x2000), permission).unwrap();
        let facts =
            fault(memory.cpu_paging_read32(GuestAddress(0x1ffe), &mut output, Access::Write));
        assert_eq!(
            (facts.linear(), facts.access(), facts.present()),
            (GuestAddress(0x2000), Access::Write, true)
        );
        assert_eq!(output, [0xa5; 4]);
    }
    memory
        .protect(range(0x2000), Permissions::READ_WRITE)
        .unwrap();
    memory
        .cpu_paging_read32(GuestAddress(0x1ffe), &mut output, Access::Write)
        .unwrap();
    assert_eq!(output, [0x11, 0x22, 0x33, 0x44]);
}

#[test]
fn checked_read_wrap_is_atomic_and_observes_mapping_changes_immediately() {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0xffff_f000, 0] {
        memory.map_zeroed(range(address), Permissions::ALL).unwrap();
    }
    memory
        .write(GuestAddress(0xffff_fffe), &[0x11, 0x22])
        .unwrap();
    memory.write(GuestAddress(0), &[0x33, 0x44]).unwrap();
    let mut output = [0xa5; 4];
    memory
        .protect(range(0), Permissions::from_bits(2).unwrap())
        .unwrap();
    let facts =
        fault(memory.cpu_paging_read32(GuestAddress(0xffff_fffe), &mut output, Access::Read));
    assert_eq!((facts.linear(), facts.present()), (GuestAddress(0), true));
    assert_eq!(output, [0xa5; 4]);

    memory.protect(range(0), Permissions::READ_WRITE).unwrap();
    memory
        .cpu_paging_read32(GuestAddress(0xffff_fffe), &mut output, Access::Read)
        .unwrap();
    assert_eq!(output, [0x11, 0x22, 0x33, 0x44]);
    output.fill(0xa5);
    memory.unmap(range(0)).unwrap();
    let facts =
        fault(memory.cpu_paging_read32(GuestAddress(0xffff_fffe), &mut output, Access::Read));
    assert_eq!((facts.linear(), facts.present()), (GuestAddress(0), false));
    assert_eq!(output, [0xa5; 4]);
    memory.map_zeroed(range(0), Permissions::READ).unwrap();
    memory
        .cpu_paging_read32(GuestAddress(0xffff_fffe), &mut output, Access::Read)
        .unwrap();
    assert_eq!(output, [0x11, 0x22, 0, 0]);
}

#[test]
fn empty_checked_read_does_not_require_a_mapping_or_modify_output() {
    let memory = AddressSpace::new(1).unwrap();
    for access in [Access::Read, Access::Write] {
        assert_eq!(
            memory.cpu_paging_read32(GuestAddress(u32::MAX), &mut [], access),
            Ok(())
        );
    }
}
