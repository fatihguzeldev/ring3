use super::support::{DATA, runtime, texture};
use crate::{
    abi::{
        arena::STATE_OFFSET,
        x86::{STATE_SIZE, encode_state_current},
    },
    cpu::x86::State32,
    graphics::d3d8::api::{InterfaceKind, MethodGate},
    memory::{GuestAddress, PageRange, Permissions},
    process::EngineInstance,
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
    for slot in [38, 51, 61, 62, 71, 83, 85] {
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
    engine.register_guest_module("last-error.exe", &image).unwrap();
    engine.start_module_process("last-error.exe", None, Default::default()).unwrap();
    engine.enable_core_execution().unwrap();
    engine.external_cpu_memory_pinned = true;
    let teb = engine.module_teb_address().unwrap();
    let mut state = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    state.eip = PC;
    state.registers[4] = DATA;
    let memory = engine.memory.as_mut().unwrap();
    memory.map_zeroed(PageRange::new(GuestAddress(DATA), 1).unwrap(), Permissions::READ_WRITE).unwrap();
    memory.write(GuestAddress(state.registers[4]), &0x9002u32.to_le_bytes()).unwrap();
    memory.write(GuestAddress(teb + 0x34), &0xf1234567u32.to_le_bytes()).unwrap();
    encode_state_current(&state, &mut engine.arena.as_mut().get_mut()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    let page = PageRange::new(GuestAddress(teb & !4095), 1).unwrap();
    engine.memory.as_mut().unwrap().protect(page, Permissions::READ).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(engine.external_cpu_can_defer_gate(PC, WindowsApi32::GetLastError.id()));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    let mut value = [0; 4];
    engine.memory().unwrap().read(GuestAddress(teb + 0x34), &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), 0xf1234567);
    engine.memory.as_mut().unwrap().protect(page, Permissions::READ.with_guard()).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(!engine.external_cpu_can_defer_gate(PC, WindowsApi32::GetLastError.id()));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.memory().unwrap().permissions_at(GuestAddress(teb)), Ok(Permissions::READ.with_guard()));
}
