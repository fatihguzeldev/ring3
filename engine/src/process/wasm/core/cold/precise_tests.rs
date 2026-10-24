use super::*;
use crate::cpu::{
    core::{
        CompileOptions, compile, compile_observed_with_cold,
        trace::{BLOCK, CompletedColdObservation, PendingColdObservation, TraceArena, TraceEvent},
    },
    x86::State32,
};

struct Capture {
    arena: TraceArena,
    pending: Option<PendingColdObservation>,
    completed: Vec<CompletedColdObservation>,
    hot: Vec<bool>,
}

impl Capture {
    fn active(source: &RegionRef, end: u32) -> Self {
        let mut arena = TraceArena::new();
        arena.begin();
        assert!(arena.push(TraceEvent {
            kind: BLOCK,
            pc: 0x1000,
            end,
            offset: 0,
            owner: source.identity(),
        }));
        Self {
            arena,
            pending: None,
            completed: Vec::new(),
            hot: Vec::new(),
        }
    }
}

impl ColdObserver for Capture {
    fn begin_instruction(
        &mut self,
        memory: &AddressSpace,
        instruction: &crate::cpu::core::NativeInstruction,
        hot: bool,
    ) -> bool {
        self.hot.push(hot);
        if self.arena.active == 1 {
            self.pending = PendingColdObservation::capture(memory, instruction);
            assert!(self.pending.is_some());
        }
        true
    }

    fn complete_instruction(&mut self, memory: &AddressSpace, pc: u32) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let completed = pending.complete(memory, pc).unwrap();
        assert!(
            self.arena
                .push(completed.event(self.completed.len()).unwrap())
        );
        self.completed.push(completed);
    }

    fn abandon_instruction(&mut self) {
        self.pending = None;
        self.arena.deactivate();
    }
}

fn fixture(
    bytes: &[u8],
    instructions: usize,
    pc: u32,
    budget: u32,
) -> (
    EngineInstance,
    Context32,
    NumericBank,
    CoreCache32,
    RegionRef,
) {
    let mut instance = EngineInstance::new(4, 7).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.map(0x3000, 1, 3).unwrap();
    let memory = instance.memory.as_mut().unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let code = compile(
        memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions,
            blocks: 8,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let mut cache = CoreCache32::default();
    let source = cache.admit(memory, code).unwrap();
    let bank = NumericBank::from_instance(&instance).unwrap();
    let mut context = Context32::new(State32 {
        registers: [0, 0, 0, 0x3000, 0x3800, 0, 0, 0],
        eip: pc,
        eflags: 2,
    });
    context.set_word(44, budget);
    context.set_word(112, 1);
    context.bytes_mut()[120..128].copy_from_slice(&source.identity().to_le_bytes());
    (instance, context, bank, cache, source)
}

fn identity(context: &Context32) -> u64 {
    u64::from_le_bytes(context.bytes()[120..128].try_into().unwrap())
}

#[test]
fn precise_interior_short_quota_executes_only_the_actual_prefix() {
    let (mut instance, mut context, mut bank, cache, source) =
        fixture(&[0x40, 0x40, 0xeb, 0xfc], 3, 0x1001, 1);
    let before_bank = bank.architectural_state();
    let before_cache = cache.snapshot();
    let mut selected = Some(source.clone());
    let mut heat = ColdState::default();
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut heat,
            &mut Unobserved,
        ),
        StopReason::Budget
    );
    assert_eq!(context.state().registers[0], 1);
    assert_eq!(context.state().eip, 0x1002);
    assert_eq!((context.retired(), context.remaining()), (1, 0));
    assert_eq!(context.state().eflags, 2);
    assert_eq!(identity(&context), source.identity());
    assert_eq!(selected.unwrap().identity(), source.identity());
    assert_eq!(&context.bytes()[112..116], &1_u32.to_le_bytes());
    assert_eq!(bank.architectural_state(), before_bank);
    assert_eq!(cache.snapshot(), before_cache);
    assert_eq!(heat.diagnostics_snapshot()[2], 0);
}

#[test]
fn precise_frontier_requires_progress_and_enough_live_work_for_the_whole_span() {
    let (mut instance, mut context, mut bank, cache, source) =
        fixture(&[0x40, 0xeb, 0xfd], 2, 0x1000, 4);
    let mut selected = Some(source.clone());
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut ColdState::default(),
            &mut Unobserved,
        ),
        StopReason::NeedCode
    );
    assert_eq!(context.state().eip, 0x1000);
    assert_eq!(context.state().registers[0], 1);
    assert_eq!((context.retired(), context.remaining()), (2, 2));
    assert_eq!(identity(&context), source.identity());
    context.set_word(44, 1);
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut ColdState::default(),
            &mut Unobserved,
        ),
        StopReason::Budget
    );
    assert_eq!((context.state().eip, context.retired()), (0x1001, 3));
}

#[test]
fn precise_native_handoff_uses_the_decoded_operation_not_owner_ordinal_zero() {
    let (mut instance, mut context, mut bank, cache, source) =
        fixture(&[0xd9, 0xe8, 0xd9, 0xe0, 0x90], 3, 0x1000, 1);
    context.bytes_mut()[120..128].fill(0);
    let segments = bank.segment_context();
    assert_eq!(
        run(
            &mut instance,
            &mut context,
            &mut bank,
            &segments,
            &[],
            &[],
            &mut ColdState::default(),
        ),
        StopReason::Budget
    );
    context.set_word(44, 1);
    context.bytes_mut()[120..128].copy_from_slice(&source.identity().to_le_bytes());
    let mut selected = Some(source.clone());
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut ColdState::default(),
            &mut Unobserved,
        ),
        StopReason::Budget
    );
    let state = bank.architectural_state().0;
    assert_eq!(state.registers[7], [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0xbf]);
    assert_eq!(state.status & 0x3800, 0x3800);
    assert_eq!((context.state().eip, context.retired()), (0x1004, 2));
    assert_eq!(identity(&context), source.identity());
}

#[test]
fn precise_keeps_active_capture_with_exact_completed_native_instructions() {
    let (mut instance, mut context, mut bank, cache, source) =
        fixture(&[0xd9, 0xe8, 0xd9, 0xe0, 0x90], 3, 0x1000, 2);
    let mut capture = Capture::active(&source, 0x1005);
    let before_cache = cache.snapshot();
    let mut selected = Some(source.clone());
    let mut heat = ColdState::default();
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut heat,
            &mut capture,
        ),
        StopReason::Budget
    );
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1004, 2, 0)
    );
    assert_eq!(identity(&context), source.identity());
    assert_eq!(capture.hot, [false, false]);
    assert_eq!(capture.arena.active, 1);
    assert!(capture.pending.is_none());
    assert_eq!(
        capture
            .completed
            .iter()
            .map(|op| (op.pc(), op.actual_post_pc()))
            .collect::<Vec<_>>(),
        [(0x1000, 0x1002), (0x1002, 0x1004)]
    );
    let fp = bank.architectural_state().0;
    assert_eq!(fp.registers[7], [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0xbf]);
    assert_eq!(fp.status, 0x3800);
    assert_eq!(cache.snapshot(), before_cache);
    assert_eq!(heat.diagnostics_snapshot()[2], 0);

    let memory = instance.memory.as_ref().unwrap();
    let observed = compile_observed_with_cold(
        memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        capture.arena.snapshot().unwrap(),
        &[&*source],
        memory.code_source_version(),
        &capture.completed,
        None,
    )
    .unwrap()
    .expect("actual precise completion certifies the observed graph");
    assert!(observed.instruction_pcs().eq([0x1000, 0x1002]));
    assert!(observed.is_current(memory));
}

#[test]
fn precise_fault_keeps_the_completed_prefix_and_held_source_capability() {
    let (mut instance, mut context, mut bank, cache, source) = fixture(&[0x89, 0x03], 1, 0x1000, 1);
    context.set_word(12, 0x5000);
    context.set_word(0, 0x1234_5678);
    let before = context.state();
    let bank_before = bank.architectural_state();
    let mut selected = Some(source.clone());
    let mut capture = Capture::active(&source, 0x1002);
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut ColdState::default(),
            &mut capture,
        ),
        StopReason::MemoryFault
    );
    assert_eq!(context.state(), before);
    assert_eq!((context.retired(), context.remaining()), (0, 1));
    assert_eq!(&context.bytes()[48..52], &0x5000_u32.to_le_bytes());
    assert_eq!(&context.bytes()[52..56], &0x10204_u32.to_le_bytes());
    assert_eq!(identity(&context), source.identity());
    assert_eq!(bank.architectural_state(), bank_before);
    assert_eq!(capture.arena.active, 0);
    assert!(capture.pending.is_none());
    assert!(capture.completed.is_empty());
    assert_eq!(capture.arena.snapshot().unwrap().len(), 1);
}

#[test]
fn precise_smc_retires_the_store_and_revokes_the_stale_source_identity() {
    let (mut instance, mut context, mut bank, cache, source) =
        fixture(&[0xc6, 0x03, 0x90], 1, 0x1000, 1);
    context.set_word(12, 0x1000);
    let stale_identity = source.identity();
    let mut selected = Some(source);
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut ColdState::default(),
            &mut Unobserved,
        ),
        StopReason::CodeInvalidated
    );
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1003, 1, 0)
    );
    assert_eq!(identity(&context), 0);
    assert_eq!(selected.as_ref().unwrap().identity(), stale_identity);
    assert!(!cache.precise_source_current(
        instance.memory.as_ref().unwrap(),
        selected.as_ref().unwrap()
    ));
    assert_eq!(&context.bytes()[112..116], &1_u32.to_le_bytes());
    let mut byte = [0];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x1000), &mut byte)
        .unwrap();
    assert_eq!(byte, [0x90]);
}
