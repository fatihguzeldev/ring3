use super::ColdState;

#[derive(Default)]
pub(super) struct Diagnostics {
    source_resets: u64,
    requested_resets: u64,
    occupied_replacements: u64,
    native_retired: u64,
    requests: [u64; 4],
}

pub(super) enum Request {
    Hot,
    Decode,
    Admission,
    Unsupported,
}

impl Diagnostics {
    pub(super) fn source_reset(&mut self) {
        self.source_resets = self.source_resets.saturating_add(1);
    }

    pub(super) fn occupied_replacement(&mut self) {
        self.occupied_replacements = self.occupied_replacements.saturating_add(1);
    }

    pub(super) fn request(&mut self, request: Request) {
        let index = match request {
            Request::Hot => 0,
            Request::Decode => 1,
            Request::Admission => 2,
            Request::Unsupported => 3,
        };
        self.requested_resets = self.requested_resets.saturating_add(1);
        self.requests[index] = self.requests[index].saturating_add(1);
    }
}

impl ColdState {
    pub(crate) fn diagnostics_snapshot(&self) -> [u64; 13] {
        let mut snapshot = [0; 13];
        snapshot[0] = 1;
        let diagnostics = &self.heat.diagnostics;
        snapshot[1] = diagnostics.source_resets;
        snapshot[2] = diagnostics.requested_resets;
        snapshot[3] = diagnostics.occupied_replacements;
        snapshot[4] = diagnostics.native_retired;
        snapshot[5..9].copy_from_slice(&diagnostics.requests);
        for row in &self.heat.rows {
            if row.generation != self.heat.generation || row.visits == 0 {
                continue;
            }
            let bin = if row.visits == 1 {
                9
            } else if row.visits < super::HOT_VISITS {
                10
            } else {
                11
            };
            snapshot[bin] += 1;
            snapshot[12] = snapshot[12].max(u64::from(row.visits));
        }
        snapshot
    }

    pub(crate) fn native_retired(&mut self) {
        let retired = &mut self.heat.diagnostics.native_retired;
        *retired = retired.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::memory::{PageRange, Permissions};

    fn memory(code: &[u8]) -> AddressSpace {
        let mut memory = AddressSpace::new(4).unwrap();
        for (pc, permissions) in [
            (0x1000, Permissions::ALL),
            (0x3000, Permissions::READ_WRITE),
            (0x4000, Permissions::READ_WRITE),
        ] {
            memory
                .map_zeroed(PageRange::new(GuestAddress(pc), 1).unwrap(), permissions)
                .unwrap();
        }
        memory.write(GuestAddress(0x1000), code).unwrap();
        memory
    }

    fn context(budget: u32) -> Context32 {
        let mut context = Context32::new(State32 {
            registers: [0; 8],
            eip: 0x1000,
            eflags: 2,
        });
        context.set_word(44, budget);
        context.set_word(112, 1);
        context
    }

    fn execute(
        memory: &mut AddressSpace,
        context: &mut Context32,
        cold: &mut ColdState,
    ) -> StopReason {
        match run(memory, context, &SegmentContext::default(), &[], &[], cold) {
            ColdOutcome::Stop(reason) => reason,
            ColdOutcome::NeedNative(_) => panic!("unexpected native operation"),
        }
    }

    #[test]
    fn cold_diagnostics_start_versioned_and_read_without_changing_heat() {
        let cold = ColdState::default();
        let expected = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(cold.diagnostics_snapshot(), expected);
        assert_eq!(cold.diagnostics_snapshot(), expected);
        assert_eq!(cold.heat.generation, 1);
        assert_eq!(cold.heat.memory, 0);
    }

    #[test]
    fn cold_diagnostics_partition_real_jit_request_causes() {
        for (code, cause, source_resets) in [
            (&[0x0f, 0x0b][..], 8, 1),
            (&[0x65, 0x8b, 0x03][..], 7, 0),
            (&[0xf3, 0xa4][..], 8, 1),
        ] {
            let mut memory = memory(code);
            let mut context = context(1);
            let mut cold = ColdState::default();
            let before = *context.bytes();
            assert_eq!(
                execute(&mut memory, &mut context, &mut cold),
                StopReason::NeedCode
            );
            assert_eq!(*context.bytes(), before);
            let mut expected = [0; 13];
            expected[0] = 1;
            expected[1] = source_resets;
            expected[2] = 1;
            expected[cause] = 1;
            assert_eq!(cold.diagnostics_snapshot(), expected);
        }
        let mut memory = memory(&[0x40, 0xeb, 0xfd]);
        let mut context = context(4);
        let mut cold = ColdState::with_threshold(2);
        assert_eq!(
            execute(&mut memory, &mut context, &mut cold),
            StopReason::Budget
        );
        assert_eq!((context.retired(), context.state().registers[0]), (4, 2));
        context.set_word(44, 1);
        let before = *context.bytes();
        assert_eq!(
            execute(&mut memory, &mut context, &mut cold),
            StopReason::NeedCode
        );
        assert_eq!(*context.bytes(), before);
        assert_eq!(
            cold.diagnostics_snapshot(),
            [1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 2]
        );
    }

    #[test]
    fn cold_diagnostics_separate_source_rebinding_live_conflicts_and_current_row_bins() {
        let mut memory = memory(&[0x90]);
        let mut cold = ColdState::default();
        for (pc, visits) in [(0x1000, 1), (0x1001, 3), (0x1002, 64)] {
            for _ in 0..visits {
                cold.heat.retired(&memory, pc);
            }
        }
        let expected = [1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 64];
        assert_eq!(cold.diagnostics_snapshot(), expected);
        memory.write(GuestAddress(0x4000), &[1]).unwrap();
        assert!(cold.heat.hot(&memory, 0x1002));
        assert_eq!(cold.diagnostics_snapshot(), expected);
        memory
            .unmap(PageRange::new(GuestAddress(0x3000), 1).unwrap())
            .unwrap();
        assert!(cold.heat.hot(&memory, 0x1002));
        assert_eq!(cold.diagnostics_snapshot(), expected);
        let mut cold = ColdState::default();
        let sites = [0x1000, 0x3004, 0x5008, 0x700c, 0x9010];
        for pc in sites {
            assert_eq!(ColdHeat::index(pc), ColdHeat::index(sites[0]));
            cold.heat.retired(&memory, pc);
        }
        assert_eq!(
            cold.diagnostics_snapshot(),
            [1, 1, 0, 1, 0, 0, 0, 0, 0, 4, 0, 0, 1]
        );
    }
}
