use super::*;
use crate::memory::{PageRange, Permissions};
use std::cell::RefCell;

struct Observer {
    stamp: u32,
    completed: Vec<u32>,
    abandoned: usize,
}

impl Observer {
    fn new(stamp: u32) -> Self {
        Self {
            stamp,
            completed: Vec::new(),
            abandoned: 0,
        }
    }
}

impl ColdObserver for Observer {
    fn exact_cold(&self) -> bool {
        true
    }
    fn bypass_token(&self) -> Option<(u64, u32)> {
        Some((23, self.stamp))
    }
    fn begin_instruction(&mut self, _: &AddressSpace, _: &NativeInstruction, _: bool) -> bool {
        true
    }
    fn complete_instruction(&mut self, _: &AddressSpace, pc: u32) {
        self.completed.push(pc);
    }
    fn abandon_instruction(&mut self) {
        self.abandoned += 1;
    }
}

fn memory(code: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for (pc, rights) in [
        (0x1000, Permissions::ALL),
        (0x3000, Permissions::READ_WRITE),
        (0x4000, Permissions::READ_WRITE),
    ] {
        memory
            .map_zeroed(PageRange::new(GuestAddress(pc), 1).unwrap(), rights)
            .unwrap();
    }
    memory.write(GuestAddress(0x1000), code).unwrap();
    memory
        .write(GuestAddress(0x3000), &[0x11, 0x22, 0x33])
        .unwrap();
    memory
}

fn context(stamp: u32, budget: u32) -> Context32 {
    let mut context = Context32::new(State32 {
        registers: [7, 3, 0, 0x3000, 0x4800, 5, 0x3000, 0x4000],
        eip: 0x1000,
        eflags: 2,
    });
    context.set_word(40, stamp);
    context.set_word(44, budget);
    context.set_word(112, 1);
    context
}

fn run(
    memory: &mut AddressSpace,
    context: &mut Context32,
    observer: &mut Observer,
    frontier: &impl Fn(&AddressSpace, u32, u32) -> bool,
) -> StopReason {
    match run_observed(
        memory,
        context,
        &SegmentContext::default(),
        &|_| None,
        frontier,
        &mut ColdState::default(),
        observer,
    ) {
        ColdOutcome::Stop(reason) => reason,
        ColdOutcome::NeedNative(_) => panic!("this fixture contains no canonical native operation"),
    }
}

#[test]
fn frontier_receives_live_completed_retirement_before_the_next_instruction() {
    for stamp in [123, u32::MAX] {
        let mut memory = memory(&[0x90, 0x90]);
        let mut context = context(stamp, 4);
        let mut wanted = context.clone();
        wanted.set_word(32, 0x1001);
        wanted.set_word(40, stamp.wrapping_add(1));
        wanted.set_word(44, 3);
        let version = memory.transaction_version();
        let mut observer = Observer::new(stamp);
        let queries = RefCell::new(Vec::new());
        let frontier = |_: &AddressSpace, pc, retired| {
            queries.borrow_mut().push((pc, retired));
            // the pending identity is hidden only until actual retirement.
            (pc == 0x1000 || pc == 0x1001) && retired != stamp
        };
        assert_eq!(
            run(&mut memory, &mut context, &mut observer, &frontier),
            StopReason::NeedCode
        );
        assert_eq!(*context.bytes(), *wanted.bytes());
        assert_eq!(
            *queries.borrow(),
            [(0x1000, stamp), (0x1001, stamp.wrapping_add(1))]
        );
        assert_eq!(observer.completed, [0x1001]);
        assert_eq!(memory.transaction_version(), version);
    }
}

#[test]
fn zero_quota_does_not_query_a_frontier_or_release_the_pending_identity() {
    let mut memory = memory(&[0x90]);
    let mut context = context(91, 0);
    let wanted = *context.bytes();
    let mut observer = Observer::new(91);
    assert_eq!(
        run(&mut memory, &mut context, &mut observer, &|_, _, _| {
            panic!("a zero-quota lease cannot select another owner")
        }),
        StopReason::Budget
    );
    assert_eq!(*context.bytes(), wanted);
    assert_eq!(observer.bypass_token(), Some((23, 91)));
    assert!(observer.completed.is_empty());
}

#[test]
fn a_fault_before_completion_keeps_the_same_retirement_and_bypass() {
    let mut memory = memory(&[0x8b, 0x06]); // mov eax,[esi]
    let mut context = context(91, 2);
    context.set_word(6 * 4, 0x5000);
    let registers = context.state().registers;
    let mut observer = Observer::new(91);
    let queries = RefCell::new(Vec::new());
    assert_eq!(
        run(
            &mut memory,
            &mut context,
            &mut observer,
            &|_, pc, retired| {
                queries.borrow_mut().push((pc, retired));
                retired != 91
            }
        ),
        StopReason::MemoryFault
    );
    assert_eq!(context.state().registers, registers);
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1000, 91, 2)
    );
    assert_eq!(word(&context, 48), 0x5000);
    assert_eq!(word(&context, 52), 0x10104);
    assert_eq!(word(&context, 116), 0x40e);
    assert_eq!(*queries.borrow(), [(0x1000, 91)]);
    assert_eq!(observer.bypass_token(), Some((23, 91)));
    assert!(observer.completed.is_empty());
    assert_eq!(observer.abandoned, 1);
}

#[test]
fn partial_rep_work_keeps_bypass_until_the_opcode_really_retires() {
    let mut memory = memory(&[0xf3, 0xa4, 0x90]);
    let mut context = context(91, 1);
    let mut observer = Observer::new(91);
    let version = memory.transaction_version();
    let queries = RefCell::new(Vec::new());
    let frontier = |_: &AddressSpace, pc, retired| {
        queries.borrow_mut().push((pc, retired));
        retired != 91
    };
    for completed_elements in 1..=2 {
        context.set_word(44, 1);
        assert_eq!(
            run(&mut memory, &mut context, &mut observer, &frontier),
            StopReason::Budget
        );
        assert_eq!(
            (context.state().eip, context.retired(), context.remaining()),
            (0x1000, 91, 0)
        );
        assert_eq!(context.state().registers[1], 3 - completed_elements);
        assert_eq!(context.state().registers[6], 0x3000 + completed_elements);
        assert_eq!(context.state().registers[7], 0x4000 + completed_elements);
        assert_eq!(observer.bypass_token(), Some((23, 91)));
        assert!(observer.completed.is_empty());
    }
    context.set_word(44, 2);
    assert_eq!(
        run(&mut memory, &mut context, &mut observer, &frontier),
        StopReason::NeedCode
    );
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1002, 92, 1)
    );
    assert_eq!(context.state().registers[1], 0);
    assert_eq!(observer.completed, [0x1002]);
    assert_eq!(
        *queries.borrow(),
        [(0x1000, 91), (0x1000, 91), (0x1000, 91), (0x1002, 92)]
    );
    assert_eq!(memory.transaction_version(), version + 3);
    let mut copied = [0; 3];
    memory.read(GuestAddress(0x4000), &mut copied).unwrap();
    assert_eq!(copied, [0x11, 0x22, 0x33]);
}
