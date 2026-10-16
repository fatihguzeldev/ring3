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

#[test]
fn captured_bstr_and_scalar_array_calls_publish_real_heap_values_and_retire_owned_allocations() {
    let mut e = engine();
    e.map(0x3000, 1, 7).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1100), &[b'a', 0, 0, 0, b'z', 0])
        .unwrap();
    let string = call(&mut e, Api::SysAllocStringLen, &[0x1100, 3]);
    assert_ne!(string, 0);
    assert_eq!(call(&mut e, Api::SysStringByteLen, &[string]), 6);
    assert_eq!(read(&e, string, 8), [b'a', 0, 0, 0, b'z', 0, 0, 0]);
    call(&mut e, Api::SysFreeString, &[string]);
    assert!(e.automation.strings.is_empty());
    assert_eq!(e.automation.bytes, 0);
    let array = call(&mut e, Api::SafeArrayCreateVector, &[3, (-2i32) as u32, 3]);
    assert_ne!(array, 0);
    assert_eq!(call(&mut e, Api::SafeArrayGetDim, &[array]), 1);
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1200), &(-1i32).to_le_bytes())
        .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1100), &0x11335577u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        call(&mut e, Api::SafeArrayPutElement, &[array, 0x1200, 0x1100]),
        0
    );
    assert_eq!(
        call(&mut e, Api::SafeArrayGetElement, &[array, 0x1200, 0x1300]),
        0
    );
    assert_eq!(read(&e, 0x1300, 4), 0x11335577u32.to_le_bytes());
    assert_eq!(call(&mut e, Api::SafeArrayDestroy, &[array]), 0);
    assert!(e.automation.arrays.is_empty());
    assert_eq!(e.automation.bytes, 0);
}

#[test]
fn captured_array_guarded_output_returns_hresult_without_changing_locks_or_last_error() {
    use crate::memory::{PageRange, Permissions};

    let mut e = engine();
    e.map(0x3000, 1, 7).unwrap();
    e.map(0x4000, 1, 3).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    let array = call(&mut e, Api::SafeArrayCreateVector, &[3, 0, 2]);
    assert_ne!(array, 0);
    let output = 0xfeed_beefu32.to_le_bytes();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x4000), &output)
        .unwrap();
    let thread = e
        .current_windows_thread()
        .unwrap()
        .with_last_error(0x1234_5678);
    e.publish_windows_thread(thread).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .protect(
            PageRange::new(GuestAddress(0x4000), 1).unwrap(),
            Permissions::READ_WRITE.with_guard(),
        )
        .unwrap();
    let owned_bytes = e.automation.bytes;
    let arrays = e.automation.arrays.len();
    let descriptor = read(&e, array, 24);
    let pages = e.memory().unwrap().mapped_pages();

    assert_eq!(
        call(&mut e, Api::SafeArrayAccessData, &[array, 0x4000]),
        0x8007_03e6
    );
    assert_eq!(
        e.current_windows_thread().unwrap().last_error(),
        0x1234_5678
    );
    assert_eq!(read(&e, array, 24), descriptor);
    assert_eq!(read(&e, array + 8, 4), [0; 4]);
    assert_eq!(read(&e, 0x4000, 4), output);
    assert_eq!(e.automation.bytes, owned_bytes);
    assert_eq!(e.automation.arrays.len(), arrays);
    assert_eq!(e.memory().unwrap().mapped_pages(), pages);
    assert_eq!(e.memory().unwrap().guard_page(GuestAddress(0x4000)), None);
    assert!(e.pending_call.is_none());

    assert_eq!(call(&mut e, Api::SafeArrayAccessData, &[array, 0x4000]), 0);
    assert_ne!(read(&e, 0x4000, 4), output);
    assert_eq!(read(&e, array + 8, 4), 1u32.to_le_bytes());
    assert_eq!(call(&mut e, Api::SafeArrayUnaccessData, &[array]), 0);
    assert_eq!(call(&mut e, Api::SafeArrayDestroy, &[array]), 0);
}

#[test]
fn scalar_array_layout_access_and_redim_match_the_native_x86_oracle() {
    let mut e = engine();
    let bounds = [2u32, (-3i32) as u32, 3, 7]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1100), &bounds)
        .unwrap();
    let array = e
        .automation_array_call(Api::SafeArrayCreate, &[3, 2, 0x1100])
        .unwrap();
    assert_eq!(
        read(&e, array + 16, 16),
        [3, 0, 0, 0, 7, 0, 0, 0, 2, 0, 0, 0, 253, 255, 255, 255]
    );
    e.automation_array_call(Api::SafeArrayAccessData, &[array, 0x1200])
        .unwrap();
    let data = u32::from_le_bytes(read(&e, 0x1200, 4).try_into().unwrap());
    for index in 0..6 {
        e.memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(data + index * 4),
                &(100u32 + index).to_le_bytes(),
            )
            .unwrap();
    }
    assert_eq!(
        e.automation_array_call(Api::SafeArrayDestroy, &[array]),
        Err(HostError::Automation(E::ArrayLocked))
    );
    e.automation_array_call(Api::SafeArrayUnaccessData, &[array])
        .unwrap();
    for (indices, value) in [([-3i32, 7], 100u32), ([-2, 7], 101), ([-3, 8], 102)] {
        e.memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(0x1100),
                &indices
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        e.automation_array_call(Api::SafeArrayGetElement, &[array, 0x1100, 0x1300])
            .unwrap();
        assert_eq!(read(&e, 0x1300, 4), value.to_le_bytes());
    }
    e.memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x1100),
            &[4u32.to_le_bytes(), 11u32.to_le_bytes()].concat(),
        )
        .unwrap();
    e.automation_array_call(Api::SafeArrayRedim, &[array, 0x1100])
        .unwrap();
    assert_eq!(read(&e, array + 16, 8), [4, 0, 0, 0, 11, 0, 0, 0]);
    e.automation_array_call(Api::SafeArrayAccessData, &[array, 0x1200])
        .unwrap();
    let data = u32::from_le_bytes(read(&e, 0x1200, 4).try_into().unwrap());
    let expected = [100u32, 101, 102, 103, 104, 105, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(read(&e, data, 32), expected);
    e.automation_array_call(Api::SafeArrayUnaccessData, &[array])
        .unwrap();
    e.automation_array_call(Api::SafeArrayDestroy, &[array])
        .unwrap();
    assert_eq!(e.automation.bytes, 0);
    assert!(e.automation.arrays.is_empty());
}
#[test]
fn array_access_and_copy_output_faults_preserve_locks_ownership_and_memory_versions() {
    let mut e = engine();
    let array = e
        .automation_array_call(Api::SafeArrayCreateVector, &[3, (-2i32) as u32, 3])
        .unwrap();
    assert_eq!(read(&e, array + 2, 2), 0x2080u16.to_le_bytes());
    let version = e.memory.as_ref().unwrap().transaction_version();
    let bytes = e.automation.bytes;
    assert!(matches!(
        e.automation_array_call(Api::SafeArrayAccessData, &[array, 0xfffffff0]),
        Err(HostError::Memory(_))
    ));
    assert_eq!(read(&e, array + 8, 4), [0; 4]);
    assert_eq!(e.memory.as_ref().unwrap().transaction_version(), version);
    assert!(matches!(
        e.automation_array_call(Api::SafeArrayCopy, &[array, 0xfffffff0]),
        Err(HostError::Memory(_))
    ));
    assert_eq!(e.automation.arrays.len(), 1);
    assert_eq!(e.automation.bytes, bytes);
    e.automation_array_call(Api::SafeArrayCopy, &[array, 0x1200])
        .unwrap();
    let copy = u32::from_le_bytes(read(&e, 0x1200, 4).try_into().unwrap());
    assert_ne!(copy, array);
    e.automation_array_call(Api::SafeArrayDestroy, &[copy])
        .unwrap();
    e.automation_array_call(Api::SafeArrayDestroy, &[array])
        .unwrap();
    assert_eq!(e.automation.bytes, 0);
}

use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
};
pub(in crate::process::windows::automation) fn call(
    e: &mut EngineInstance,
    api: Api,
    args: &[u32],
) -> u32 {
    for (index, value) in [0x3000u32, 2, 0x3000, api.id()].into_iter().enumerate() {
        e.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    e.compile_with_gates(1, 1).unwrap();
    for (index, value) in std::iter::once(0x3002)
        .chain(args.iter().copied())
        .enumerate()
    {
        e.memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(0x1000 + index as u32 * 4),
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    let mut state = State32 {
        eip: 0x3000,
        ..Default::default()
    };
    state.registers[4] = 0x1000;
    encode_state_current(&state, &mut e.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut e.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let generation = e.generation;
    let token = e.capture_windows_call(0xa070, generation).unwrap().token;
    e.complete_windows_call(0xa070, generation, token).unwrap();
    decode_state(&e.arena()[..STATE_SIZE]).unwrap().registers[0]
}
