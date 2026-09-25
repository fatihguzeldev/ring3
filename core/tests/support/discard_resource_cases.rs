use ring3_core::execution::{Cpu32, Process32, ProcessStop, Register32, StopReason};

const STACK: u32 = 0x1000_fe00;
const DEVICE: u32 = 0x7000_1004;
const OUTPUT: u32 = 0x0040_2a00;
const LOCK: u32 = OUTPUT + 16;

fn read(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        p.memory
            .write(u64::from(address) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn method(p: &Process32, object: u32, slot: u32) -> u32 {
    read(p, read(p, object) + slot * 4)
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    words(p, STACK, &[0x0040_1000]);
    words(p, STACK + 4, args);
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
    value
}

fn created() -> Process32 {
    let mut p = Process32::load(&super::d3d8_executable::pe32(4, 3, 0xff12_3456), 64).unwrap();
    assert_eq!(
        p.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    p
}

pub fn imported_discard_uses_device_vtable() {
    let mut bytes = super::d3d8_executable::pe32(4, 3, 0xff12_3456);
    let halt = 0x200
        + bytes[0x200..0x400]
            .iter()
            .position(|byte| *byte == 0xcc)
            .unwrap();
    bytes[halt..halt + 8].copy_from_slice(&[0x8b, 0x06, 0x6a, 0, 0x56, 0xff, 0x50, 0x14]);
    bytes[halt + 8] = 0xcc;
    let mut previous = None;
    for budget in [1, 200] {
        let mut p = Process32::load(&bytes, 64).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 200);
        }
        assert_eq!(counts.1, 13);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        if let Some(previous) = previous {
            assert_eq!((p.cpu, counts), previous);
        }
        previous = Some((p.cpu, counts));
    }
}

pub fn discard_preserves_resources_scene_and_frame() {
    for pool in [0, 1, 2] {
        let mut p = created();
        let discard = method(&p, DEVICE, 5);
        assert_eq!(read(&p, read(&p, DEVICE) + 6 * 4), 0x7000_0ffc);
        call(&mut p, 0x7000_0000, &[77]);
        let create = method(&p, DEVICE, 20);
        assert_eq!(
            call(&mut p, create, &[DEVICE, 4, 4, 1, 0, 21, pool, OUTPUT]),
            0
        );
        let texture = read(&p, OUTPUT);
        let lock = method(&p, texture, 16);
        assert_eq!(call(&mut p, lock, &[texture, 0, LOCK, 0, 0]), 0);
        let pixels = read(&p, LOCK + 4);
        words(&mut p, pixels, &[0xff12_3456; 16]);
        let bind = method(&p, DEVICE, 61);
        assert_eq!(call(&mut p, bind, &[DEVICE, 0, texture]), 0);
        let begin = method(&p, DEVICE, 34);
        assert_eq!(call(&mut p, begin, &[DEVICE]), 0);
        let available = method(&p, DEVICE, 4);
        let free = call(&mut p, available, &[DEVICE]);
        let pages = p.memory.mapped_pages();
        for _ in 0..2 {
            assert_eq!(call(&mut p, discard, &[DEVICE, 0]), 0);
        }
        assert_eq!(call(&mut p, available, &[DEVICE]), free);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(p.last_error().unwrap(), 77);
        for offset in 0..16 {
            assert_eq!(read(&p, pixels + offset * 4), 0xff12_3456);
        }
        let frame = p.take_frame().unwrap();
        assert_eq!((frame.width, frame.height), (4, 3));
        assert!(
            frame
                .rgba
                .chunks_exact(4)
                .all(|pixel| pixel == [0x12, 0x34, 0x56, 255])
        );
        let unlock = method(&p, texture, 17);
        assert_eq!(call(&mut p, unlock, &[texture, 0]), 0);
        let end = method(&p, DEVICE, 35);
        assert_eq!(call(&mut p, end, &[DEVICE]), 0);
        let release = method(&p, texture, 2);
        assert_eq!(call(&mut p, release, &[texture]), 1);
        assert_eq!(call(&mut p, bind, &[DEVICE, 0, 0]), 0);
        let release = method(&p, DEVICE, 2);
        assert_eq!(call(&mut p, release, &[DEVICE]), 0);
        assert_eq!(call(&mut p, discard, &[DEVICE, 0]), 0x8876_086c);
    }
}

pub fn discard_rejections_preserve_state() {
    let mut p = created();
    let discard = method(&p, DEVICE, 5);
    for receiver in [0, 0x7000_1000, DEVICE + 1, 0x5000_0000] {
        assert_eq!(call(&mut p, discard, &[receiver, 0]), 0x8876_086c);
    }
    let pages = p.memory.mapped_pages();
    for receiver in [DEVICE, 0] {
        for count in [1, 4096, u32::MAX] {
            let before = prepare(&mut p, discard, &[receiver, count]);
            let result = p.run(1);
            assert_eq!(
                result.reason,
                ProcessStop::UnsupportedApi { address: discard }
            );
            assert_eq!((result.instructions, result.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
        }
    }
    prepare(&mut p, discard, &[DEVICE, 0]);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(call(&mut p, discard, &[DEVICE, 0]), 0);
    assert!(p.take_frame().is_some());
}
