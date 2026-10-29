use super::support::{DATA, read, runtime, texture};
use crate::{
    abi::{
        arena::STATE_OFFSET,
        x86::{STATE_SIZE, encode_state_current},
    },
    cpu::x86::State32,
    graphics::d3d8::{
        api::{InterfaceKind, MethodGate},
        device::Device,
        resources::LockFlags,
    },
    memory::{GuestAddress, PageRange, Permissions},
    process::{CallError, EngineInstance, HostError},
};

const PC: u32 = 0x9000;
const PAYLOAD: u32 = DATA + 4096;

fn setup() -> (EngineInstance, GuestAddress) {
    let (context, mut memory, device) = runtime();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(PAYLOAD), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut engine = EngineInstance::new(256, 91).unwrap();
    engine.memory = Some(memory);
    engine.d3d8 = Some(context);
    engine.enable_core_execution().unwrap();
    engine.external_cpu_memory_pinned = true;
    (engine, device)
}

fn prepare(engine: &mut EngineInstance, device: GuestAddress, slot: u8) -> u32 {
    let args: &[u32] = match slot {
        37 => &[device.0, 1, PAYLOAD],
        42 => &[device.0, PAYLOAD],
        46 => &[device.0, 0, 1],
        50 => &[device.0, 7, 1],
        63 => &[device.0, 0, 1, 1],
        70 => &[device.0, 4, 0, 1],
        71 => &[device.0, 4, 0, 3, 0, 1],
        _ => &[device.0, 0, PAYLOAD, 1],
    };
    let memory = engine.memory.as_mut().unwrap();
    memory
        .write(GuestAddress(DATA), &0x9002u32.to_le_bytes())
        .unwrap();
    for (index, &arg) in args.iter().enumerate() {
        memory
            .write(
                GuestAddress(DATA + (index as u32 + 1) * 4),
                &arg.to_le_bytes(),
            )
            .unwrap();
    }
    let mut state = State32::default();
    state.eip = PC;
    state.registers[4] = DATA;
    encode_state_current(
        &state,
        &mut engine.arena.as_mut().get_mut()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    MethodGate {
        interface: InterfaceKind::Device,
        slot,
    }
    .id()
}

#[test]
fn external_direct_gate_d3d8_facade_matches_original_scalar_dispatch() {
    for slot in [37, 42, 46, 50, 63, 61, 83, 85] {
        let (mut direct, device) = setup();
        let (mut original, original_device) = setup();
        assert_eq!(device, original_device);
        for engine in [&mut direct, &mut original] {
            engine
                .configure_user32(
                    Default::default(),
                    PageRange::new(GuestAddress(0x3000_0000), 16).unwrap(),
                )
                .unwrap();
        }
        let id = if matches!(slot, 61 | 83 | 85) {
            let resource = binding_resource(&mut direct, device, slot);
            assert_eq!(
                resource,
                binding_resource(&mut original, original_device, slot)
            );
            prepare_binding(&mut direct, device, slot, resource).0
        } else {
            prepare(&mut direct, device, slot)
        };
        if slot == 37 {
            direct
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(DATA + 8), &2u32.to_le_bytes())
                .unwrap();
        }
        let state = crate::abi::x86::decode_state(&direct.arena()[..STATE_SIZE]).unwrap();
        let frame = crate::windows::CallFrame32::capture(
            direct.memory().unwrap(),
            state,
            crate::windows::CallingConvention32::Stdcall,
            u32::from(MethodGate::from_id(id).unwrap().method().stack_words),
        )
        .unwrap();
        assert!(direct.external_cpu_can_defer_gate(PC, id));
        let (generation, token) = super::transport::capture_gate(
            &mut original,
            MethodGate::from_id(id).unwrap(),
            frame.arguments(),
        );
        original
            .complete_windows_call(91, generation, token)
            .unwrap();
        let expected = crate::abi::x86::decode_state(&original.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0];
        let arena = direct.arena().to_vec();
        let version = direct.memory().unwrap().transaction_version();
        let epoch = direct.core_epoch;
        let call_token = direct.call_token;
        assert_eq!(
            direct
                .execute_external_direct_windows_gate(id, frame)
                .unwrap(),
            expected
        );
        assert_eq!(expected, 0, "valid scalar setter slot {slot}");
        assert_eq!(
            direct
                .d3d8
                .as_ref()
                .unwrap()
                .device(device)
                .unwrap()
                .state()
                .snapshot(),
            original
                .d3d8
                .as_ref()
                .unwrap()
                .device(original_device)
                .unwrap()
                .state()
                .snapshot()
        );
        assert_eq!(direct.arena(), arena);
        assert_eq!(direct.memory().unwrap().transaction_version(), version);
        assert_eq!(direct.core_epoch, epoch);
        assert_eq!(direct.call_token, call_token);
        assert!(direct.pending_call.is_none());
    }
}

fn prepare_draw(slot: u8, fragmented: bool) -> (EngineInstance, GuestAddress, u32) {
    let (mut engine, device) = setup();
    if fragmented {
        let context = engine.d3d8.as_mut().unwrap();
        let description = context.device(device).unwrap().description();
        let mut limits = context.config.device;
        limits.commands.batch_bytes = 128;
        *context.device_mut(device).unwrap() = Device::new(1, description, limits).unwrap();
    }
    let drain = |context: &mut super::super::D3D8Runtime32| {
        while let Some(packet) = context.device_mut(device).unwrap().seal_batch().unwrap() {
            context
                .device_mut(device)
                .unwrap()
                .acknowledge_batch(packet)
                .unwrap();
        }
    };
    drain(engine.d3d8.as_mut().unwrap());
    for binding in [83, 85] {
        let pointer = binding_resource(&mut engine, device, binding);
        drain(engine.d3d8.as_mut().unwrap());
        let (_, args) = prepare_binding(&mut engine, device, binding, pointer);
        let mut context = engine.d3d8.take().unwrap();
        let memory = engine.memory.as_mut().unwrap();
        let surface = context.resource(pointer, device).unwrap();
        let bytes: &[u8] = if binding == 83 {
            &[0x3a; 36]
        } else {
            &[0, 0, 1, 0, 2, 0]
        };
        context
            .lock_buffer_guest(
                memory,
                device,
                surface.resource,
                0,
                bytes.len() as u32,
                LockFlags(0),
                PAYLOAD,
            )
            .unwrap();
        memory
            .write(GuestAddress(read(memory, PAYLOAD)), bytes)
            .unwrap();
        context.unlock_guest(memory, device, surface).unwrap();
        assert_eq!(
            context
                .device(device)
                .unwrap()
                .resources()
                .version(surface.resource),
            Ok(2)
        );
        assert_eq!(
            &context
                .device(device)
                .unwrap()
                .resources()
                .bytes(surface)
                .unwrap()[..bytes.len()],
            bytes
        );
        drain(&mut context);
        context
            .binding_call(memory, device, binding, &args)
            .unwrap();
        engine.d3d8 = Some(context);
    }
    let context = engine.d3d8.as_mut().unwrap();
    context
        .device_mut(device)
        .unwrap()
        .bind_vertex_shader(2)
        .unwrap();
    context.device_mut(device).unwrap().begin_scene().unwrap();
    let id = prepare(&mut engine, device, slot);
    (engine, device, id)
}

#[test]
fn external_cpu_dirty_policy_draws_use_explicit_resource_versions_without_guest_mutation() {
    for slot in [70, 71] {
        let (mut engine, device, id) = prepare_draw(slot, false);
        let before = engine.memory.as_mut().unwrap().external_descriptor();
        let mut ram = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(DATA), &mut ram)
            .unwrap();
        assert!(
            engine.external_cpu_can_defer_gate(PC, id),
            "draw slot {slot}"
        );
        let args = if slot == 70 {
            vec![device.0, 4, 0, 1]
        } else {
            vec![device.0, 4, 0, 3, 0, 1]
        };
        let mut context = engine.d3d8.take().unwrap();
        assert_eq!(
            context
                .draw_call(engine.memory.as_mut().unwrap(), device, slot, &args)
                .unwrap(),
            Some(0)
        );
        let backend = context.device_mut(device).unwrap();
        let target = backend.back_buffer(0).unwrap();
        assert_eq!(backend.resources().version(target.resource), Ok(2));
        backend.seal_batch().unwrap().unwrap();
        let (_, fragments) =
            crate::abi::graphics::d3d8::packet(backend.queue().peek().unwrap().1).unwrap();
        assert_eq!(fragments.filter(|fragment| fragment.opcode == 8).count(), 1);
        engine.d3d8 = Some(context);
        let after = engine.memory.as_mut().unwrap().external_descriptor();
        assert_eq!(
            &after[64..96],
            &before[64..96],
            "identity, content/code and mapping epochs"
        );
        assert_eq!(&after[112..120], &before[112..120]);
        let mut current = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(DATA), &mut current)
            .unwrap();
        assert_eq!(current, ram);
        assert!(
            !engine.external_cpu_can_defer_gate(PC, id),
            "sealed queue restores publication barrier"
        );
    }
}

#[test]
fn external_cpu_dirty_policy_draw_drain_retains_completion_without_replaying_draw() {
    let (mut engine, device, id) = prepare_draw(71, true);
    engine
        .configure_user32(
            Default::default(),
            PageRange::new(GuestAddress(0x3000_0000), 16).unwrap(),
        )
        .unwrap();
    assert!(engine.external_cpu_can_defer_gate(PC, id));
    let (generation, token) = super::transport::capture_gate(
        &mut engine,
        MethodGate {
            interface: InterfaceKind::Device,
            slot: 71,
        },
        &[device.0, 4, 0, 3, 0, 1],
    );
    let before = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.complete_windows_call(91, generation, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(device)
            .unwrap()
            .queue()
            .is_draining()
    );
    assert!(!engine.external_cpu_can_defer_gate(PC, id));
    let target = engine
        .d3d8
        .as_ref()
        .unwrap()
        .device(device)
        .unwrap()
        .back_buffer(0)
        .unwrap();
    let mut draws = 0;
    while let Some(packet) = engine
        .d3d8
        .as_mut()
        .unwrap()
        .device_mut(device)
        .unwrap()
        .seal_batch()
        .unwrap()
    {
        let backend = engine.d3d8.as_ref().unwrap().device(device).unwrap();
        let (_, fragments) =
            crate::abi::graphics::d3d8::packet(backend.queue().peek().unwrap().1).unwrap();
        draws += fragments
            .filter(|fragment| fragment.opcode == 8 && fragment.flags & 1 != 0)
            .count();
        engine
            .d3d8
            .as_mut()
            .unwrap()
            .device_mut(device)
            .unwrap()
            .acknowledge_batch(packet)
            .unwrap();
    }
    engine.complete_windows_call(91, generation, token).unwrap();
    assert_eq!(draws, 1);
    assert_eq!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(device)
            .unwrap()
            .resources()
            .version(target.resource),
        Ok(2)
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(engine.pending_call.is_none());
}

fn binding_resource(engine: &mut EngineInstance, device: GuestAddress, slot: u8) -> GuestAddress {
    let mut context = engine.d3d8.take().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    let resource = if slot == 61 {
        texture(&mut context, memory, device)
    } else {
        let (creation, format) = if slot == 83 { (23, 2) } else { (24, 101) };
        context
            .device_resource_call(
                memory,
                device,
                creation,
                &[device.0, 64, 0, format, 1, DATA],
            )
            .unwrap();
        GuestAddress(read(memory, DATA))
    };
    engine.d3d8 = Some(context);
    resource
}

fn prepare_binding(
    engine: &mut EngineInstance,
    device: GuestAddress,
    slot: u8,
    resource: GuestAddress,
) -> (u32, Vec<u32>) {
    let id = prepare(engine, device, slot);
    let args = match slot {
        61 => vec![device.0, 0, resource.0],
        83 => vec![device.0, 0, resource.0, 12],
        85 => vec![device.0, resource.0, 0],
        _ => unreachable!(),
    };
    for (index, value) in args.iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(DATA + (index as u32 + 1) * 4),
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    (id, args)
}

#[test]
fn external_direct_gate_mask_binding_keeps_legacy_and_unrequested_calls_neutral() {
    for (slot, bit) in [(61, 8), (83, 9), (85, 10)] {
        let mut engine = EngineInstance::new(1024, 91).unwrap();
        engine.register_kernel32_module(0x70000000).unwrap();
        engine.register_d3d8_module(0x72000000).unwrap();
        engine
            .register_guest_module(
                "direct.exe",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/process/images/startup/imports/fixtures/p2-c-imports/program.exe"
                )),
            )
            .unwrap();
        engine
            .start_module_process(
                "direct.exe",
                None,
                crate::process::ModuleRuntimeConfig32 {
                    stack_base: 0x7e100000,
                    stack_pages: 8,
                    runtime_base: 0x7d000000,
                },
            )
            .unwrap();
        engine.load_library_module("d3d8.dll", None).unwrap();
        let (context, mut memory, device) =
            super::support::runtime_in(engine.memory.take().unwrap());
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(PAYLOAD), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        engine.memory = Some(memory);
        engine.d3d8 = Some(context);
        engine
            .configure_user32(
                Default::default(),
                PageRange::new(GuestAddress(0x3000_0000), 16).unwrap(),
            )
            .unwrap();
        engine.enable_core_execution().unwrap();
        engine.external_cpu_memory_pinned = true;
        engine.set_external_cpu_gate_mode(1).unwrap();
        let resource = binding_resource(&mut engine, device, slot);
        for bound in [resource, resource, GuestAddress(0)] {
            let (id, _) = prepare_binding(&mut engine, device, slot, bound);
            let pc = engine
                .module_gates()
                .unwrap()
                .into_iter()
                .find(|gate| gate.id == id)
                .unwrap()
                .entry
                .0;
            let arena = engine.arena().to_vec();
            let version = engine.memory().unwrap().transaction_version();
            let mapped = engine.memory().unwrap().mapped_pages();
            let references = engine.d3d8.as_ref().unwrap().com.state(resource).unwrap();
            assert_eq!(engine.external_cpu_gate_match(pc), Some((id, true)));
            assert!(
                !engine
                    .try_direct_external_cpu_gate(pc, DATA, 0xabcdef01)
                    .unwrap()
            );
            assert_eq!(
                engine.arena(),
                arena,
                "legacy8 must decline the owned binding"
            );
            for mask in [0xff, 0x7ff ^ (1 << bit), 0x800 | (1 << bit)] {
                assert!(
                    !engine
                        .try_direct_external_cpu_gate_mask(pc, DATA, 0xabcdef01, mask)
                        .unwrap()
                );
                assert_eq!(engine.arena(), arena);
                assert_eq!(
                    engine.d3d8.as_ref().unwrap().com.state(resource).unwrap(),
                    references
                );
                assert_eq!(engine.memory().unwrap().transaction_version(), version);
                assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
            }
            assert!(
                engine
                    .try_direct_external_cpu_gate_mask(pc, DATA, 0xabcdef01, 1 << bit)
                    .unwrap()
            );
            let packet = &engine.arena()
                [crate::abi::arena::TRANSFER_OFFSET..crate::abi::arena::TRANSFER_OFFSET + 32];
            crate::abi::header::validate(packet, *b"R3IF", 32).unwrap();
            assert_eq!(crate::abi::header::read_u32(packet, 16), id);
            assert_eq!(crate::abi::header::read_u32(packet, 20), 0);
            assert_eq!(crate::abi::header::read_u32(packet, 24), 0x9002);
            assert_eq!(
                crate::abi::header::read_u32(packet, 28),
                DATA + 4 + u32::from(MethodGate::from_id(id).unwrap().method().stack_words) * 4
            );
            assert_eq!(&engine.arena()[..140], &arena[..140]);
            assert_eq!(&engine.arena()[172..], &arena[172..]);
            assert_eq!(engine.memory().unwrap().transaction_version(), version);
            assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
            assert_eq!(
                engine
                    .d3d8
                    .as_ref()
                    .unwrap()
                    .com
                    .state(resource)
                    .unwrap()
                    .holds,
                u32::from(bound.0 != 0)
            );
        }
    }
}

#[test]
fn external_cpu_dirty_policy_resource_binding_keeps_live_resources_and_ram_stable() {
    for slot in [61, 83, 85] {
        let (mut engine, device) = setup();
        let resource = binding_resource(&mut engine, device, slot);
        for bound in [resource, resource, GuestAddress(0)] {
            let (id, args) = prepare_binding(&mut engine, device, slot, bound);
            let before = engine.d3d8.as_ref().unwrap().com.state(resource).unwrap();
            let version = engine.memory().unwrap().transaction_version();
            let mapped = engine.memory().unwrap().mapped_pages();
            assert!(
                engine.external_cpu_can_defer_gate(PC, id),
                "binding slot {slot}"
            );
            assert_eq!(
                engine.d3d8.as_ref().unwrap().com.state(resource).unwrap(),
                before
            );
            let mut context = engine.d3d8.take().unwrap();
            context
                .binding_call(engine.memory.as_mut().unwrap(), device, slot, &args)
                .unwrap();
            let after = context.com.state(resource).unwrap();
            assert_eq!(after.references, before.references);
            assert_eq!(after.holds, u32::from(bound.0 != 0));
            engine.d3d8 = Some(context);
            assert_eq!(engine.memory().unwrap().transaction_version(), version);
            assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
        }
    }
}

#[test]
fn external_cpu_dirty_policy_resource_binding_rejects_last_hold_disposal() {
    for slot in [61, 83, 85] {
        let (mut engine, device) = setup();
        let resource = binding_resource(&mut engine, device, slot);
        let (_, args) = prepare_binding(&mut engine, device, slot, resource);
        let mut context = engine.d3d8.take().unwrap();
        context
            .binding_call(engine.memory.as_mut().unwrap(), device, slot, &args)
            .unwrap();
        assert_eq!(
            context
                .release(engine.memory.as_mut().unwrap(), resource)
                .unwrap(),
            0
        );
        assert_eq!(context.com.state(resource).unwrap().holds, 1);
        engine.d3d8 = Some(context);
        let (id, args) = prepare_binding(&mut engine, device, slot, GuestAddress(0));
        let version = engine.memory().unwrap().transaction_version();
        let mapped = engine.memory().unwrap().mapped_pages();
        assert!(!engine.external_cpu_can_defer_gate(PC, id));
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
        let mut context = engine.d3d8.take().unwrap();
        context
            .binding_call(engine.memory.as_mut().unwrap(), device, slot, &args)
            .unwrap();
        assert!(context.com.lookup(resource).is_err());
        assert!(
            engine.memory().unwrap().mapped_pages() < mapped,
            "excluded real unbind retires COM mapping"
        );
    }
}

#[test]
fn external_cpu_dirty_policy_resource_binding_rejects_guarded_cow_or_missing_com_neutrally() {
    for change in [0, 1, 2] {
        let (mut engine, device) = setup();
        let resource = binding_resource(&mut engine, device, 61);
        let (id, _) = prepare_binding(&mut engine, device, 61, resource);
        let page = PageRange::new(GuestAddress(resource.0 & !4095), 1).unwrap();
        let memory = engine.memory.as_mut().unwrap();
        if change == 0 {
            memory
                .protect(page, Permissions::READ_WRITE.with_guard())
                .unwrap();
        } else if change == 1 {
            let mut bytes = [0; 4096];
            memory
                .read(GuestAddress(resource.0 & !4095), &mut bytes)
                .unwrap();
            let shared = memory.allocate_shared_pages(1, &bytes).unwrap();
            memory.unmap(page).unwrap();
            memory
                .map_shared_pages(page, &shared, 0, Permissions::READ_WRITE, true)
                .unwrap();
        } else {
            memory.unmap(page).unwrap();
        }
        let version = memory.transaction_version();
        let mapped = memory.mapped_pages();
        memory.begin_guard_service().unwrap();
        assert!(!engine.external_cpu_can_defer_gate(PC, id));
        let memory = engine.memory().unwrap();
        assert_eq!(memory.transaction_version(), version);
        assert_eq!(memory.mapped_pages(), mapped);
        assert!(memory.end_guard_service().is_none());
    }
    let (mut engine, device) = setup();
    let resource = binding_resource(&mut engine, device, 61);
    let (_, args) = prepare_binding(&mut engine, device, 61, resource);
    let mut context = engine.d3d8.take().unwrap();
    context
        .binding_call(engine.memory.as_mut().unwrap(), device, 61, &args)
        .unwrap();
    engine.d3d8 = Some(context);
    let (id, _) = prepare_binding(&mut engine, device, 61, GuestAddress(0));
    let memory = engine.memory.as_mut().unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(resource.0 & !4095), 1).unwrap(),
            Permissions::READ_WRITE.with_guard(),
        )
        .unwrap();
    let version = memory.transaction_version();
    memory.begin_guard_service().unwrap();
    assert!(
        !engine.external_cpu_can_defer_gate(PC, id),
        "unbind preflights the released resource even with a null new pointer"
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert!(engine.memory().unwrap().end_guard_service().is_none());
}

#[test]
fn external_cpu_dirty_policy_allows_only_audited_scalar_setters() {
    let (mut engine, device) = setup();
    for slot in [37, 42, 46, 50, 63] {
        let id = prepare(&mut engine, device, slot);
        let version = engine.memory().unwrap().transaction_version();
        assert!(
            engine.external_cpu_can_defer_gate(PC, id),
            "audited slot {slot}"
        );
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert!(!engine.external_cpu_can_defer_gate(PC + 1, id));
    }
    for slot in [38, 51, 61, 62, 72, 73, 83] {
        let id = prepare(&mut engine, device, slot);
        assert!(
            !engine.external_cpu_can_defer_gate(PC, id),
            "unsafe or unaudited slot {slot}"
        );
    }
}

#[test]
fn external_cpu_dirty_policy_rejects_guarded_spans_and_stale_objects() {
    let (mut engine, device) = setup();
    let id = prepare(&mut engine, device, 37);
    let payload = PageRange::new(GuestAddress(PAYLOAD), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(payload, Permissions::READ.with_guard())
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(!engine.external_cpu_can_defer_gate(PC, id));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(payload, Permissions::READ_WRITE)
        .unwrap();
    assert!(engine.external_cpu_can_defer_gate(PC, id));
    let object = PageRange::new(GuestAddress(device.0 & !4095), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(object, Permissions::READ_WRITE)
        .unwrap();
    assert!(
        !engine.external_cpu_can_defer_gate(PC, id),
        "same rights with a new object mapping incarnation is stale"
    );
}

#[test]
fn external_cpu_dirty_policy_rejects_deferred_releases_and_sealed_queues() {
    let (mut engine, device) = setup();
    let id = prepare(&mut engine, device, 50);
    engine
        .d3d8
        .as_mut()
        .unwrap()
        .deferred_private_releases
        .push(device);
    assert!(!engine.external_cpu_can_defer_gate(PC, id));
    engine
        .d3d8
        .as_mut()
        .unwrap()
        .deferred_private_releases
        .clear();
    let mut context = engine.d3d8.take().unwrap();
    texture(&mut context, engine.memory.as_mut().unwrap(), device);
    let packet = context
        .device_mut(device)
        .unwrap()
        .seal_batch()
        .unwrap()
        .unwrap();
    engine.d3d8 = Some(context);
    assert!(!engine.external_cpu_can_defer_gate(PC, id));
    engine
        .d3d8
        .as_mut()
        .unwrap()
        .device_mut(device)
        .unwrap()
        .acknowledge_batch(packet)
        .unwrap();
    assert!(engine.external_cpu_can_defer_gate(PC, id));
}

#[test]
fn external_cpu_dirty_policy_get_last_error_reads_teb_without_mutation() {
    use crate::{abi::x86::decode_state, windows::WindowsApi32};
    let mut engine = EngineInstance::new(128, 91).unwrap();
    let image = crate::process::windows::process::test_image_for_startup();
    engine
        .register_guest_module("last-error.exe", &image)
        .unwrap();
    engine
        .start_module_process("last-error.exe", None, Default::default())
        .unwrap();
    engine.enable_core_execution().unwrap();
    engine.external_cpu_memory_pinned = true;
    let teb = engine.module_teb_address().unwrap();
    let mut state = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    state.eip = PC;
    state.registers[4] = DATA;
    let memory = engine.memory.as_mut().unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(DATA), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(state.registers[4]), &0x9002u32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(teb + 0x34), &0xf1234567u32.to_le_bytes())
        .unwrap();
    encode_state_current(
        &state,
        &mut engine.arena.as_mut().get_mut()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    let page = PageRange::new(GuestAddress(teb & !4095), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::READ)
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(engine.external_cpu_can_defer_gate(PC, WindowsApi32::GetLastError.id()));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    let mut value = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(teb + 0x34), &mut value)
        .unwrap();
    assert_eq!(u32::from_le_bytes(value), 0xf1234567);
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::READ.with_guard())
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(!engine.external_cpu_can_defer_gate(PC, WindowsApi32::GetLastError.id()));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(
        engine.memory().unwrap().permissions_at(GuestAddress(teb)),
        Ok(Permissions::READ.with_guard())
    );
}

fn last_error_setter() -> (EngineInstance, u32, u32) {
    use crate::{
        abi::{
            arena::EXIT_OFFSET,
            x86::{EXIT_SIZE, decode_state, encode_exit_v15},
        },
        cpu::{ExecutionExit, ExitReason},
        windows::WindowsApi32,
    };
    let mut engine = EngineInstance::new(128, 91).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .register_guest_module(
            "last-error.exe",
            include_bytes!("../../../../../tests/process/images/startup/imports/fixtures/p2-c-imports/program.exe"),
        )
        .unwrap();
    engine
        .start_module_process("last-error.exe", None, Default::default())
        .unwrap();
    engine.enable_core_execution().unwrap();
    engine.external_cpu_memory_pinned = true;
    let teb = engine.module_teb_address().unwrap();
    let pc = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id == WindowsApi32::SetLastError.id())
        .unwrap()
        .entry
        .0;
    let mut state = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    state.eip = pc;
    state.registers[4] = DATA;
    let memory = engine.memory.as_mut().unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(DATA), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(DATA), &0x9002u32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(DATA + 4), &9u32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(teb + 0x34), &7u32.to_le_bytes())
        .unwrap();
    let arena = engine.arena.as_mut().get_mut();
    encode_state_current(&state, &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_exit_v15(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: WindowsApi32::SetLastError.id(),
            },
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    (engine, teb, pc)
}

#[test]
fn external_cpu_dirty_policy_set_last_error_preserves_real_write_and_versions() {
    use crate::windows::WindowsApi32;
    let (mut engine, teb, pc) = last_error_setter();
    let address = GuestAddress(teb + 0x34);
    let memory = engine.memory().unwrap();
    let version = memory.transaction_version();
    let mapping = memory.mapping_version_at(address).unwrap();
    let code = memory.code_validation_version();
    assert!(engine.external_cpu_can_defer_gate(pc, WindowsApi32::SetLastError.id()));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    let token = engine.capture_core_windows_call().unwrap().token;
    engine.complete_core_windows_call(token).unwrap();
    let memory = engine.memory().unwrap();
    let mut value = [0; 4];
    memory.read(address, &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), 9);
    assert!(memory.transaction_version() > version);
    assert_eq!(memory.mapping_version_at(address).unwrap(), mapping);
    assert_eq!(memory.code_validation_version(), code);
}

#[test]
fn external_cpu_dirty_policy_set_last_error_rejects_cow_and_executable_aliases() {
    use crate::windows::WindowsApi32;
    let (mut engine, teb, pc) = last_error_setter();
    let page = PageRange::new(GuestAddress(teb & !4095), 1).unwrap();
    let memory = engine.memory.as_mut().unwrap();
    memory.protect(page, Permissions::READ).unwrap();
    assert!(!engine.external_cpu_can_defer_gate(pc, WindowsApi32::SetLastError.id()));
    let memory = engine.memory.as_mut().unwrap();
    memory
        .protect(page, Permissions::READ_WRITE.with_guard())
        .unwrap();
    let version = memory.transaction_version();
    assert!(!engine.external_cpu_can_defer_gate(pc, WindowsApi32::SetLastError.id()));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    let memory = engine.memory.as_mut().unwrap();
    memory.protect(page, Permissions::READ_WRITE).unwrap();
    let mut bytes = [0; 4096];
    memory.read(GuestAddress(teb & !4095), &mut bytes).unwrap();
    let shared = memory.allocate_shared_pages(1, &bytes).unwrap();
    memory.unmap(page).unwrap();
    memory
        .map_shared_pages(page, &shared, 0, Permissions::READ_WRITE, true)
        .unwrap();
    let mapping = memory.mapping_version_at(GuestAddress(teb)).unwrap();
    let version = memory.transaction_version();
    assert!(!engine.external_cpu_can_defer_gate(pc, WindowsApi32::SetLastError.id()));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(
        engine
            .memory()
            .unwrap()
            .mapping_version_at(GuestAddress(teb))
            .unwrap(),
        mapping
    );
    let memory = engine.memory.as_mut().unwrap();
    memory.unmap(page).unwrap();
    memory
        .map_shared_pages(page, &shared, 0, Permissions::READ_WRITE, false)
        .unwrap();
    let alias = PageRange::new(GuestAddress(0x30000000), 1).unwrap();
    memory
        .map_shared_pages(alias, &shared, 0, Permissions::READ_EXECUTE, false)
        .unwrap();
    assert!(!engine.external_cpu_can_defer_gate(pc, WindowsApi32::SetLastError.id()));
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA + 4), &7u32.to_le_bytes())
        .unwrap();
    assert!(
        engine.external_cpu_can_defer_gate(pc, WindowsApi32::SetLastError.id()),
        "equal value never writes through the executable alias"
    );
    let version = engine.memory().unwrap().transaction_version();
    let token = engine.capture_core_windows_call().unwrap().token;
    engine.complete_core_windows_call(token).unwrap();
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}
