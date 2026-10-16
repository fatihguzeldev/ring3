use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, Oleaut32Api32 as Api},
};
pub(in crate::process::windows::automation) fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(128, 0xa070).unwrap();
    e.map(0x1000, 1, 3).unwrap();
    e
}
pub(in crate::process::windows::automation) fn read(
    e: &EngineInstance,
    at: u32,
    len: usize,
) -> Vec<u8> {
    let mut b = vec![0; len];
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(at), &mut b)
        .unwrap();
    b
}
#[test]
fn bstr_embedded_null_and_binary_lengths_are_preserved_and_realloc_aliases_are_snapshotted() {
    let mut e = engine();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1100), &[b'a', 0, 0, 0, b'b', 0])
        .unwrap();
    let b = e
        .automation_string(Api::SysAllocStringLen, &[0x1100, 3])
        .unwrap();
    assert_eq!(e.automation_string(Api::SysStringLen, &[b]).unwrap(), 3);
    assert_eq!(read(&e, b, 8), [b'a', 0, 0, 0, b'b', 0, 0, 0]);
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1200), &b.to_le_bytes())
        .unwrap();
    assert_eq!(
        e.automation_string(Api::SysReAllocStringLen, &[0x1200, b + 4, 1])
            .unwrap(),
        1
    );
    let new = u32::from_le_bytes(read(&e, 0x1200, 4).try_into().unwrap());
    assert_eq!(read(&e, new, 4), [b'b', 0, 0, 0]);
    assert_eq!(e.automation.strings.len(), 1);
    e.automation_string(Api::SysFreeString, &[new]).unwrap();
    assert_eq!(e.automation.bytes, 0);
    let odd = e
        .automation_string(Api::SysAllocStringByteLen, &[0x1100, 3])
        .unwrap();
    assert_eq!(e.automation_string(Api::SysStringLen, &[odd]).unwrap(), 1);
    assert_eq!(
        e.automation_string(Api::SysStringByteLen, &[odd]).unwrap(),
        3
    );
}
#[test]
fn reallocation_bad_output_and_capacity_leave_old_bstr_and_heap_unchanged() {
    let mut e = engine();
    let b = e
        .automation_string(Api::SysAllocStringLen, &[0, 2])
        .unwrap();
    let version = e.memory.as_ref().unwrap().transaction_version();
    assert!(matches!(
        e.automation_string(Api::SysReAllocStringLen, &[0xfffffff0, b, 2]),
        Err(HostError::Memory(_))
    ));
    assert_eq!(e.memory.as_ref().unwrap().transaction_version(), version);
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1200), &b.to_le_bytes())
        .unwrap();
    let before = read(&e, b - 4, 10);
    assert_eq!(
        e.automation_string(Api::SysReAllocStringLen, &[0x1200, b, u32::MAX]),
        Err(HostError::Automation(E::Capacity))
    );
    assert_eq!(read(&e, b - 4, 10), before);
    assert_eq!(read(&e, 0x1200, 4), b.to_le_bytes());
    assert_eq!(e.automation.strings.len(), 1);
}
