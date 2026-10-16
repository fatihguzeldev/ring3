use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, X87_OFFSET},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::{Access, GuestAddress},
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0xa533_1234_5678_abcd;
const CODE: u32 = 0x1000;
const ESP: u32 = 0x8080;
const RETURN: u32 = 0x9000;
const OUTPUT: u32 = 0x8100;
const WINDOW: u32 = 0x1000_0000;
const ALLOC: u32 = 0x10005;
const PROTECT: u32 = 0x10007;
const GET: u32 = 0x10001;
const SET: u32 = 0x10002;
const KEEP: u32 = 0x3000;
const MODES: [(u32, u32); 6] = [(1, 0), (2, 1), (4, 3), (0x10, 4), (0x20, 5), (0x40, 7)];

#[derive(Clone, Copy)]
enum Owner {
    Replacement,
    Resident,
}

fn capture(
    engine: &mut EngineInstance,
    owner: Owner,
    unit: u64,
    id: u32,
    args: [u32; 4],
) -> (u32, State32) {
    engine.write32(ESP, RETURN).unwrap();
    for (index, value) in args.into_iter().enumerate() {
        engine.write32(ESP + 4 + index as u32 * 4, value).unwrap();
    }
    // native gate input; generated guest calls are a separate actual-wasm proof.
    let state = State32 {
        registers: [0x89ab_cdef, 1, 2, 3, ESP, 5, 6, 7],
        eip: CODE
            + match id {
                ALLOC => 0,
                PROTECT => 16,
                GET => 32,
                SET => 48,
                _ => unreachable!(),
            },
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Gate { id },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    let count = match id {
        GET => 0,
        SET => 1,
        _ => 4,
    };
    let record = match owner {
        Owner::Replacement => {
            engine.capture_call(KEY, unit as u32, CallingConvention32::Stdcall, count)
        }
        Owner::Resident => {
            engine.capture_resident_call(KEY, unit, CallingConvention32::Stdcall, count)
        }
    }
    .unwrap();
    (record.token, state)
}

fn complete(
    engine: &mut EngineInstance,
    authority: (Owner, u64),
    token: u32,
    state: State32,
    result: u32,
    count: u32,
    output: Option<u32>,
) {
    let (owner, unit) = authority;
    let mut expected = engine.arena().to_vec();
    let mut next = state;
    next.registers[0] = result;
    next.registers[4] += 4 + count * 4;
    next.eip = RETURN;
    encode_state(&next, &mut expected[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut expected[56..96],
    )
    .unwrap();
    let actual = match owner {
        Owner::Replacement => engine.complete_windows_call(KEY, unit as u32, token),
        Owner::Resident => engine.complete_resident_windows_call(KEY, unit, token),
    };
    assert_eq!(actual, Ok(()));
    assert_eq!(engine.arena(), expected, "output pointer {output:#x?}");
}

#[test]
fn virtual_protect_changes_an_owned_rw_page_to_rx_and_reports_old_mode() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut engine = EngineInstance::new(3, KEY).unwrap();
        engine.map(CODE, 1, 7).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        for offset in [0, 16] {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
                .copy_from_slice(&[0x0f, 0x0b]);
            engine.upload(CODE + offset, 2).unwrap();
        }
        for (index, value) in [CODE, 2, CODE + 16, 2, CODE, ALLOC, CODE + 16, PROTECT]
            .into_iter()
            .enumerate()
        {
            let at = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let unit = match owner {
            Owner::Replacement => u64::from(engine.compile_with_gates(2, 2).unwrap()),
            Owner::Resident => engine.compile_resident_with_gates(2, 2).unwrap().get(),
        };
        let (token, state) = capture(&mut engine, owner, unit, ALLOC, [0, 4096, 0x3000, 4]);
        complete(&mut engine, (owner, unit), token, state, WINDOW, 4, None);
        engine.write32(OUTPUT, 0xfeed_cafe).unwrap();
        let (token, state) = capture(
            &mut engine,
            owner,
            unit,
            PROTECT,
            [WINDOW, 4096, 0x20, OUTPUT],
        );
        complete(&mut engine, (owner, unit), token, state, 1, 4, None);
        let memory = engine.memory().unwrap();
        let mut old = [0; 4];
        memory.read(GuestAddress(OUTPUT), &mut old).unwrap();
        assert_eq!(u32::from_le_bytes(old), 4);
        assert!(memory.resolve(GuestAddress(WINDOW), Access::Read).is_ok());
        assert!(
            memory
                .resolve(GuestAddress(WINDOW), Access::Execute)
                .is_ok()
        );
        assert!(memory.resolve(GuestAddress(WINDOW), Access::Write).is_err());
    }
}

struct Fixture {
    engine: EngineInstance,
    owner: Owner,
    generation: u32,
    resident: u64,
    keeper: u64,
    output: Option<u32>,
}

impl Fixture {
    fn new(owner: Owner, pages: u32) -> Self {
        let mut engine = EngineInstance::new(pages, KEY).unwrap();
        engine.map(CODE, 1, 7).unwrap();
        engine.map(KEEP, 1, 7).unwrap();
        engine.map(0x8000, 2, 3).unwrap();
        for offset in [0, 16, 32, 48] {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
                .copy_from_slice(&[0x0f, 0x0b]);
            engine.upload(CODE + offset, 2).unwrap();
        }
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x90;
        engine.upload(KEEP, 1).unwrap();
        describe(&mut engine);
        let resident = engine.compile_resident_with_gates(4, 4).unwrap().get();
        for (index, value) in [KEEP, 1].into_iter().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let keeper = engine.compile_resident(1).unwrap().get();
        describe(&mut engine);
        let generation = engine.compile_with_gates(4, 4).unwrap();
        let arena = engine.arena_mut().unwrap();
        arena[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
        arena[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
        for (index, byte) in arena[X87_OFFSET + 40..X87_OFFSET + 120]
            .iter_mut()
            .enumerate()
        {
            *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
        }
        Self {
            engine,
            owner,
            generation,
            resident,
            keeper,
            output: None,
        }
    }

    fn unit(&self) -> u64 {
        match self.owner {
            Owner::Replacement => u64::from(self.generation),
            Owner::Resident => self.resident,
        }
    }

    fn capture(&mut self, id: u32, args: [u32; 4]) -> (u32, State32) {
        let unit = self.unit();
        capture(&mut self.engine, self.owner, unit, id, args)
    }

    fn complete_raw(&mut self, token: u32) -> Result<(), HostError> {
        match self.owner {
            Owner::Replacement => self
                .engine
                .complete_windows_call(KEY, self.generation, token),
            Owner::Resident => {
                self.engine
                    .complete_resident_windows_call(KEY, self.resident, token)
            }
        }
    }

    fn completed(&mut self, token: u32, state: State32, result: u32, count: u32) {
        let unit = self.unit();
        complete(
            &mut self.engine,
            (self.owner, unit),
            token,
            state,
            result,
            count,
            self.output,
        );
    }

    fn invoke(&mut self, id: u32, args: [u32; 4], result: u32) {
        let (token, state) = self.capture(id, args);
        self.completed(
            token,
            state,
            result,
            match id {
                GET => 0,
                SET => 1,
                _ => 4,
            },
        );
    }

    fn allocate(&mut self, size: u32, mode: u32, address: u32) {
        self.invoke(ALLOC, [0, size, 0x3000, mode], address);
    }

    fn set_error(&mut self, error: u32) {
        self.invoke(SET, [error, 0, 0, 0], 0x89ab_cdef);
    }

    fn current(&self) {
        self.engine.guard(KEY, self.generation).unwrap();
        self.engine.guard_resident(KEY, self.resident).unwrap();
        self.engine.guard_resident(KEY, self.keeper).unwrap();
    }
}

fn describe(engine: &mut EngineInstance) {
    for (index, value) in [
        CODE,
        2,
        CODE + 16,
        2,
        CODE + 32,
        2,
        CODE + 48,
        2,
        CODE,
        ALLOC,
        CODE + 16,
        PROTECT,
        CODE + 32,
        GET,
        CODE + 48,
        SET,
    ]
    .into_iter()
    .enumerate()
    {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn bytes(engine: &EngineInstance, address: u32, length: usize) -> Result<Vec<u8>, HostError> {
    let mut bytes = vec![0; length];
    engine
        .memory()?
        .read(GuestAddress(address), &mut bytes)
        .map_err(HostError::Memory)?;
    Ok(bytes)
}

fn word(engine: &EngineInstance, address: u32) -> u32 {
    u32::from_le_bytes(bytes(engine, address, 4).unwrap().try_into().unwrap())
}

type ModuleView = Result<(Vec<u8>, usize), HostError>;
#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    pointer: usize,
    modules: [ModuleView; 3],
    mapped: u32,
    ram: Vec<Result<Vec<u8>, HostError>>,
    access: Vec<[bool; 3]>,
}

fn saved(f: &Fixture) -> Saved {
    let e = &f.engine;
    let pages = [
        CODE,
        KEEP,
        0x8000,
        0x9000,
        0xa000,
        0xb000,
        WINDOW,
        WINDOW + 4096,
    ];
    Saved {
        arena: e.arena().to_vec(),
        pointer: e.arena_address(),
        modules: [
            e.artifact_bytes(),
            e.resident_bytes(f.resident),
            e.resident_bytes(f.keeper),
        ]
        .map(|result| result.map(|b| (b.to_vec(), b.as_ptr() as usize))),
        mapped: e.memory().unwrap().mapped_pages(),
        ram: pages.map(|address| bytes(e, address, 4096)).to_vec(),
        access: pages
            .map(|address| {
                [Access::Read, Access::Write, Access::Execute].map(|access| {
                    e.memory()
                        .unwrap()
                        .resolve(GuestAddress(address), access)
                        .is_ok()
                })
            })
            .to_vec(),
    }
}

fn reject(f: &mut Fixture, token: u32, error: HostError) {
    let before = saved(f);
    assert_eq!(f.complete_raw(token), Err(error));
    assert_eq!(saved(f), before);
}

#[test]
fn all_simple_modes_report_first_page_and_accept_cross_page_output() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for (old_mode, _) in MODES {
            for (new_mode, new_bits) in MODES {
                let mut f = Fixture::new(owner, 6);
                f.set_error(0xf123_4567);
                f.allocate(4097, old_mode, WINDOW);
                f.engine.write32(0x8ffe, 0xfeed_cafe).unwrap();
                let (token, state) = f.capture(PROTECT, [WINDOW + 1, 4096, new_mode, 0x8ffe]);
                let mut stack = bytes(&f.engine, 0x8000, 8192).unwrap();
                stack[0xffe..0x1002].copy_from_slice(&old_mode.to_le_bytes());
                let before_modules = saved(&f).modules;
                f.completed(token, state, 1, 4);
                assert_eq!(bytes(&f.engine, 0x8000, 8192).unwrap(), stack);
                assert_eq!(saved(&f).modules, before_modules);
                assert_eq!(f.engine.memory().unwrap().mapped_pages(), 6);
                for address in [WINDOW, WINDOW + 4095, WINDOW + 4096, WINDOW + 8191] {
                    for (access, bit) in
                        [(Access::Read, 1), (Access::Write, 2), (Access::Execute, 4)]
                    {
                        assert_eq!(
                            f.engine
                                .memory()
                                .unwrap()
                                .resolve(GuestAddress(address), access)
                                .is_ok(),
                            new_bits & bit != 0
                        );
                    }
                }
                for (fetch, allowed) in [(false, new_bits & 1 != 0), (true, new_bits & 4 != 0)] {
                    let mut data = vec![0xa5; 8192];
                    let result = if fetch {
                        f.engine
                            .memory()
                            .unwrap()
                            .fetch(GuestAddress(WINDOW), &mut data)
                    } else {
                        f.engine
                            .memory()
                            .unwrap()
                            .read(GuestAddress(WINDOW), &mut data)
                    };
                    assert_eq!(result.is_ok(), allowed);
                    assert_eq!(data, vec![if allowed { 0 } else { 0xa5 }; 8192]);
                }
                f.current();
                f.invoke(GET, [0; 4], 0xf123_4567);
                reject(&mut f, token, HostError::Call(CallError::InvalidToken));
            }
        }
    }
}

#[test]
fn partial_and_mixed_page_changes_report_only_the_first_old_mode() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = Fixture::new(owner, 6);
        f.allocate(8192, 0x40, WINDOW);
        f.engine.write32(WINDOW, 0x1234_5678).unwrap();
        f.engine.write32(WINDOW + 8188, 0x89ab_cdef).unwrap();
        let first = f
            .engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(WINDOW), 1)
            .unwrap();
        let second = f
            .engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(WINDOW + 4096), 1)
            .unwrap();
        f.invoke(PROTECT, [WINDOW + 4096, 1, 2, OUTPUT], 1);
        assert_eq!(word(&f.engine, OUTPUT), 0x40);
        assert!(f.engine.memory().unwrap().is_code_current(&first));
        assert!(!f.engine.memory().unwrap().is_code_current(&second));
        f.invoke(PROTECT, [WINDOW + 3, 4096, 0x10, OUTPUT], 1);
        assert_eq!(word(&f.engine, OUTPUT), 0x40);
        let mut data = [0; 8192];
        f.engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(WINDOW), &mut data)
            .unwrap();
        assert_eq!(&data[..4], &0x1234_5678_u32.to_le_bytes());
        assert_eq!(&data[8188..], &0x89ab_cdef_u32.to_le_bytes());
        assert!(!f.engine.memory().unwrap().is_code_current(&first));
        f.current();
    }
}

#[test]
fn invalid_protect_arguments_are_consumed_without_changing_memory_or_artifacts() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for (args, error) in [
            ([WINDOW, 0, 4, OUTPUT], 87),
            ([WINDOW, 65_537, 4, OUTPUT], 487),
            ([u32::MAX, 2, 4, OUTPUT], 87),
            ([WINDOW, 1, 3, OUTPUT], 87),
            ([WINDOW + 127, 1, 4, WINDOW - 3], 998),
            ([CODE + 127, 1, 4, CODE], 487),
        ] {
            let mut f = Fixture::new(owner, 6);
            f.allocate(8192, 4, WINDOW);
            f.set_error(0xf123_4567);
            let (token, state) = f.capture(PROTECT, args);
            let before = saved(&f);
            f.completed(token, state, 0, 4);
            let after = saved(&f);
            assert_eq!(after.ram, before.ram);
            assert_eq!(after.access, before.access);
            assert_eq!(after.modules, before.modules);
            assert_eq!(after.mapped, before.mapped);
            reject(&mut f, token, HostError::Call(CallError::InvalidToken));
            f.invoke(GET, [0; 4], error);
        }
        for bits in [2, 6] {
            let mut f = Fixture::new(owner, 6);
            f.allocate(8192, 4, WINDOW);
            f.engine.protect(WINDOW, 1, bits).unwrap();
            let (token, state) = f.capture(PROTECT, [WINDOW, 1, 4, 0]);
            let before = saved(&f);
            f.completed(token, state, 0, 4);
            let after = saved(&f);
            assert_eq!(after.ram, before.ram);
            assert_eq!(after.access, before.access);
            f.invoke(GET, [0; 4], 998);
        }
    }
}

#[test]
fn guarded_protection_and_output_aliases_use_atomic_full_vm_semantics() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for (address, protection, output) in [
            (WINDOW, 0x140, OUTPUT),
            (WINDOW + 127, 4, WINDOW),
            (WINDOW + 127, 4, WINDOW + 4095),
        ] {
            let mut f = Fixture::new(owner, 6);
            f.allocate(8192, 4, WINDOW);
            f.set_error(0xf123_4567);
            let (token, state) = f.capture(PROTECT, [address, 1, protection, output]);
            f.completed(token, state, 1, 4);
            assert_eq!(word(&f.engine, output), 4);
            f.invoke(PROTECT, [WINDOW, 1, 4, OUTPUT], 1);
            assert_eq!(word(&f.engine, OUTPUT), protection);
            f.current();
            f.invoke(GET, [0; 4], 0xf123_4567);
        }
    }
}

#[test]
fn invalid_outputs_complete_false_without_partial_writes_or_protection() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for output in [0, u32::MAX - 1, 0xb000, 0x9ffe, 0xa000] {
            let mut f = Fixture::new(owner, 6);
            f.output = Some(output);
            f.allocate(4096, 4, WINDOW);
            if output == 0xa000 {
                f.engine.map(0xa000, 1, 1).unwrap();
            }
            f.engine.write32(0x9ffc, 0xfeed_cafe).unwrap();
            f.set_error(0xf123_4567);
            let (token, state) = f.capture(PROTECT, [WINDOW, 4096, 0x20, output]);
            let before = saved(&f);
            f.completed(token, state, 0, 4);
            let after = saved(&f);
            assert_eq!(after.ram, before.ram);
            assert_eq!(after.access, before.access);
            assert_eq!(after.modules, before.modules);
            assert_eq!(after.mapped, before.mapped);
            f.current();
            f.invoke(GET, [0; 4], 998);
            reject(&mut f, token, HostError::Call(CallError::InvalidToken));
        }
    }
}

#[test]
fn invalid_targets_and_null_outputs_are_distinct_failure_neutral_errors() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = Fixture::new(owner, 36);
        f.allocate(65_536, 4, WINDOW);
        f.allocate(65_536, 4, WINDOW + 0x10000);
        for (address, size) in [(CODE, 1), (WINDOW + 65_535, 2), (WINDOW + 0x20000, 1)] {
            let (token, state) = f.capture(PROTECT, [address, size, 0x20, OUTPUT]);
            let before = saved(&f);
            f.completed(token, state, 0, 4);
            let after = saved(&f);
            assert_eq!(after.ram, before.ram);
            assert_eq!(after.access, before.access);
            assert_eq!(after.modules, before.modules);
            assert_eq!(after.mapped, 36);
            for at in [WINDOW + 65535, WINDOW + 65536] {
                assert!(
                    f.engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(at), Access::Write)
                        .is_ok()
                );
            }
            f.invoke(GET, [0; 4], 487);
            let (token, state) = f.capture(PROTECT, [address, size, 0x20, 0]);
            let before = saved(&f);
            f.completed(token, state, 0, 4);
            let after = saved(&f);
            assert_eq!(after.ram, before.ram);
            assert_eq!(after.access, before.access);
            assert_eq!(after.modules, before.modules);
            f.invoke(GET, [0; 4], 998);
        }
    }
}

#[test]
fn state_token_and_cancellation_guards_precede_the_memory_transaction() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = Fixture::new(owner, 5);
        f.allocate(4096, 4, WINDOW);
        let (token, state) = f.capture(PROTECT, [WINDOW, 4096, 0x20, OUTPUT]);
        let before = saved(&f);
        let wrong_key = match owner {
            Owner::Replacement => f.engine.complete_windows_call(KEY + 1, f.generation, token),
            Owner::Resident => f
                .engine
                .complete_resident_windows_call(KEY + 1, f.resident, token),
        };
        assert_eq!(wrong_key, Err(HostError::InvalidArtifact));
        assert_eq!(saved(&f), before);
        reject(&mut f, 0, HostError::Call(CallError::InvalidToken));
        reject(&mut f, token + 1, HostError::Call(CallError::InvalidToken));
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut f, token, HostError::Call(CallError::Cancelled));
        f.engine.arena_mut().unwrap()[96..100].fill(0);
        f.engine.arena_mut().unwrap()[16] ^= 1;
        reject(&mut f, token, HostError::Call(CallError::StateChanged));
        f.engine.arena_mut().unwrap()[16] ^= 1;
        f.completed(token, state, 1, 4);
        assert_eq!(word(&f.engine, OUTPUT), 4);
        f.current();
    }
}

#[test]
fn identical_protection_and_executable_output_invalidate_their_own_sources() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = Fixture::new(owner, 5);
        f.allocate(4096, 0x40, WINDOW);
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x90;
        f.engine.upload(WINDOW, 1).unwrap();
        for (index, value) in [WINDOW, 1].into_iter().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            f.engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let target = f.engine.compile_resident(1).unwrap().get();
        f.invoke(PROTECT, [WINDOW, 1, 0x40, OUTPUT], 1);
        assert_eq!(word(&f.engine, OUTPUT), 0x40);
        assert_eq!(
            f.engine.resident_bytes(target),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        f.current();
        f.invoke(PROTECT, [WINDOW, 1, 0x20, KEEP + 64], 1);
        assert_eq!(word(&f.engine, KEEP + 64), 0x40);
        assert_eq!(
            f.engine.resident_bytes(f.keeper),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        f.engine.guard(KEY, f.generation).unwrap();
        f.engine.guard_resident(KEY, f.resident).unwrap();
    }
}

#[test]
fn output_on_caller_code_commits_before_that_owner_becomes_stale() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = Fixture::new(owner, 5);
        f.allocate(4096, 4, WINDOW);
        let (token, state) = f.capture(PROTECT, [WINDOW, 1, 0x20, CODE + 128]);
        f.completed(token, state, 1, 4);
        assert_eq!(word(&f.engine, CODE + 128), 4);
        assert_eq!(f.engine.artifact_bytes(), Err(HostError::CodeInvalidated));
        assert_eq!(
            f.engine.resident_bytes(f.resident),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        f.engine.guard_resident(KEY, f.keeper).unwrap();
        describe(&mut f.engine);
        match owner {
            Owner::Replacement => f.generation = f.engine.compile_with_gates(4, 4).unwrap(),
            Owner::Resident => {
                f.resident = f.engine.compile_resident_with_gates(4, 4).unwrap().get()
            }
        }
        reject(&mut f, token, HostError::Call(CallError::InvalidToken));
    }
}
