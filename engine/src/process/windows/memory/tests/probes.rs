use super::*;
use crate::windows::memory::MemoryApi32 as Api;
#[test]
fn pointer_probes_check_actual_permissions_boundaries_and_string_terminators() {
    let mut engine = engine();
    let memory = engine.memory.as_mut().unwrap();
    memory.write(GuestAddress(0x2ffe), &[b'x', 0]).unwrap();
    assert_eq!(
        engine
            .memory_probe(Api::IsBadReadPtr, &[0x2ffe, 2])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .memory_probe(Api::IsBadWritePtr, &[0x3000, 1])
            .unwrap(),
        0
    );
    assert_eq!(engine.memory_probe(Api::IsBadReadPtr, &[0, 0]).unwrap(), 0);
    assert_eq!(
        engine
            .memory_probe(Api::IsBadReadPtr, &[0x4000, 1])
            .unwrap(),
        1
    );
    assert_eq!(
        engine
            .memory_probe(Api::IsBadStringPtrA, &[0x2ffe, u32::MAX])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .memory_probe(Api::IsBadStringPtrW, &[0xffff_fffe, 2])
            .unwrap(),
        1
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(
            crate::memory::PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            crate::memory::Permissions::READ,
        )
        .unwrap();
    assert_eq!(
        engine.memory_probe(Api::IsBadCodePtr, &[0x1000]).unwrap(),
        0
    );
    assert_eq!(
        engine
            .memory_probe(Api::IsBadWritePtr, &[0x1000, 1])
            .unwrap(),
        1
    );
}
