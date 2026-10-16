use super::super::tests::{call, engine, read};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, AutomationLimits32, Oleaut32Api32 as Api},
};
fn initialized() -> EngineInstance {
    let mut e = engine();
    e.map(0x3000, 1, 7).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    e
}
fn word(e: &mut EngineInstance, at: u32, value: u32) {
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(at), &value.to_le_bytes())
        .unwrap();
}
fn pointer(e: &EngineInstance, at: u32) -> u32 {
    u32::from_le_bytes(read(e, at, 4).try_into().unwrap())
}
fn string(e: &mut EngineInstance, text: &[u16]) -> u32 {
    let bytes = text
        .iter()
        .copied()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1100), &bytes)
        .unwrap();
    call(e, Api::SysAllocStringLen, &[0x1100, text.len() as u32])
}
#[test]
fn native_null_bstr_array_get_allocates_an_empty_string_while_copy_preserves_null_slots() {
    let mut e = initialized();
    let array = call(&mut e, Api::SafeArrayCreateVector, &[8, 0, 1]);
    word(&mut e, 0x1200, 0);
    assert_eq!(
        call(&mut e, Api::SafeArrayGetElement, &[array, 0x1200, 0x1300]),
        0
    );
    let empty = pointer(&e, 0x1300);
    assert_ne!(empty, 0);
    assert_eq!(call(&mut e, Api::SysStringByteLen, &[empty]), 0);
    assert_eq!(read(&e, empty, 2), [0; 2]);
    call(&mut e, Api::SysFreeString, &[empty]);
    assert_eq!(
        call(&mut e, Api::SafeArrayPutElement, &[array, 0x1200, 0]),
        0
    );
    assert_eq!(call(&mut e, Api::SafeArrayCopy, &[array, 0x1400]), 0);
    let copy = pointer(&e, 0x1400);
    call(&mut e, Api::SafeArrayAccessData, &[copy, 0x1300]);
    let data = pointer(&e, 0x1300);
    assert_eq!(read(&e, data, 4), [0; 4]);
    call(&mut e, Api::SafeArrayUnaccessData, &[copy]);
    assert_eq!(
        call(&mut e, Api::SafeArrayGetElement, &[copy, 0x1200, 0x1300]),
        0
    );
    let empty = pointer(&e, 0x1300);
    assert_ne!(empty, 0);
    call(&mut e, Api::SysFreeString, &[empty]);
    call(&mut e, Api::SafeArrayDestroy, &[array]);
    call(&mut e, Api::SafeArrayDestroy, &[copy]);
    assert_eq!(e.automation.bytes, 0);
}
#[test]
fn captured_bstr_array_copy_get_and_destroy_keep_independent_embedded_null_strings() {
    let mut e = initialized();
    let array = call(&mut e, Api::SafeArrayCreateVector, &[8, (-1i32) as u32, 2]);
    assert_ne!(array, 0);
    let source = string(&mut e, &[97, 0, 122]);
    word(&mut e, 0x1200, (-1i32) as u32);
    assert_eq!(
        call(&mut e, Api::SafeArrayPutElement, &[array, 0x1200, source]),
        0
    );
    call(&mut e, Api::SysFreeString, &[source]);
    assert_eq!(
        call(&mut e, Api::SafeArrayGetElement, &[array, 0x1200, 0x1300]),
        0
    );
    let first = pointer(&e, 0x1300);
    assert_eq!(read(&e, first, 8), [97, 0, 0, 0, 122, 0, 0, 0]);
    assert_eq!(call(&mut e, Api::SafeArrayCopy, &[array, 0x1400]), 0);
    let copy = pointer(&e, 0x1400);
    assert_ne!(copy, array);
    assert_eq!(call(&mut e, Api::SafeArrayDestroy, &[array]), 0);
    assert_eq!(
        call(&mut e, Api::SafeArrayGetElement, &[copy, 0x1200, 0x1300]),
        0
    );
    let second = pointer(&e, 0x1300);
    assert_ne!(first, second);
    assert_eq!(read(&e, first, 8), read(&e, second, 8));
    call(&mut e, Api::SysFreeString, &[first]);
    call(&mut e, Api::SysFreeString, &[second]);
    call(&mut e, Api::SafeArrayDestroy, &[copy]);
    assert!(e.automation.strings.is_empty());
    assert!(e.automation.arrays.is_empty());
    assert_eq!(e.automation.bytes, 0);
}
#[test]
fn bstr_replacement_uses_one_live_slot_and_failed_get_or_copy_has_no_heap_side_effect() {
    let mut e = initialized();
    e.configure_automation(AutomationLimits32 {
        objects: 3,
        ..Default::default()
    })
    .unwrap();
    let array = call(&mut e, Api::SafeArrayCreateVector, &[8, 0, 1]);
    word(&mut e, 0x1200, 0);
    let source = string(&mut e, &[97]);
    call(&mut e, Api::SafeArrayPutElement, &[array, 0x1200, source]);
    word(&mut e, 0x1400, source);
    word(&mut e, 0x1100, 98);
    assert_eq!(
        call(&mut e, Api::SysReAllocStringLen, &[0x1400, 0x1100, 1]),
        1
    );
    let source = pointer(&e, 0x1400);
    assert_eq!(
        call(&mut e, Api::SafeArrayPutElement, &[array, 0x1200, source]),
        0
    );
    assert_eq!(e.automation.strings.len(), 2);
    call(&mut e, Api::SysFreeString, &[source]);
    let bytes = e.automation.bytes;
    let version = e.memory.as_ref().unwrap().transaction_version();
    let pages = e.memory.as_ref().unwrap().mapped_pages();
    assert!(matches!(
        e.automation_array_call(Api::SafeArrayGetElement, &[array, 0x1200, 0xfffffff0]),
        Err(HostError::Memory(_))
    ));
    assert!(matches!(
        e.automation_array_call(Api::SafeArrayCopy, &[array, 0xfffffff0]),
        Err(HostError::Memory(_))
    ));
    assert_eq!(e.automation.bytes, bytes);
    assert_eq!(e.automation.strings.len(), 1);
    assert_eq!(e.memory.as_ref().unwrap().transaction_version(), version);
    assert_eq!(e.memory.as_ref().unwrap().mapped_pages(), pages);
    call(&mut e, Api::SafeArrayGetElement, &[array, 0x1200, 0x1300]);
    let result = pointer(&e, 0x1300);
    assert_eq!(read(&e, result, 4), [98, 0, 0, 0]);
    call(&mut e, Api::SafeArrayDestroy, &[array]);
    call(&mut e, Api::SysFreeString, &[result]);
    assert_eq!(e.automation.bytes, 0);
}
#[test]
fn bstr_array_resize_preserves_flat_prefix_and_retires_only_removed_owned_elements() {
    let mut e = initialized();
    let array = call(&mut e, Api::SafeArrayCreateVector, &[8, 0, 3]);
    for index in 0..3 {
        let source = string(&mut e, &[65 + index as u16]);
        word(&mut e, 0x1200, index);
        call(&mut e, Api::SafeArrayPutElement, &[array, 0x1200, source]);
        call(&mut e, Api::SysFreeString, &[source]);
    }
    assert_eq!(e.automation.strings.len(), 3);
    word(&mut e, 0x1500, 1);
    word(&mut e, 0x1504, 10);
    assert_eq!(call(&mut e, Api::SafeArrayRedim, &[array, 0x1500]), 0);
    assert_eq!(e.automation.strings.len(), 1);
    word(&mut e, 0x1200, 10);
    call(&mut e, Api::SafeArrayGetElement, &[array, 0x1200, 0x1300]);
    let result = pointer(&e, 0x1300);
    assert_eq!(read(&e, result, 4), [65, 0, 0, 0]);
    call(&mut e, Api::SysFreeString, &[result]);
    word(&mut e, 0x1500, 4);
    word(&mut e, 0x1504, 20);
    assert_eq!(call(&mut e, Api::SafeArrayRedim, &[array, 0x1500]), 0);
    call(&mut e, Api::SafeArrayAccessData, &[array, 0x1300]);
    let data = pointer(&e, 0x1300);
    assert_ne!(pointer(&e, data), 0);
    assert_eq!(read(&e, data + 4, 12), [0; 12]);
    assert_eq!(
        call(&mut e, Api::SafeArrayDestroy, &[array]),
        E::ArrayLocked.hresult()
    );
    call(&mut e, Api::SafeArrayUnaccessData, &[array]);
    call(&mut e, Api::SafeArrayDestroy, &[array]);
    assert_eq!(e.automation.bytes, 0);
}
#[test]
fn bstr_element_put_is_valid_while_access_data_is_locked_and_invalid_owned_pointer_is_neutral() {
    let mut e = initialized();
    let array = call(&mut e, Api::SafeArrayCreateVector, &[8, 0, 1]);
    word(&mut e, 0x1200, 0);
    call(&mut e, Api::SafeArrayAccessData, &[array, 0x1300]);
    let data = pointer(&e, 0x1300);
    let source = string(&mut e, &[120]);
    assert_eq!(
        call(&mut e, Api::SafeArrayPutElement, &[array, 0x1200, source]),
        0
    );
    assert_eq!(pointer(&e, array + 8), 1);
    let owned = pointer(&e, data);
    word(&mut e, data, 0x1994);
    let before = e.automation.bytes;
    let version = e.memory.as_ref().unwrap().transaction_version();
    assert_eq!(
        e.automation_array_call(Api::SafeArrayPutElement, &[array, 0x1200, source]),
        Err(HostError::Automation(E::InvalidParameter))
    );
    assert_eq!(pointer(&e, data), 0x1994);
    assert_eq!(e.automation.bytes, before);
    assert_eq!(e.memory.as_ref().unwrap().transaction_version(), version);
    word(&mut e, data, owned);
    call(&mut e, Api::SafeArrayUnaccessData, &[array]);
    call(&mut e, Api::SafeArrayDestroy, &[array]);
    call(&mut e, Api::SysFreeString, &[source]);
    assert_eq!(e.automation.bytes, 0);
}
