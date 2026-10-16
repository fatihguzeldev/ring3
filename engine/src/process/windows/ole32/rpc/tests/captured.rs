#[path = "../../../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;

use super::super::context::{Broker32, BuildPhase32};
use super::*;
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    process::{EngineInstance, ModuleRuntimeConfig32},
    windows::{ole32::RpcChannelMethod32, thread::ThreadState32},
};
const KEY: u64 = 77;
const MESSAGE: u32 = 0x7e001000;

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(512, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_ole32_module(0x76000000).unwrap();
    engine
        .configure_ole32(Ole32Config32 {
            objects: PageRange::new(GuestAddress(0x68000000), 16).unwrap(),
            max_objects: 8,
            max_threads: 8,
        })
        .unwrap();
    engine
        .register_guest_module("rpc-captured.exe", &pe::image(false, 0x400000, &[], &[]))
        .unwrap();
    engine
        .start_module_process(
            "rpc-captured.exe",
            None,
            ModuleRuntimeConfig32 {
                stack_base: 0x7e000000,
                stack_pages: 8,
                runtime_base: 0x7f000000,
            },
        )
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine.load_library_module("ole32.dll", None).unwrap();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .apartments
        .initialize(1, 0, 2)
        .unwrap();
    assert_eq!(engine.ensure_com_apartment_host(true).unwrap(), 2);
    assert_eq!(
        engine.windows_thread_info(2).unwrap().state,
        ThreadState32::Waiting
    );
    for (address, value) in [
        (MESSAGE + 0x100, MESSAGE + 0x200),
        (MESSAGE + 0x200 + 20, 0x401000),
    ] {
        engine.write32(address, value).unwrap();
    }
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.rpc.brokers[0] = Some(Broker32 {
        source: crate::process::windows::ole32::marshaling::ExecutionApartment32 {
            apartment: crate::windows::ole32::ApartmentId32::Sta(2),
            thread: 2,
        },
        client: crate::process::windows::ole32::marshaling::ExecutionApartment32 {
            apartment: crate::windows::ole32::ApartmentId32::Sta(1),
            thread: 1,
        },
        requested: crate::windows::com::InterfaceId([9; 16]),
        source_object: 0,
        owns_source: false,
        result: 0,
        source_factory: 0,
        stub: MESSAGE + 0x100,
        client_factory: 0,
        proxy: 0,
        interface: 0,
        controller: None,
        channel: None,
        controller_live: false,
        channel_live: false,
        proxy_channel_references: 1,
        connected: true,
        build: BuildPhase32::Publish,
        rundown: None,
    });
    engine.prepare_rpc_identities(0).unwrap();
    message(engine.memory.as_mut().unwrap(), MESSAGE, 12);
    engine
        .ole32
        .as_mut()
        .unwrap()
        .allocate_rpc_buffer(engine.memory.as_mut().unwrap(), 0, MESSAGE)
        .unwrap();
    engine
}

fn capture(engine: &mut EngineInstance, method: RpcChannelMethod32, args: &[u32]) -> (u32, u32) {
    let gate = engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == method.id())
        .unwrap()
        .entry
        .0;
    for (index, value) in [gate, 2, gate, method.id()].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = gate;
    state.registers[4] = 0x7e007e00;
    let bytes: Vec<_> = core::iter::once(0x401000u32)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(state.registers[4]), &bytes)
        .unwrap();
    encode_state_current(&state, &mut engine.arena.as_mut().get_mut()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: method.id() },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine.capture_windows_call(KEY, generation).unwrap().token;
    (generation, token)
}

#[test]
fn accepted_captured_invoke_keeps_channel_live_until_its_dead_sender_packet_is_retired() {
    let mut engine = engine();
    let pointer = engine.ole32.as_ref().unwrap().rpc.brokers[0]
        .as_ref()
        .unwrap()
        .channel
        .unwrap();
    let (generation, token) = capture(&mut engine, RpcChannelMethod32(4), &[pointer.0, MESSAGE, 0]);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(
        engine.ole32.as_ref().unwrap().rpc.calls.len(),
        1,
        "the actual captured gate accepted and parked the invocation"
    );
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    assert!(
        prepared.ranges().is_empty(),
        "an accepted source invocation still borrows the zero-reference channel"
    );
    engine.check_com_thread_exit(&prepared).unwrap();
    engine.commit_com_thread_exit_after_memory(prepared, false);
    let runtime = engine.ole32.as_ref().unwrap();
    assert_eq!(runtime.com.references(pointer).unwrap(), 0);
    runtime
        .com
        .preflight(engine.memory().unwrap(), pointer)
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    engine.service_abandoned_rpc_work().unwrap();
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.com.lookup(pointer).is_err());
    assert!(runtime.rpc.calls.is_empty() && runtime.rpc.buffers.is_empty());
    assert_eq!(runtime.streams.store.reserved_bytes(), 0);
    assert_eq!(
        engine.memory().unwrap().transaction_version(),
        version + 1,
        "channel, packet and call scratch retire in one RAM transaction"
    );
}
