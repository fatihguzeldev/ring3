use super::*;
use crate::memory::{PageRange, Permissions};

struct ExactObserver;

impl ColdObserver for ExactObserver {
    fn exact_cold(&self) -> bool {
        true
    }
    fn begin_instruction(&mut self, _: &AddressSpace, _: &NativeInstruction, _: bool) -> bool {
        true
    }
}

#[test]
fn inactive_optional_jit_reuses_decode_across_exact_ownerless_leases() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0xff, 0xe6])
        .unwrap();
    let mut before_ram = [0; 4096];
    memory.read(GuestAddress(0x1000), &mut before_ram).unwrap();
    let source = memory.snapshot_code(GuestAddress(0x1000), 3).unwrap();
    let version = memory.transaction_version();
    let mut cold = ColdState::default();
    let mut context = Context32::new(State32 {
        registers: [
            0,
            0x1122_3344,
            0x5566_7788,
            0x3000,
            0x4800,
            0x99aa_bbcc,
            0x1000,
            0xddee_ff00,
        ],
        eip: 0x1000,
        eflags: 2,
    });
    context.set_word(44, 256);
    context.set_word(112, 1);
    let mut expected = context.clone();
    expected.set_word(0, 128);
    expected.set_word(36, 0x12);
    expected.set_word(40, 256);
    expected.set_word(44, 0);
    assert!(matches!(
        run_observed_progress(
            &mut memory,
            &mut context,
            &SegmentContext::default(),
            &|_| None,
            &|_, _, _| false,
            &mut cold,
            &mut ExactObserver,
        ),
        ColdOutcome::Stop(StopReason::Budget)
    ));
    assert_eq!(
        context.bytes(),
        expected.bytes(),
        "the first exact cold lease preserves every Context byte"
    );

    context.set_word(36, 3);
    context.set_word(44, 256);
    expected = context.clone();
    expected.set_word(0, 256);
    expected.set_word(36, 0x17);
    expected.set_word(40, 512);
    expected.set_word(44, 0);
    assert!(matches!(
        run_observed_progress(
            &mut memory,
            &mut context,
            &SegmentContext::default(),
            &|_| None,
            &|_, _, _| false,
            &mut cold,
            &mut ExactObserver,
        ),
        ColdOutcome::Stop(StopReason::Budget)
    ));
    assert_eq!(
        context.bytes(),
        expected.bytes(),
        "fresh input flags and registers execute through the same exact base tier"
    );
    let mut after_ram = [0; 4096];
    memory.read(GuestAddress(0x1000), &mut after_ram).unwrap();
    assert_eq!(after_ram, before_ram);
    assert_eq!(memory.transaction_version(), version);
    assert!(memory.is_code_current(&source));
    assert_eq!(
        cold.decode.decode_calls(),
        2,
        "both leases decode only their two distinct source-current instructions"
    );
}

#[test]
fn one_step_qualification_is_consumed_once_before_each_actual_instruction() {
    struct Qualified {
        queries: Vec<(u32, u32, u32)>,
        begins: usize,
    }
    impl ColdObserver for Qualified {
        fn qualify_step(
            &mut self,
            _: &AddressSpace,
            pc: u32,
            progress: ColdProgress,
        ) -> Option<ColdStepQualification> {
            self.queries
                .push((pc, progress.retired, progress.remaining));
            Some(ColdStepQualification {
                frontier: false,
                track_heat: false,
            })
        }
        fn tracks_heat(&self, _: &AddressSpace, _: u32) -> bool {
            panic!("qualified execution must not repeat the hot capability lookup");
        }
        fn begin_instruction(
            &mut self,
            _: &AddressSpace,
            _: &NativeInstruction,
            hot: bool,
        ) -> bool {
            assert!(!hot);
            self.begins += 1;
            true
        }
    }
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0x40, 0x40])
        .unwrap();
    let mut context = Context32::new(State32 {
        registers: [0; 8],
        eip: 0x1000,
        eflags: 3,
    });
    context.set_word(44, 3);
    context.set_word(112, 1);
    let mut expected = context.clone();
    expected.set_word(0, 3);
    expected.set_word(32, 0x1003);
    expected.set_word(36, 7);
    expected.set_word(40, 3);
    expected.set_word(44, 0);
    let mut observer = Qualified {
        queries: Vec::new(),
        begins: 0,
    };
    let mut cold = ColdState::default();
    assert!(matches!(
        run_observed_progress(
            &mut memory,
            &mut context,
            &SegmentContext::default(),
            &|_| None,
            &|_, _, _| panic!("the qualified frontier already owns this exact step"),
            &mut cold,
            &mut observer,
        ),
        ColdOutcome::Stop(StopReason::Budget)
    ));
    assert_eq!(context.bytes(), expected.bytes());
    assert_eq!(
        observer.queries,
        [(0x1000, 0, 3), (0x1001, 1, 2), (0x1002, 2, 1)]
    );
    assert_eq!(observer.begins, 3);
    assert_eq!(cold.decode.decode_calls(), 3);
}
