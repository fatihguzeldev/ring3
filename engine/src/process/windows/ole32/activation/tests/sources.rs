#[path = "../../../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;

use super::*;
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    files::{ATTR_DIRECTORY, ATTR_NORMAL, AssetRef32, FileReadRequest32, FileTimes32},
    process::{FileConfig32, HostError, ModuleOperation32, ModuleRuntimeConfig32},
    windows::ole32::{HResult32, IID_ICLASSFACTORY},
};

const KEY: u64 = 77;
const DATA: u32 = 0x7e001000;

fn assets() -> Vec<Vec<u8>> {
    vec![
        pe::image(
            true,
            0x500000,
            &[pe::Import {
                module: "eager.dll",
                symbols: vec![pe::Symbol::Name("Run"), pe::Symbol::Ordinal(1)],
            }],
            &[
                (1, Some("DllGetClassObject"), "@1000"),
                (2, Some("DllCanUnloadNow"), "lazy-unload.CanUnload"),
            ],
        ),
        pe::image(true, 0x600000, &[], &[(1, Some("CanUnload"), "@1000")]),
    ]
}

fn engine(assets: &[Vec<u8>]) -> EngineInstance {
    let mut engine = EngineInstance::new(512, KEY).unwrap();
    engine.configure_files(FileConfig32::default()).unwrap();
    engine
        .mount_directory(
            &r"C:\app".encode_utf16().collect::<Vec<_>>(),
            ATTR_DIRECTORY,
            FileTimes32::default(),
        )
        .unwrap();
    for (index, name) in [r"C:\app\server.dll", r"C:\app\lazy-unload.dll"]
        .into_iter()
        .enumerate()
    {
        engine
            .mount_file(
                &name.encode_utf16().collect::<Vec<_>>(),
                AssetRef32 {
                    id: index as u64 + 1,
                    length: assets[index].len() as u64,
                },
                ATTR_NORMAL,
                FileTimes32::default(),
            )
            .unwrap();
    }
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_ole32_module(0x76000000).unwrap();
    engine
        .configure_ole32(crate::process::Ole32Config32 {
            objects: PageRange::new(GuestAddress(0x68000000), 16).unwrap(),
            max_objects: 8,
            max_threads: 8,
        })
        .unwrap();
    engine
        .register_guest_module(
            "eager.dll",
            &pe::image(true, 0x700000, &[], &[(1, Some("Run"), "@1000")]),
        )
        .unwrap();
    engine
        .register_guest_module(r"C:\app\main.exe", &pe::image(false, 0x400000, &[], &[]))
        .unwrap();
    engine
        .start_module_process(
            r"C:\app\main.exe",
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
    engine
}

fn capture(
    engine: &mut EngineInstance,
    api: Ole32Api32,
    args: &[u32],
) -> (PendingOwner, u32, CallFrame32) {
    let gate = engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == api.id())
        .unwrap()
        .entry
        .0;
    let callback = engine.module_handle(Some("server.dll")).unwrap().is_some();
    let return_gate = *engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == crate::process::windows::threads::INVOCATION_RETURN_ID)
        .unwrap();
    let words = if callback {
        vec![
            gate,
            2,
            0x501000,
            1,
            return_gate.entry.0,
            2,
            gate,
            api.id(),
            return_gate.entry.0,
            return_gate.id,
        ]
    } else {
        vec![gate, 2, gate, api.id()]
    };
    for (index, value) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine
        .compile_with_gates(if callback { 3 } else { 1 }, if callback { 2 } else { 1 })
        .unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = gate;
    state.registers[4] = 0x7e007e00;
    let bytes: Vec<_> = std::iter::once(0x401000u32)
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
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine.capture_windows_call(KEY, generation).unwrap().token;
    let owner = PendingOwner::Replacement(generation);
    let frame = engine.checked_pending_call(owner, token).unwrap().frame;
    (owner, token, frame)
}

fn push(engine: &mut EngineInstance, api: Ole32Api32, args: &[u32], phase: Phase32) -> u32 {
    let (owner, token, frame) = capture(engine, api, args);
    let mut bytes = [0; 64];
    bytes[16..32].copy_from_slice(&IID_ICLASSFACTORY.0);
    bytes[32..48].copy_from_slice(&IID_ICLASSFACTORY.0);
    let scratch = GuestCallbackScratch32::allocate(
        engine.memory.as_mut().unwrap(),
        ActivationRuntime32::scratch_reservation(),
        &bytes,
    )
    .unwrap();
    let runtime = &mut engine.ole32.as_mut().unwrap().activation;
    runtime.next = 1;
    runtime.pending.push(PendingActivation32 {
        continuation: 1,
        thread: 1,
        owner,
        token,
        frame,
        api,
        out: DATA,
        outer: 0,
        path: r"C:\app\server.dll".into(),
        scratch,
        phase,
        threading: ServerThreadingModel32::Both,
        server_threads: vec![1],
        server: None,
    });
    token
}

fn supply(engine: &mut EngineInstance, request: FileReadRequest32, assets: &[Vec<u8>]) {
    let bytes = &assets[request.asset as usize - 1];
    let range = &bytes[request.offset as usize..request.offset as usize + request.length as usize];
    for (index, chunk) in range.chunks(4096).enumerate() {
        engine
            .supply_file_bytes(request, index as u32 * 4096, chunk)
            .unwrap();
    }
}

fn drain(engine: &mut EngineInstance, assets: &[Vec<u8>]) -> usize {
    let mut waits = 0;
    while let Some(source) = engine.pending_module_source() {
        assert!(!source.ready);
        let request = source.request.unwrap();
        let before = engine.pending_module_source();
        assert!(
            engine
                .resume_module_source(source.epoch, source.token + 1)
                .is_err()
        );
        assert_eq!(engine.pending_module_source(), before);
        supply(engine, request, assets);
        assert!(engine.pending_module_source().unwrap().ready);
        engine
            .resume_module_source(source.epoch, source.token)
            .unwrap();
        waits += 1;
        assert!(waits < 16);
    }
    waits
}

#[test]
fn a_registered_com_load_parks_exact_asset_bytes_until_real_factory_dispatch_is_ready() {
    let assets = assets();
    let mut engine = engine(&assets);
    engine.write32(DATA, 0x12345678).unwrap();
    let token = push(
        &mut engine,
        Ole32Api32::CoGetClassObject,
        &[DATA + 16, 1, 0, DATA + 32, DATA],
        Phase32::Load,
    );
    engine.advance_registered_com_loading(0).unwrap();
    assert!(engine.pending_module_source().is_some());
    assert_eq!(
        engine.ole32.as_ref().unwrap().activation.pending[0].phase,
        Phase32::Loading
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(engine.ole32_read_word(DATA).unwrap(), 0x12345678);
    assert!(drain(&mut engine, &assets) > 0);
    let callback = engine.pending_windows_callback().unwrap();
    assert_eq!(callback.procedure, GuestAddress(0x501000));
    assert_eq!(callback.count, 3);
    assert_eq!(
        engine.ole32.as_ref().unwrap().activation.pending[0].phase,
        Phase32::Factory
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.ole32_read_word(DATA).unwrap(), 0x12345678);
}

#[test]
fn forwarded_can_unload_source_wait_retains_the_exact_server_lease_and_caller() {
    let assets = assets();
    let mut engine = engine(&assets);
    assert!(matches!(
        engine
            .load_library_module(r"C:\app\server.dll", None)
            .unwrap(),
        ModuleOperation32::NeedBytes { .. }
    ));
    assert!(drain(&mut engine, &assets) > 0);
    let handle = engine.module_handle(Some("server.dll")).unwrap().unwrap();
    let token = push(
        &mut engine,
        Ole32Api32::CoFreeUnusedLibrariesEx,
        &[0, 0],
        Phase32::UnloadNext { after: 0, delay: 0 },
    );
    engine
        .ole32
        .as_mut()
        .unwrap()
        .activation
        .servers
        .push(Server32 {
            id: 1,
            path: r"C:\app\server.dll".into(),
            handle,
            threading: ServerThreadingModel32::Both,
            threads: vec![1],
            mta_lease: false,
            neutral_lease: false,
            unused_deadline: None,
            activity: 7,
        });
    engine.advance_registered_com_loading(0).unwrap();
    assert_eq!(
        engine
            .pending_module_source()
            .unwrap()
            .request
            .unwrap()
            .asset,
        2
    );
    assert!(matches!(
        engine.ole32.as_ref().unwrap().activation.pending[0].phase,
        Phase32::ResolveUnload {
            lease: ServerUse32 {
                server: 1,
                activity: 7,
                ..
            },
            ..
        }
    ));
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert!(drain(&mut engine, &assets) > 0);
    let callback = engine.pending_windows_callback().unwrap();
    assert_eq!(
        (callback.procedure, callback.count),
        (GuestAddress(0x601000), 0)
    );
    assert!(matches!(
        engine.ole32.as_ref().unwrap().activation.pending[0].phase,
        Phase32::CanUnload {
            lease: ServerUse32 {
                server: 1,
                activity: 7,
                ..
            },
            ..
        }
    ));
    assert_eq!(engine.ole32.as_ref().unwrap().activation.servers.len(), 1);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(
        engine.module_handle(Some("server.dll")).unwrap(),
        Some(handle)
    );
}

#[test]
fn bad_lazy_com_image_returns_the_real_hresult_without_publishing_a_factory() {
    let mut assets = assets();
    assets[0] = vec![0; 128];
    let mut engine = engine(&assets);
    engine.write32(DATA, 0x12345678).unwrap();
    push(
        &mut engine,
        Ole32Api32::CoGetClassObject,
        &[DATA + 16, 1, 0, DATA + 32, DATA],
        Phase32::Load,
    );
    engine.advance_registered_com_loading(0).unwrap();
    assert!(drain(&mut engine, &assets) > 0);
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(engine.ole32_read_word(DATA).unwrap(), 0);
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        HResult32::ERROR_IN_DLL.0
    );
    assert!(engine.ole32.as_ref().unwrap().activation.pending.is_empty());
    assert_eq!(engine.module_handle(Some("server.dll")).unwrap(), None);
}

#[test]
fn transient_forwarded_unload_failure_restores_its_retry_point_without_retiring_the_server() {
    let (mut engine, scratch) = pending(Ole32Api32::CoFreeUnusedLibrariesEx);
    let lease = ServerUse32 {
        server: 1,
        handle: 0x500000,
        activity: 7,
    };
    engine
        .ole32
        .as_mut()
        .unwrap()
        .activation
        .servers
        .push(Server32 {
            id: 1,
            path: "native.dll".into(),
            handle: lease.handle,
            threading: ServerThreadingModel32::Both,
            threads: vec![1],
            mta_lease: false,
            neutral_lease: false,
            unused_deadline: None,
            activity: 7,
        });
    engine.ole32.as_mut().unwrap().activation.pending[0].phase =
        Phase32::ResolveUnload { lease, delay: 17 };
    let before = engine.memory().unwrap().transaction_version();
    let error = HostError::File(crate::files::FileError32::Exhausted);
    assert!(
        matches!(engine.finish_registered_com_unload_module(0,Err(error)),Err(actual) if actual == error)
    );
    assert_eq!(
        engine.ole32.as_ref().unwrap().activation.pending[0].phase,
        Phase32::UnloadNext {
            after: 0,
            delay: 17
        }
    );
    assert_eq!(
        engine.ole32.as_ref().unwrap().activation.servers[0].lease(),
        lease
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_ok()
    );
}

fn ready_factory() -> (EngineInstance, u32, u32) {
    let assets = assets();
    let mut engine = engine(&assets);
    assert!(matches!(
        engine
            .load_library_module(r"C:\app\server.dll", None)
            .unwrap(),
        ModuleOperation32::NeedBytes { .. }
    ));
    assert!(drain(&mut engine, &assets) > 0);
    let handle = engine.module_handle(Some("server.dll")).unwrap().unwrap();
    push(
        &mut engine,
        Ole32Api32::CoGetClassObject,
        &[DATA + 16, 1, 0, DATA + 32, DATA],
        Phase32::Resolve { handle },
    );
    engine.advance_registered_com_loading(0).unwrap();
    let scratch = engine.ole32.as_ref().unwrap().activation.pending[0]
        .scratch
        .address
        .0;
    let return_pc = engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == crate::process::windows::threads::INVOCATION_RETURN_ID)
        .unwrap()
        .entry
        .0;
    (engine, scratch, return_pc)
}

#[test]
fn ready_factory_callback_checks_the_original_scratch_before_guest_entry() {
    let (mut valid, _, return_pc) = ready_factory();
    let generation = valid.generation;
    valid
        .begin_windows_callback(
            KEY,
            generation,
            return_pc,
            crate::process::windows::threads::INVOCATION_RETURN_ID,
        )
        .unwrap();
    assert!(valid.callback.is_some());

    let (mut engine, scratch, return_pc) = ready_factory();
    engine.unmap(scratch, 1).unwrap();
    engine.map(scratch, 1, 3).unwrap();
    engine.write32(scratch, 0xdeadbeef).unwrap();
    let callback = engine.pending_windows_callback();
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.begin_windows_callback(
            KEY,
            engine.generation,
            return_pc,
            crate::process::windows::threads::INVOCATION_RETURN_ID
        ),
        Err(HostError::Memory(crate::memory::MemoryError::InvalidRange))
    );
    assert!(engine.callback.is_none());
    assert_eq!(engine.pending_windows_callback(), callback);
    assert_eq!(engine.arena(), arena.as_slice());
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.ole32_read_word(scratch).unwrap(), 0xdeadbeef);
    assert_eq!(
        engine.ole32.as_ref().unwrap().activation.pending[0].phase,
        Phase32::Factory
    );
}

#[test]
fn a_completed_load_does_not_adopt_a_replaced_activation_scratch() {
    let assets = assets();
    let mut engine = engine(&assets);
    push(
        &mut engine,
        Ole32Api32::CoGetClassObject,
        &[DATA + 16, 1, 0, DATA + 32, DATA],
        Phase32::Loading,
    );
    let scratch = engine.ole32.as_ref().unwrap().activation.pending[0]
        .scratch
        .address
        .0;
    engine.unmap(scratch, 1).unwrap();
    engine.map(scratch, 1, 3).unwrap();
    engine.write32(scratch, 0xdeadbeef).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.accept_registered_com_module_result(
            0,
            Ok(ModuleOperation32::Loaded { handle: 0x500000 })
        ),
        Err(HostError::Memory(crate::memory::MemoryError::InvalidRange))
    );
    let runtime = &engine.ole32.as_ref().unwrap().activation;
    assert!(runtime.servers.is_empty());
    assert_eq!(runtime.pending[0].phase, Phase32::Loading);
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.ole32_read_word(scratch).unwrap(), 0xdeadbeef);
}

#[test]
fn final_uninitialize_keeps_a_shared_mta_servers_real_executable_module_alive() {
    let assets = assets();
    let mut engine = engine(&assets);
    let apartments = &mut engine.ole32.as_mut().unwrap().apartments;
    apartments.uninitialize(1);
    apartments.initialize(1, 0, 0).unwrap();
    apartments.initialize(2, 0, 0).unwrap();
    push(
        &mut engine,
        Ole32Api32::CoGetClassObject,
        &[DATA + 16, 1, 0, DATA + 32, DATA],
        Phase32::Load,
    );
    engine.advance_registered_com_loading(0).unwrap();
    assert!(drain(&mut engine, &assets) > 0);
    assert!(engine.pending_windows_callback().is_some());
    let handle = engine.module_handle(Some("server.dll")).unwrap().unwrap();
    let result = engine
        .resume_ole32_callback(1, HResult32::NO_INTERFACE.0)
        .unwrap();
    engine.accept_ole32_loader_outcome(result).unwrap();
    assert!(engine.pending_call.is_none());
    let (_, token, _) = capture(&mut engine, Ole32Api32::CoUninitialize, &[]);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    assert_eq!(
        engine.module_handle(Some("server.dll")).unwrap(),
        Some(handle)
    );
    engine
        .memory()
        .unwrap()
        .check_span(
            GuestAddress(handle + 0x1000),
            1,
            crate::memory::Access::Execute,
        )
        .unwrap();
    let runtime = engine.ole32.as_ref().unwrap();
    assert_eq!(runtime.apartments.initialization_count(1), 0);
    assert_eq!(runtime.apartments.initialization_count(2), 1);
    assert_eq!(runtime.activation.servers.len(), 1);
    assert!(runtime.activation.servers[0].mta_lease);
    assert!(runtime.activation.servers[0].threads.is_empty());
    assert!(runtime.activation.servers[0].closes_with(&runtime.apartments, 2));
}
