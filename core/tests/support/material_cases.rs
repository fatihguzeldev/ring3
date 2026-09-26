use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const DEVICE: u32 = 0x7000_1004;
const MATERIAL: u32 = 0x0040_2f00;
const STACK: u32 = 0x1000_fe00;
const INVALID_CALL: u32 = 0x8876_086c;

fn read(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    process
        .memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    for (index, value) in args.iter().enumerate() {
        process
            .memory
            .write(
                u64::from(STACK) + 4 + index as u64 * 4,
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(process, api, args);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
    value
}

fn created() -> Process32 {
    let mut process =
        Process32::load(&super::d3d8_executable::pe32(4, 3, 0xff12_3456), 64).unwrap();
    assert_eq!(
        process.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    process
}

pub fn material_state_accepts_a_full_readable_structure() {
    let mut process = created();
    let method = read(&process, read(&process, DEVICE) + 42 * 4);
    assert_eq!(method, 0x7000_05e4);
    let fields: Vec<_> = (0_u16..17)
        .flat_map(|index| (f32::from(index) / 16.0).to_le_bytes())
        .collect();
    process.memory.write(u64::from(MATERIAL), &fields).unwrap();
    assert_eq!(call(&mut process, method, &[DEVICE, MATERIAL]), 0);
    assert_eq!(call(&mut process, method, &[DEVICE, 0]), INVALID_CALL);
    assert_eq!(
        call(&mut process, method, &[DEVICE + 1, MATERIAL]),
        INVALID_CALL
    );
    process
        .memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut process, method, &[DEVICE, MATERIAL]), 0);
    process
        .memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let release = read(&process, read(&process, DEVICE) + 2 * 4);
    assert_eq!(call(&mut process, release, &[DEVICE]), 0);
    assert_eq!(
        call(&mut process, method, &[DEVICE, MATERIAL]),
        INVALID_CALL
    );
}

pub fn material_read_fault_does_not_complete_the_call() {
    let mut process = created();
    let method = read(&process, read(&process, DEVICE) + 42 * 4);
    process
        .memory
        .map_zeroed(0x5000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let before = prepare(&mut process, method, &[DEVICE, 0x5000_0ff0]);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    assert_eq!(call(&mut process, method, &[DEVICE, MATERIAL]), 0);
}
