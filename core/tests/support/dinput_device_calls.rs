pub use super::dinput_device_cases::{DATA, DEVICE, KEYBOARD};
use super::imported_executable;
pub use ring3_core::execution::{
    Access, Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

pub const ROOT_CREATE: u32 = 0x7000_0558;
pub const ROOT_QUERY: u32 = 0x7000_055c;
pub const ROOT_RELEASE: u32 = 0x7000_0564;
pub const CREATE: u32 = 0x7000_0568;
pub const QUERY: u32 = 0x7000_056c;
pub const ADD: u32 = 0x7000_0570;
pub const RELEASE: u32 = 0x7000_0574;
pub const PAGE: u32 = 0x7001_7000;
pub const STACK: u32 = 0x1000_ef00;
pub const CODE: u32 = 0x0040_1000;
pub const GUID: u32 = DATA + 64;
pub const IID: u32 = DATA + 80;

pub fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "dinput.dll", &["DirectInputCreateA"]),
        96,
    )
    .unwrap()
}

pub fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

pub fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(CODE)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    p.memory.write(u64::from(STACK), &frame).unwrap();
    p.cpu
}

pub fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let cleanup = if api == 0x7000_0548 {
        4
    } else {
        (args.len() + 1) * 4
    };
    assert_eq!(
        p.cpu.register(Register32::Esp),
        STACK + u32::try_from(cleanup).unwrap()
    );
    p.cpu.register(Register32::Eax)
}

pub fn refused(p: &mut Process32, expected: &ProcessStop) {
    let before = p.cpu;
    for _ in 0..2 {
        let run = p.run(1);
        assert_eq!(&run.reason, expected);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

pub fn denied(p: &mut Process32, api: u32, args: &[u32]) {
    prepare(p, api, args);
    refused(p, &ProcessStop::UnsupportedApi { address: api });
}

pub fn fault(p: &mut Process32) {
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

pub fn root(p: &mut Process32) -> u32 {
    assert_eq!(call(p, ROOT_CREATE, &[0x0040_0000, 0x700, DATA, 0]), 0);
    word(p, DATA)
}

pub fn create(p: &mut Process32, root: u32) -> u32 {
    p.memory.write(u64::from(GUID), &KEYBOARD).unwrap();
    assert_eq!(call(p, CREATE, &[root, GUID, DATA + 5, 0]), 0);
    word(p, DATA + 5)
}
