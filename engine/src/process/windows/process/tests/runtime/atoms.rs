use super::*;
use crate::memory::GuestAddress;
#[test]
fn global_atom_names_preserve_case_reference_counts_and_integer_identity() {
    let mut engine = EngineInstance::new(16, 7).unwrap();
    engine.map(0x10000, 1, 3).unwrap();
    for (at, text) in [
        (0x10000, b"Example\0".as_slice()),
        (0x10100, b"eXAMPLE\0".as_slice()),
        (0x10200, b"#42\0".as_slice()),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(at), text)
            .unwrap();
    }
    let atom = engine
        .execute_process_call(api("GlobalAddAtomA"), &[0x10000])
        .unwrap()
        .value as u32;
    assert!(atom >= 0xc000);
    assert_eq!(
        engine
            .execute_process_call(api("GlobalAddAtomA"), &[0x10100])
            .unwrap()
            .value,
        u64::from(atom)
    );
    assert_eq!(
        engine
            .execute_process_call(api("GlobalGetAtomNameA"), &[atom, 0x10300, 32])
            .unwrap()
            .value,
        7
    );
    let mut name = [0; 8];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x10300), &mut name)
        .unwrap();
    assert_eq!(&name, b"Example\0");
    engine
        .execute_process_call(api("GlobalDeleteAtom"), &[atom])
        .unwrap();
    assert_eq!(
        engine
            .execute_process_call(api("GlobalFindAtomA"), &[0x10100])
            .unwrap()
            .value,
        u64::from(atom)
    );
    engine
        .execute_process_call(api("GlobalDeleteAtom"), &[atom])
        .unwrap();
    assert_eq!(
        engine
            .execute_process_call(api("GlobalFindAtomA"), &[0x10000])
            .unwrap()
            .value,
        0
    );
    assert_eq!(
        engine
            .execute_process_call(api("GlobalAddAtomA"), &[0x10200])
            .unwrap()
            .value,
        42
    );
    assert_eq!(
        engine
            .execute_process_call(api("GlobalAddAtomA"), &[42])
            .unwrap()
            .value,
        42
    );
}
