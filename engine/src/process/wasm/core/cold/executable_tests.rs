use super::*;
use crate::cpu::{
    core::{CompileOptions, compile},
    x86::State32,
};

#[derive(Default)]
struct Observations {
    hot: usize,
    completed: usize,
}
impl ColdObserver for Observations {
    fn begin_instruction(
        &mut self,
        _memory: &AddressSpace,
        _instruction: &crate::cpu::core::NativeInstruction,
        hot: bool,
    ) -> bool {
        self.hot += usize::from(hot);
        true
    }
    fn complete_instruction(&mut self, _memory: &AddressSpace, _post: u32) {
        self.completed += 1;
    }
}
fn fixture(
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
    memory
        .write(
            GuestAddress(0x1000),
            &[0xb8, 7, 0, 0, 0, 0x8d, 0x40, 1, 0x8d, 0x40, 2, 0xff, 0xe6],
        )
        .unwrap();
    let code = compile(
        memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 4,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let mut cache = CoreCache32::default();
    let source = cache.admit(memory, code).unwrap();
    let bank = NumericBank::from_instance(&instance).unwrap();
    let mut context = Context32::new(State32 {
        registers: [17, 0, 0, 0x3000, 0x3800, 0, 0x1005, 0],
        eip: pc,
        eflags: 2,
    });
    context.set_word(44, budget);
    context.set_word(112, 1);
    context.bytes_mut()[120..128].copy_from_slice(&source.identity().to_le_bytes());
    (instance, context, bank, cache, source)
}

#[test]
fn owned_non_hot_precise_ingress_records_real_heat_and_exact_completions() {
    let (mut instance, mut context, mut bank, cache, source) = fixture(0x1005, 384);
    let before = bank.architectural_state();
    let mut selected = Some(source.clone());
    let mut heat = ColdState::default();
    let mut observer = Observations::default();
    assert_eq!(
        precise(
            &mut instance,
            &mut context,
            &mut bank,
            &mut selected,
            &cache,
            &mut heat,
            &mut observer
        ),
        StopReason::Budget
    );
    assert_eq!(context.state().registers[0], 401);
    assert_eq!(context.state().eip, 0x1005);
    assert_eq!(context.retired(), 384);
    assert_eq!(context.remaining(), 0);
    assert_eq!(context.state().eflags, 2);
    assert_eq!(bank.architectural_state(), before);
    assert_eq!(observer.completed, 384);
    assert!(
        observer.hot > 0,
        "actual owned interior lacking a hot port can initiate a capture"
    );
    assert_eq!(selected.unwrap().identity(), source.identity());
}

#[test]
fn repeated_short_quota_at_hot_entry_cannot_heat_later_interior_instructions() {
    let (mut instance, mut context, mut bank, cache, source) = fixture(0x1000, 2);
    let mut selected = Some(source.clone());
    let mut heat = ColdState::default();
    let mut observer = Observations::default();
    for _ in 0..256 {
        context.set_word(32, 0x1000);
        context.set_word(44, 2);
        assert_eq!(
            precise(
                &mut instance,
                &mut context,
                &mut bank,
                &mut selected,
                &cache,
                &mut heat,
                &mut observer
            ),
            StopReason::Budget
        );
        assert_eq!(context.state().registers[0], 8);
        assert_eq!(context.state().eip, 0x1008);
    }
    assert_eq!(observer.completed, 512);
    assert_eq!(
        observer.hot, 0,
        "quota fallback does not train a redundant interior cut"
    );
    assert_eq!(cache.snapshot()[0], 1);
    assert_eq!(selected.unwrap().identity(), source.identity());
}

#[test]
fn executable_probe_uses_live_quota_and_preserves_legacy_source_selection() {
    let (mut instance, mut context, _bank, mut cache, source) = fixture(0x1000, 0);
    instance.map(0x2000, 1, 7).unwrap();
    let memory = instance.memory.as_mut().unwrap();
    memory
        .write(GuestAddress(0x2000), &[0x40, 0xff, 0xe6])
        .unwrap();
    let target = cache
        .admit(
            memory,
            compile(
                memory,
                GuestAddress(0x2000),
                CompileOptions {
                    instructions: 2,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&source, 1).unwrap();
    cache.install_link(&target, 2).unwrap();
    assert_eq!(target.hot_span(0x2000).unwrap().instructions, 2);
    let source_bytes = source.wasm().to_vec();
    let target_bytes = target.wasm().to_vec();
    let mut selected = Some(source.clone());

    // the generated dispatcher owns the live quota; Context can be an older snapshot.
    assert_eq!(context.remaining(), 0);
    assert_eq!(
        probe_executable(
            &mut instance,
            &mut context,
            &mut selected,
            &mut cache,
            0x2000,
            4
        ),
        2
    );
    assert_eq!(selected.as_ref().unwrap().identity(), target.identity());
    assert_eq!(context.remaining(), 0);
    assert_eq!(
        u64::from_le_bytes(context.bytes()[120..128].try_into().unwrap()),
        target.identity()
    );

    for remaining in [0, 1] {
        selected = Some(source.clone());
        context.bytes_mut()[120..128].copy_from_slice(&source.identity().to_le_bytes());
        context.set_word(44, 100);
        let before = context.bytes().to_vec();
        assert_eq!(
            probe_executable(
                &mut instance,
                &mut context,
                &mut selected,
                &mut cache,
                0x2000,
                remaining
            ),
            COLD_MARK
        );
        assert_eq!(context.bytes(), before.as_slice());
        assert_eq!(selected.as_ref().unwrap().identity(), source.identity());
    }

    context.set_word(44, 0);
    assert_eq!(
        probe(
            &mut instance,
            &mut context,
            &mut selected,
            &mut cache,
            0x2000
        ),
        2
    );
    assert_eq!(selected.as_ref().unwrap().identity(), target.identity());
    assert_eq!(context.remaining(), 0);
    assert_eq!(source.wasm(), source_bytes);
    assert_eq!(target.wasm(), target_bytes);
    let memory = instance.memory.as_ref().unwrap();
    assert_eq!(cache.lookup_linked(memory, 0x1000).unwrap().1, 1);
    assert_eq!(cache.lookup_linked(memory, 0x2000).unwrap().1, 2);
    assert_eq!(cache.snapshot()[0], 2);
}
