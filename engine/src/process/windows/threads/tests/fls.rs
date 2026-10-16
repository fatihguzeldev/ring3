use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::{EngineInstance, ModuleRuntimeConfig32},
    windows::{CallingConvention32, thread::ThreadApi32},
};
const KEY: u64 = 71;
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine.register_kernel32_module(0x7000_0000).unwrap();
    engine
        .register_guest_module("fls.exe", &super::static_tls::image(0x400000, false, false))
        .unwrap();
    engine
        .start_module_process("fls.exe", None, ModuleRuntimeConfig32::default())
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine
}
fn invoke(engine: &mut EngineInstance, id: u32, args: &[u32], stack: Option<u32>) {
    let gate = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    for (index, word) in [gate, 2, gate, id].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = gate;
    state.registers[4] = stack.unwrap_or(state.registers[4] - 64);
    let words = std::iter::once(0x401010u32)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(state.registers[4]), &words)
        .unwrap();
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_call(
            KEY,
            engine.generation,
            CallingConvention32::Stdcall,
            args.len() as u32,
        )
        .unwrap()
        .token;
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
}
fn call(engine: &mut EngineInstance, api: ThreadApi32, args: &[u32]) -> u32 {
    invoke(engine, api.id(), args, None);
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
fn returned(engine: &mut EngineInstance) {
    let index = engine.threads.current_id() as usize - 1;
    let esp = engine.threads.threads[index]
        .active_invocations
        .last()
        .unwrap()
        .return_esp;
    invoke(engine, super::super::INVOCATION_RETURN_ID, &[], Some(esp));
}
fn argument(engine: &EngineInstance) -> u32 {
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    let mut value = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(state.registers[4] + 4), &mut value)
        .unwrap();
    u32::from_le_bytes(value)
}
#[test]
fn fls_free_runs_real_stdcall_invocations_for_each_thread_value_then_restores_the_captured_caller()
{
    let mut engine = engine();
    let index = call(&mut engine, ThreadApi32::FlsAlloc, &[0x401000]);
    call(&mut engine, ThreadApi32::FlsSetValue, &[index, 11]);
    engine.threads.fls.context.set(2, index, 33).unwrap();
    invoke(&mut engine, ThreadApi32::FlsFree.id(), &[index], None);
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap().eip,
        0x401000
    );
    assert_eq!(argument(&engine), 11);
    assert_eq!(call(&mut engine, ThreadApi32::FlsGetValue, &[index]), 11);
    call(&mut engine, ThreadApi32::FlsSetValue, &[index, 44]);
    returned(&mut engine);
    assert_eq!(argument(&engine), 33);
    assert_eq!(engine.threads.fls.context.get(1, index), Ok(0));
    returned(&mut engine);
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap().eip,
        0x401010
    );
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        1
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.threads.threads[0].active_invocations.is_empty());
    assert_eq!(call(&mut engine, ThreadApi32::FlsAlloc, &[0]), index);
}
#[test]
fn normal_exit_drains_fls_before_loader_detach_and_forced_retirement_runs_no_callback() {
    let mut engine = engine();
    let index = call(&mut engine, ThreadApi32::FlsAlloc, &[0x401000]);
    call(&mut engine, ThreadApi32::FlsSetValue, &[index, 11]);
    engine.threads.fls.context.set(2, index, 22).unwrap();
    assert!(!engine.advance_windows_fls_process_exit().unwrap());
    assert_eq!(argument(&engine), 11);
    returned(&mut engine);
    assert!(engine.advance_windows_fls_process_exit().unwrap());
    assert_eq!(engine.threads.fls.context.get(1, index), Ok(0));
    engine.threads.fls.retire_thread(2);
    assert_eq!(engine.threads.fls.context.get(2, index), Ok(0));
    assert!(engine.threads.threads[0].active_invocations.is_empty());
}

#[test]
fn captured_child_return_runs_fls_before_requesting_another_scheduler_boundary() {
    let mut engine = engine();
    let index = call(&mut engine, ThreadApi32::FlsAlloc, &[0x401000]);
    let child = call(
        &mut engine,
        ThreadApi32::CreateThread,
        &[0, 0, 0x401000, 0, 0, 0],
    );
    assert_ne!(child, 0);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        crate::windows::thread::ThreadSchedule32::Running { thread: 2 }
    );
    call(&mut engine, ThreadApi32::FlsSetValue, &[index, 0xfeed]);
    let child = engine.windows_thread_info(2).unwrap();
    invoke(
        &mut engine,
        super::super::THREAD_RETURN_ID,
        &[],
        Some(child.stack_base + child.stack_pages * 4096 - 4),
    );
    assert_eq!(engine.pending_windows_thread_schedule(), None);
    assert!(engine.pending_call.is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap().eip,
        0x401000
    );
    assert_eq!(argument(&engine), 0xfeed);
    assert_eq!(engine.threads.threads[1].active_invocations.len(), 1);
    returned(&mut engine);
    let pending = engine.pending_call.as_ref().unwrap();
    let (owner, token) = (pending.owner, pending.token);
    assert!(matches!(
        crate::abi::x86::decode_exit(&pending.exit).unwrap().reason,
        ExitReason::Gate {
            id: super::super::THREAD_RETURN_ID
        }
    ));
    let crate::process::calls::call::PendingOwner::Replacement(generation) = owner else {
        panic!("the original child return retains its replacement owner");
    };
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(
        engine.windows_thread_info(2).unwrap().state,
        crate::windows::thread::ThreadState32::Terminated
    );
    assert_eq!(engine.pending_windows_thread_schedule(), Some(2));
    assert_eq!(engine.threads.fls.context.get(2, index), Ok(0));
}
