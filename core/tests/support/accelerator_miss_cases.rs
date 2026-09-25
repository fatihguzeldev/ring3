use ring3_core::execution::{
    PAGE_SIZE, Permissions, PostedMessage, Process32, ProcessStop, Register32, StopReason,
};

const WINDOW: u32 = 0x7500_0004;
const STACK: u32 = 0x1000_fe00;
const MESSAGE: u32 = 0x0040_2601;

fn write(process: &mut Process32, address: u32, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    process.memory.write(u64::from(address), &bytes).unwrap();
}

fn read(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) {
    let mut frame = vec![0x0040_10f0];
    frame.extend_from_slice(args);
    write(process, STACK, &frame);
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(process, api, args);
    let mut expected = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    expected.eip = 0x0040_10f0;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(process.cpu, expected);
    value
}

fn created() -> (Process32, u32, u32) {
    let mut bytes = super::accelerator_executable::pe32(&[
        [1, 65, 10, 0],
        [0x10, 66, 20, 0],
        [0x80, 67, 30, 0],
    ]);
    let window = super::window_creation_executable::guest();
    bytes[0x200..0x600].copy_from_slice(&window[0x200..0x600]);
    assert_eq!(bytes[0x24f], 0x50);
    bytes[0x24f] = 0xcc;
    bytes[0x552..0x580].fill(0);
    bytes[0x552..0x568].copy_from_slice(b"TranslateAcceleratorA\0");
    let mut code = Vec::new();
    for value in [MESSAGE, 0x7700_0004, WINDOW] {
        code.push(0x68);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[0xff, 0x15, 0x6c, 0x20, 0x40, 0, 0xcc]);
    bytes[0x380..0x380 + code.len()].copy_from_slice(&code);
    let mut process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        process.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Ebx), WINDOW);
    let table = call(&mut process, 0x7000_029c, &[0, 1]);
    let translate = read(&process, 0x0040_206c);
    (process, table, translate)
}

pub fn distinct_keys_are_definite_misses() {
    let (mut process, table, translate) = created();
    for message in [0x100, 0x101, 0x104, 0x105] {
        for (key, flags) in [
            (0, 0),
            (40, 0xc150_0001),
            (40, 0xe150_0001),
            (255, u32::MAX),
        ] {
            let words = [0, message, key, flags, 123, 17, 19, 0];
            write(&mut process, MESSAGE, &words);
            assert_eq!(call(&mut process, translate, &[WINDOW, table, MESSAGE]), 0);
            for (index, expected) in words.into_iter().enumerate() {
                assert_eq!(
                    read(&process, MESSAGE + u32::try_from(index).unwrap() * 4),
                    expected
                );
            }
        }
    }
    let mut expected = None;
    for budget in [1, 100] {
        let (mut process, _, _) = created();
        write(&mut process, MESSAGE, &[0, 0x100, 40, 0, 0, 0, 0, 0]);
        process.cpu.eip = 0x0040_1180;
        process.cpu.set_register(Register32::Esp, STACK);
        let mut counts = (0, 0);
        loop {
            let result = process.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            assert!(counts.0 + counts.1 < 20);
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
        }
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.cpu.register(Register32::Esp), STACK);
        assert_eq!(counts.1, 1);
        if let Some(prior) = expected {
            assert_eq!((process.cpu, counts), prior);
        }
        expected = Some((process.cpu, counts));
    }
}

fn refused(process: &mut Process32, api: u32, args: &[u32], memory_fault: bool) {
    prepare(process, api, args);
    check_refusal(process, api, memory_fault);
}

fn check_refusal(process: &mut Process32, api: u32, memory_fault: bool) {
    let before = process.cpu;
    let result = process.run(1);
    if memory_fault {
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    } else {
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
    }
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
}

pub fn candidates_and_unsupported_messages_remain_unhandled() {
    let (mut process, table, translate) = created();
    for message in [0x100, 0x101, 0x104, 0x105] {
        for key in [65, 66, 67] {
            for flags in [0, 0x2000_0000, 0x0100_0000] {
                write(
                    &mut process,
                    MESSAGE,
                    &[WINDOW, message, key, flags, 0, 0, 0, 0],
                );
                refused(&mut process, translate, &[WINDOW, table, MESSAGE], false);
            }
        }
    }
    for (message, key) in [
        (0x102, 40),
        (0x106, 40),
        (0x200, 0),
        (0x100, 256),
        (0x101, 256),
        (0x105, u32::MAX),
        (0x104, u32::MAX),
    ] {
        write(
            &mut process,
            MESSAGE,
            &[WINDOW, message, key, 0, 0, 0, 0, 0],
        );
        refused(&mut process, translate, &[WINDOW, table, MESSAGE], false);
    }
    write(&mut process, MESSAGE, &[WINDOW, 0x100, 40, 0, 0, 0, 0, 0]);
    assert_eq!(call(&mut process, translate, &[WINDOW, table, MESSAGE]), 0);
}

pub fn misses_preserve_owned_data_and_queued_messages() {
    for message in [0x104, 0x101, 0x105] {
        preserved_miss(message);
    }
}

fn preserved_miss(kind: u32) {
    let (mut process, table, translate) = created();
    let address = 0x5000_0001;
    process
        .memory
        .map_zeroed(0x5000_0000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let message = [0, kind, 40, 0xc150_0001, 77, 1, 2, 3];
    write(&mut process, address, &message);
    write(&mut process, 0x7ffd_e034, &[77]);
    write(&mut process, 0x7000_2020, &[88]);
    process
        .post_message(PostedMessage {
            hwnd: WINDOW,
            message: 0x400,
            wparam: 42,
            lparam: 99,
            time: 123,
            point: [5, 6],
        })
        .unwrap();
    let windows = process.window_snapshots();
    // the cached table must not read its resource payload again.
    for page in [0x0040_2000, 0x5000_0000, 0x7ffd_e000, 0x7000_2000] {
        process
            .memory
            .protect(
                page,
                PAGE_SIZE,
                if page == 0x0040_2000 {
                    Permissions::NONE
                } else {
                    Permissions::READ
                },
            )
            .unwrap();
    }
    assert_eq!(call(&mut process, translate, &[WINDOW, table, address]), 0);
    for (index, expected) in message.into_iter().enumerate() {
        assert_eq!(
            read(&process, address + u32::try_from(index).unwrap() * 4),
            expected
        );
    }
    assert_eq!(read(&process, 0x7ffd_e034), 77);
    assert_eq!(read(&process, 0x7000_2020), 88);
    assert_eq!(process.window_snapshots(), windows);
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut process, 0x7000_043c, &[MESSAGE, 0, 0, 0, 1]), 1);
    for (index, expected) in [WINDOW, 0x400, 42, 99, 123, 5, 6, 0]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            read(&process, MESSAGE + u32::try_from(index).unwrap() * 4),
            expected
        );
    }
}

pub fn invalid_targets_and_memory_are_atomic() {
    let (mut process, table, translate) = created();
    write(&mut process, MESSAGE, &[WINDOW, 0x100, 40, 0, 0, 0, 0, 0]);
    for target in [0, WINDOW + 4, 0x7500_0000, u32::MAX] {
        refused(&mut process, translate, &[target, table, MESSAGE], false);
    }
    for invalid in [0, table + 4, u32::MAX] {
        refused(&mut process, translate, &[WINDOW, invalid, MESSAGE], false);
    }
    process
        .memory
        .map_zeroed(0x5000_0000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for address in [0, 0x6000_0000, 0x5000_0ff0, 0xffff_fff0] {
        refused(&mut process, translate, &[WINDOW, table, address], true);
    }
    for fs in [0, 0x1101_0000] {
        process.cpu.set_fs_base(fs);
        process.cpu.eip = translate;
        process.cpu.set_register(Register32::Esp, 0x6000_0000);
        check_refusal(&mut process, translate, false);
    }
    process.cpu.set_fs_base(0x7ffd_e000);
    for stack in [0x1000_fff8, 0xffff_fff0] {
        if stack == 0xffff_fff0 {
            write(&mut process, stack, &[0x0040_10f0, WINDOW, table, MESSAGE]);
        }
        process.cpu.eip = translate;
        process.cpu.set_register(Register32::Esp, stack);
        check_refusal(&mut process, translate, true);
    }
    assert_eq!(call(&mut process, translate, &[WINDOW, table, MESSAGE]), 0);
}
