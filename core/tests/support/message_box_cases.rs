use ring3_core::execution::{
    Cpu32, MessageBoxResponseError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_05e0;
const STACK: u32 = 0x1000_ff00;
const TEXT: u32 = 0x0040_2180;
const CAPTION: u32 = 0x0040_21a0;

fn executable() -> Vec<u8> {
    let mut bytes = super::imported_executable::pe32(
        &[
            0x6a, 0, 0x68, 0xa0, 0x21, 0x40, 0, 0x68, 0x80, 0x21, 0x40, 0, 0x6a, 0, 0xff, 0x15,
            0x60, 0x20, 0x40, 0, 0xcc,
        ],
        "USER32.dll",
        &["MessageBoxA"],
    );
    bytes[0x580..0x588].copy_from_slice(b"message\0");
    bytes[0x5a0..0x5a8].copy_from_slice(b"caption\0");
    bytes
}

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn prepare(p: &mut Process32, args: [u32; 4]) -> Cpu32 {
    words(p, STACK, &[0x0040_1014]);
    words(p, STACK + 4, &args);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 77);
    p.cpu.eflags = 0xced7;
    p.cpu
}

fn wait(p: &mut Process32, before: Cpu32) -> u64 {
    let result = p.run(1);
    assert_eq!(result.reason, ProcessStop::MessageBoxRequired);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.pending_message_box().unwrap().id
}

fn finish(p: &mut Process32, id: u64, mut expected: Cpu32) {
    p.acknowledge_message_box(id).unwrap();
    assert_eq!(p.cpu, expected);
    assert!(p.pending_message_box().is_none());
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, expected);
    assert_eq!(
        p.acknowledge_message_box(id),
        Err(MessageBoxResponseError::AlreadyAcknowledged)
    );
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x0040_1014;
    expected.set_register(Register32::Esp, expected.register(Register32::Esp) + 20);
    expected.set_register(Register32::Eax, 1);
    assert_eq!(p.cpu, expected);
    assert_eq!(
        p.acknowledge_message_box(id),
        Err(MessageBoxResponseError::NoRequest)
    );
}

pub fn imported_message_box_waits_for_acknowledgement() {
    for budget in [1, 40] {
        let mut p = Process32::load(&executable(), 64).unwrap();
        assert_eq!(
            p.acknowledge_message_box(1),
            Err(MessageBoxResponseError::NoRequest)
        );
        let mut counts = (0, 0);
        for _ in 0..20 {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason == ProcessStop::MessageBoxRequired {
                break;
            }
            assert_eq!(
                result.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
        }
        assert_eq!(counts, (5, 0));
        let before = p.cpu;
        let request = p.pending_message_box().unwrap();
        assert_eq!(
            (request.owner, request.text, request.caption),
            (0, b"message".as_slice(), b"caption".as_slice())
        );
        let id = request.id;
        for budget in [0, 1, 10000] {
            let result = p.run(budget);
            assert_eq!((result.instructions, result.api_calls), (0, 0));
            assert_eq!(p.cpu, before);
            if budget != 0 {
                assert_eq!(result.reason, ProcessStop::MessageBoxRequired);
            }
        }
        assert_eq!(
            p.acknowledge_message_box(id + 1),
            Err(MessageBoxResponseError::StaleRequest)
        );
        finish(&mut p, id, before);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        let before = prepare(&mut p, [0, 0, 0, 0]);
        let second = wait(&mut p, before);
        assert_eq!(second, id + 1);
        assert_eq!(
            p.acknowledge_message_box(id),
            Err(MessageBoxResponseError::StaleRequest)
        );
        assert_eq!(p.pending_message_box().unwrap().text, b"");
        assert_eq!(p.pending_message_box().unwrap().caption, b"Error");
        finish(&mut p, second, before);
    }
}

fn create_child(p: &mut Process32) -> u32 {
    let template = [
        1_u16, 0xffff, 0, 0, 0, 0, 0, 0x80c0, 1, 0, 0, 100, 50, 0, 0, 84, 0, 0, 0, 0, 0, 0, 0,
        0x5000, 4, 5, 30, 12, 42, 0, 0xffff, 0x80, 79, 75, 0, 0,
    ];
    let bytes: Vec<_> = template
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(0x0040_2400, &bytes).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .write(0x0040_1200, &[0x31, 0xc0, 0xc2, 16, 0, 0xcc])
        .unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    words(
        p,
        STACK,
        &[0x0040_1205, 0x0040_0000, 0x0040_2400, 0, 0x0040_1200, 0],
    );
    p.cpu.eip = 0x7000_0434;
    p.cpu.set_register(Register32::Esp, STACK);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    p.window_snapshots()
        .into_iter()
        .find(|window| window.parent != 0)
        .unwrap()
        .hwnd
}

pub fn message_snapshots_preserve_bytes_owner_and_error_state() {
    let mut p = Process32::load(&super::window_creation_executable::guest(), 64).unwrap();
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let windows = p.window_snapshots();
    let owner = windows[0].hwnd;
    words(&mut p, 0x7ffd_e034, &[91]);
    p.memory.write(u64::from(TEXT), b"a\r\n\x80\xff\0").unwrap();
    let before = prepare(&mut p, [owner, TEXT, 0, 0]);
    let id = wait(&mut p, before);
    p.memory.write(u64::from(TEXT), b"edited\0").unwrap();
    assert_eq!(p.pending_message_box().unwrap().text, b"a\r\n\x80\xff");
    assert_eq!(p.pending_message_box().unwrap().owner, owner);
    finish(&mut p, id, before);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(p.last_error().unwrap(), 91);
    let mut expected = prepare(&mut p, [0xdead_beef, u32::MAX, u32::MAX, 0]);
    let result = p.run(1);
    assert_eq!(result.api_calls, 1);
    expected.eip = 0x0040_1014;
    expected.set_register(Register32::Esp, STACK + 20);
    expected.set_register(Register32::Eax, 0);
    assert_eq!(p.cpu, expected);
    assert_eq!(p.last_error().unwrap(), 1400);
    assert!(p.pending_message_box().is_none());
    let child = create_child(&mut p);
    let windows = p.window_snapshots();
    let before = prepare(&mut p, [child, u32::MAX, u32::MAX, 0]);
    failure(&mut p, before, false);
    assert!(p.pending_message_box().is_none());
    assert_eq!(p.window_snapshots(), windows);
}

fn failure(p: &mut Process32, before: Cpu32, fault: bool) {
    let result = p.run(1);
    if fault {
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    } else {
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    }
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

pub fn malformed_messages_do_not_publish_or_mutate() {
    let mut p = Process32::load(&executable(), 64).unwrap();
    p.memory
        .map_zeroed(0x6000_0000, 8192, Permissions::READ_WRITE)
        .unwrap();
    for (args, fault) in [
        ([0, u32::MAX, u32::MAX, 1], false),
        ([0, 0x5000_0000, 0, 0], true),
        ([0, TEXT, 0x5000_0000, 0], true),
    ] {
        let before = prepare(&mut p, args);
        failure(&mut p, before, fault);
        assert!(p.pending_message_box().is_none());
    }
    p.memory.write(0x6000_0000, &[b'x'; 4096]).unwrap();
    let before = prepare(&mut p, [0, 0x6000_0000, 0, 0]);
    failure(&mut p, before, false);
    assert!(p.pending_message_box().is_none());
    p.memory.write(0x6000_0fff, &[0]).unwrap();
    let id = wait(&mut p, before);
    assert_eq!(id, 1);
    assert_eq!(p.pending_message_box().unwrap().text.len(), 4095);
    finish(&mut p, id, before);
    prepare(&mut p, [0, TEXT, CAPTION, 0]);
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = p.cpu;
    failure(&mut p, before, true);
    assert!(p.pending_message_box().is_none());
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_ffec, &[0x0040_1014, 0, 0, 0, 0]);
    p.cpu.set_register(Register32::Esp, 0xffff_ffec);
    let before = p.cpu;
    failure(&mut p, before, true);
    assert!(p.pending_message_box().is_none());
}

pub fn changed_continuations_cannot_consume_acknowledgements() {
    let mut p = Process32::load(&executable(), 64).unwrap();
    let before = prepare(&mut p, [0, TEXT, CAPTION, 0]);
    let id = wait(&mut p, before);
    p.acknowledge_message_box(id).unwrap();
    p.cpu.set_register(Register32::Eax, 88);
    let changed = p.cpu;
    failure(&mut p, changed, false);
    p.cpu = before;
    words(&mut p, STACK, &[0x0040_1015]);
    failure(&mut p, before, false);
    words(&mut p, STACK, &[0x0040_1014]);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(p.cpu.eip, 0x0040_1014);
}

pub fn pending_message_pauses_ready_threads_and_pins_completion() {
    let mut bytes = executable();
    let message_call = bytes[0x200..0x215].to_vec();
    let mut code = Vec::new();
    for value in [0_u32, 4, 0, 0x0040_1100, 0, 0] {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0xb8, 0x48, 5, 0, 0x70, 0xff, 0xd0, 0x83, 0xc4, 24, 0x50]);
    code.extend([0xb8, 0x50, 5, 0, 0x70, 0xff, 0xd0]);
    let return_address = 0x0040_1000 + u32::try_from(code.len()).unwrap() + 20;
    code.extend(message_call);
    code.resize(256, 0xcc);
    code.extend([0xff, 5, 0, 0x22, 0x40, 0, 0xeb, 0xf8]);
    bytes[0x200..0x200 + code.len()].copy_from_slice(&code);
    for budget in [1, 4096, 10000] {
        let mut p = Process32::load(&bytes, 80).unwrap();
        for _ in 0..10000 {
            if p.run(budget).reason == ProcessStop::MessageBoxRequired {
                break;
            }
        }
        let id = p.pending_message_box().unwrap().id;
        let before = p.cpu;
        let mut counter = [0; 4];
        p.memory.read(0x0040_2200, &mut counter).unwrap();
        assert_ne!(counter, [0; 4]);
        assert_eq!(p.run(10000).reason, ProcessStop::MessageBoxRequired);
        let mut after = [0; 4];
        p.memory.read(0x0040_2200, &mut after).unwrap();
        assert_eq!(after, counter);
        assert_eq!(p.cpu, before);
        p.acknowledge_message_box(id).unwrap();
        let result = p.run(1);
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(p.cpu.eip, return_address);
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.fs_base(), before.fs_base());
        p.memory.read(0x0040_2200, &mut after).unwrap();
        assert_eq!(after, counter);
    }
}
