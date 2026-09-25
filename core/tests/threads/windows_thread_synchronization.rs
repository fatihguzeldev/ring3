use super::suspended_thread_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const PRIMARY: u32 = 0x7ffd_e000;
const CHILD: u32 = 0x1101_0000;
const STACK: u32 = 0x1000_ff00;
const MUTEX: u32 = 0x7000_0210;
const WAIT: u32 = 0x7000_0214;
const RELEASE: u32 = 0x7000_0218;
const CLOSE: u32 = 0x7000_021c;
const INIT: u32 = 0x7000_0034;
const ENTER: u32 = 0x7000_0038;
const TRY: u32 = 0x7000_003c;
const LEAVE: u32 = 0x7000_0060;
const OBJECT: u32 = 0x0040_2280;

fn load() -> (Process32, u32) {
    let mut p = Process32::load(&suspended_thread_executable::pe32(), 64).unwrap();
    assert_eq!(
        p.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let handle = p.cpu.register(Register32::Eax);
    (p, handle)
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn read(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000_u32).chain(args).enumerate() {
        put(p, STACK + u32::try_from(index).unwrap() * 4, *value);
    }
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
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    assert_eq!(p.cpu, expected);
    value
}

#[test]
fn named_mutex_ownership_is_bound_to_registered_threads_and_survives_handle_close() {
    let (mut p, child_handle) = load();
    let name = 0x0040_2240;
    p.memory.write(u64::from(name), b"shared\0").unwrap();
    let first = call(&mut p, MUTEX, &[0, 1, name]);
    put(&mut p, PRIMARY + 0x34, 77);
    p.cpu.set_fs_base(CHILD);
    put(&mut p, CHILD + 0x24, 1);
    let second = call(&mut p, MUTEX, &[0, 1, name]);
    assert_eq!(p.last_error().unwrap(), 183);
    assert_eq!(call(&mut p, WAIT, &[second, 0]), 258);
    assert_eq!(call(&mut p, RELEASE, &[second]), 0);
    assert_eq!(p.last_error().unwrap(), 288);
    assert_eq!(read(&p, PRIMARY + 0x34), 77);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, RELEASE, &[first]), 1);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, WAIT, &[second, 0]), 0);
    assert_eq!(call(&mut p, CLOSE, &[child_handle]), 1);
    assert_eq!(call(&mut p, WAIT, &[second, 0]), 0);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, WAIT, &[first, 0]), 258);
    assert_eq!(call(&mut p, RELEASE, &[first]), 0);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, RELEASE, &[second]), 1);
    assert_eq!(call(&mut p, RELEASE, &[second]), 1);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, WAIT, &[first, 0]), 0);
}

#[test]
fn critical_sections_distinguish_registered_owners_from_guest_teb_identity() {
    let (mut p, child_handle) = load();
    call(&mut p, INIT, &[OBJECT]);
    call(&mut p, ENTER, &[OBJECT]);
    p.cpu.set_fs_base(CHILD);
    put(&mut p, CHILD + 0x24, 1);
    assert_eq!(call(&mut p, TRY, &[OBJECT]), 0);
    assert_eq!(read(&p, OBJECT + 8), 1);
    assert_eq!(read(&p, OBJECT + 12), 1);
    for api in [ENTER, LEAVE] {
        let before = prepare(&mut p, api, &[OBJECT]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: api }
        );
        assert_eq!(p.cpu, before);
    }
    p.cpu.set_fs_base(PRIMARY);
    call(&mut p, LEAVE, &[OBJECT]);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, TRY, &[OBJECT]), 1);
    call(&mut p, ENTER, &[OBJECT]);
    assert_eq!(read(&p, OBJECT + 8), 2);
    assert_eq!(read(&p, OBJECT + 12), 2);
    assert_eq!(call(&mut p, CLOSE, &[child_handle]), 1);
    call(&mut p, LEAVE, &[OBJECT]);
    call(&mut p, LEAVE, &[OBJECT]);
    assert_eq!(read(&p, OBJECT + 8), 0);
    assert_eq!(read(&p, OBJECT + 12), 0);
}

#[test]
fn contended_mutex_waits_and_nonowner_error_faults_preserve_ownership() {
    let (mut p, _) = load();
    let handle = call(&mut p, MUTEX, &[0, 1, 0]);
    put(&mut p, PRIMARY + 0x34, 77);
    put(&mut p, CHILD + 0x34, 55);
    p.cpu.set_fs_base(CHILD);
    for timeout in [1, u32::MAX] {
        let before = prepare(&mut p, WAIT, &[handle, timeout]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: WAIT }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(p.last_error().unwrap(), 55);
    }
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, RELEASE, &[handle]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(p.last_error().unwrap(), 55);
    assert_eq!(read(&p, PRIMARY + 0x34), 77);
    p.memory
        .protect(u64::from(CHILD), PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, WAIT, &[handle, 0]), 258);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, RELEASE, &[handle]), 1);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, WAIT, &[handle, 0]), 0);
}

#[test]
fn contended_critical_sections_validate_guest_state_without_writing_it() {
    let (mut p, _) = load();
    call(&mut p, INIT, &[OBJECT]);
    call(&mut p, ENTER, &[OBJECT]);
    p.cpu.set_fs_base(CHILD);
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(call(&mut p, TRY, &[OBJECT]), 0);
    for api in [ENTER, LEAVE, 0x7000_0064] {
        let before = prepare(&mut p, api, &[OBJECT]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: api }
        );
        assert_eq!(p.cpu, before);
    }
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    put(&mut p, OBJECT + 12, 2);
    let before = prepare(&mut p, TRY, &[OBJECT]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: TRY }
    );
    assert_eq!(p.cpu, before);
    put(&mut p, OBJECT + 12, 1);
    p.cpu.set_fs_base(PRIMARY);
    call(&mut p, LEAVE, &[OBJECT]);
    p.cpu.set_fs_base(CHILD);
    call(&mut p, ENTER, &[OBJECT]);
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, LEAVE, &[OBJECT]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(read(&p, OBJECT + 12), 2);
    assert_eq!(read(&p, OBJECT + 8), 1);
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, LEAVE, &[OBJECT]);
    p.cpu.set_fs_base(PRIMARY);
    call(&mut p, 0x7000_0064, &[OBJECT]);
}

#[test]
fn unregistered_actors_cannot_change_shared_objects_or_handles() {
    let (mut p, child_handle) = load();
    let mutex = call(&mut p, MUTEX, &[0, 1, 0]);
    call(&mut p, INIT, &[OBJECT]);
    let pages = p.memory.mapped_pages();
    p.cpu.set_fs_base(0x5000_0000);
    for (api, args) in [
        (MUTEX, vec![0, 0, 0]),
        (WAIT, vec![mutex, 0]),
        (RELEASE, vec![mutex]),
        (CLOSE, vec![child_handle]),
        (ENTER, vec![OBJECT]),
        (INIT, vec![OBJECT + 24]),
        (0x7000_053c, vec![0, 0, 0, 0]),
    ] {
        let before = prepare(&mut p, api, &args);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: api });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    assert_eq!(p.memory.mapped_pages(), pages);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, WAIT, &[child_handle, 0]), 258);
    assert_eq!(call(&mut p, RELEASE, &[mutex]), 1);
    call(&mut p, ENTER, &[OBJECT]);
}

#[test]
fn child_creation_and_invalid_handle_errors_use_only_the_child_teb() {
    let (mut p, _) = load();
    let name = 0x0040_2240;
    p.memory.write(u64::from(name), b"typed\0").unwrap();
    call(&mut p, MUTEX, &[0, 0, name]);
    put(&mut p, PRIMARY + 0x34, 77);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, 0x7000_053c, &[0, 0, 0, name]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(read(&p, PRIMARY + 0x34), 77);
    put(&mut p, CHILD + 0x34, 55);
    assert_eq!(call(&mut p, WAIT, &[0, 0]), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(read(&p, PRIMARY + 0x34), 77);
    let owned = call(&mut p, MUTEX, &[0, 1, 0]);
    p.cpu.set_fs_base(PRIMARY);
    assert_eq!(call(&mut p, WAIT, &[owned, 0]), 258);
    p.cpu.set_fs_base(CHILD);
    assert_eq!(call(&mut p, RELEASE, &[owned]), 1);
}
