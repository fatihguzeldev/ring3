#[path = "support/file_size_cases.rs"]
mod file_size_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_size_preserves_lifetime_and_abi() {
    file_size_cases::imported_size_preserves_lifetime_and_abi();
}

use file_size_cases::{
    CLOSE, ERRNO, ERROR, FIRST, HIGH, PATH, SIZE, STACK, call, open, prepare, process, put, word,
};
use ring3_core::execution::{
    MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

fn fault(p: &mut Process32) {
    let before = p.cpu;
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn queries_use_owned_content_and_preserve_reader_lifetime() {
    let mut p = process(b"some data", 0);
    assert_eq!(open(&mut p), FIRST);
    let other = open(&mut p);
    p.memory.write(u64::from(PATH), b"changed.bin\0").unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    let pages = p.memory.mapped_pages();
    for _ in 0..3 {
        assert_eq!(call(&mut p, SIZE, &[FIRST, 0]), 9);
        assert_eq!(call(&mut p, SIZE, &[other, STACK + 64]), 9);
        assert_eq!(word(&p, STACK + 64), 0);
    }
    assert_eq!(call(&mut p, CLOSE, &[other]), 1);
    assert_eq!(call(&mut p, SIZE, &[FIRST, 0]), 9);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(word(&p, ERROR), 77);
    assert_eq!(word(&p, ERRNO), 88);
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .write(u64::from(PATH), b"C:\\sample.bin\0")
        .unwrap();
    let remove = |p: &mut Process32| {
        prepare(p, 0x7000_0188, &[PATH]);
        assert_eq!(p.run(1).api_calls, 1);
        p.cpu.register(Register32::Eax)
    };
    assert_eq!(remove(&mut p), u32::MAX);
    assert_eq!(call(&mut p, SIZE, &[FIRST, HIGH]), 9);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    assert_eq!(remove(&mut p), 0);
}

#[test]
fn invalid_handles_do_not_access_output_or_change_other_lifetimes() {
    let mut p = process(b"1234", 0);
    assert_eq!(open(&mut p), FIRST);
    let mutex = call(&mut p, 0x7000_0210, &[0, 0, 0]);
    p.memory.write(u64::from(PATH), b"C:\\*\0").unwrap();
    let search = call(&mut p, 0x7000_0234, &[PATH, 0x0040_2400]);
    for invalid in [
        0,
        u32::MAX,
        u32::MAX - 1,
        FIRST + 1,
        FIRST + 4,
        mutex,
        search,
    ] {
        assert_eq!(call(&mut p, SIZE, &[invalid, HIGH]), u32::MAX);
        assert_eq!(word(&p, HIGH), 0xfeed_abba);
        assert_eq!(word(&p, ERROR), 6);
        assert_eq!(call(&mut p, SIZE, &[invalid, u32::MAX]), u32::MAX);
    }
    assert_eq!(call(&mut p, CLOSE, &[mutex]), 1);
    assert_eq!(call(&mut p, 0x7000_023c, &[search]), 1);
    assert_eq!(call(&mut p, SIZE, &[FIRST, HIGH]), 4);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    put(&mut p, HIGH, 0xfeed_abba);
    assert_eq!(call(&mut p, SIZE, &[FIRST, HIGH]), u32::MAX);
    assert_eq!(word(&p, HIGH), 0xfeed_abba);
    assert_eq!(word(&p, ERRNO), 88);
}

#[test]
fn high_output_preflights_the_whole_write_and_retries_after_repair() {
    let mut p = process(b"123456", 0);
    assert_eq!(open(&mut p), FIRST);
    p.memory
        .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6000_0ffe, &[0x11, 0x22]).unwrap();
    prepare(&mut p, SIZE, &[FIRST, 0x6000_0ffe]);
    fault(&mut p);
    let mut prefix = [0; 2];
    p.memory.read(0x6000_0ffe, &mut prefix).unwrap();
    assert_eq!(prefix, [0x11, 0x22]);
    p.memory
        .map_zeroed(0x6000_1000, 4096, Permissions::READ)
        .unwrap();
    fault(&mut p);
    p.memory.read(0x6000_0ffe, &mut prefix).unwrap();
    assert_eq!(prefix, [0x11, 0x22]);
    p.memory
        .protect(
            0x6000_1000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    p.memory
        .protect(0x6000_1000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(word(&p, 0x6000_0ffe), 0);
    prepare(&mut p, SIZE, &[FIRST, 0x5000_0000]);
    fault(&mut p);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fffe, &[0x55, 0x66]).unwrap();
    prepare(&mut p, SIZE, &[FIRST, 0xffff_fffe]);
    let before = p.cpu;
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
    );
    assert_eq!(p.cpu, before);
    assert_eq!(run.api_calls, 0);
    p.memory.read(0xffff_fffe, &mut prefix).unwrap();
    assert_eq!(prefix, [0x55, 0x66]);
    assert_eq!(call(&mut p, SIZE, &[FIRST, HIGH]), 6);
    assert_eq!(word(&p, ERROR), 77);
}

#[test]
fn captured_arguments_and_reread_return_slot_define_output_aliases() {
    for output in [STACK, STACK + 4, STACK + 8, ERROR, ERRNO] {
        let mut p = process(b"data", 0);
        assert_eq!(open(&mut p), FIRST);
        let mut expected = prepare(&mut p, SIZE, &[FIRST, output]);
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((run.instructions, run.api_calls), (0, 1));
        expected.eip = if output == STACK { 0 } else { 0x0040_1000 };
        expected.set_register(Register32::Esp, STACK + 12);
        expected.set_register(Register32::Eax, 4);
        assert_eq!(p.cpu, expected);
        assert_eq!(word(&p, output), 0);
        assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
    }
    let mut p = process(b"", 0);
    assert_eq!(call(&mut p, SIZE, &[FIRST, ERROR]), u32::MAX);
    assert_eq!(word(&p, ERROR), 6);
}

#[test]
fn error_page_and_unregistered_actor_fail_without_losing_the_handle() {
    let mut p = process(b"data", 0);
    assert_eq!(open(&mut p), FIRST);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, SIZE, &[FIRST + 4, HIGH]);
    fault(&mut p);
    assert_eq!(word(&p, HIGH), 0xfeed_abba);
    assert_eq!(word(&p, ERROR), 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(word(&p, ERROR), 6);
    let fs = p.cpu.fs_base();
    p.cpu.set_fs_base(0x6000_0000);
    let before = prepare(&mut p, SIZE, &[FIRST, HIGH]);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: SIZE });
    assert_eq!(p.cpu, before);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(word(&p, HIGH), 0xfeed_abba);
    p.cpu.set_fs_base(fs);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Eax), 4);
    assert_eq!(call(&mut p, CLOSE, &[FIRST]), 1);
}

#[test]
fn incomplete_and_top_address_frames_do_not_write_output() {
    let mut p = process(b"data", 0);
    assert_eq!(open(&mut p), FIRST);
    p.memory
        .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x6000_0ff8, 0x0040_1000);
    put(&mut p, 0x6000_0ffc, FIRST);
    p.cpu.eip = SIZE;
    p.cpu.set_register(Register32::Esp, 0x6000_0ff8);
    fault(&mut p);
    assert_eq!(word(&p, HIGH), 0xfeed_abba);
    p.memory
        .map_zeroed(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, 0x6000_1000, HIGH);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(word(&p, HIGH), 0);
    put(&mut p, HIGH, 0xfeed_abba);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (i, value) in [0x0040_1000, FIRST, HIGH].into_iter().enumerate() {
        put(&mut p, 0xffff_fff4 + u32::try_from(i).unwrap() * 4, value);
    }
    p.cpu.eip = SIZE;
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    let before = p.cpu;
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow))
    );
    assert_eq!(p.cpu, before);
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(word(&p, HIGH), 0xfeed_abba);
    assert_eq!(call(&mut p, SIZE, &[FIRST, HIGH]), 4);
}
