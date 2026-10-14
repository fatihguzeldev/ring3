use super::super::detection::ConsumedDetection32;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, WORK_OFFSET, WORK_SIZE},
        x86::{EFLAGS_OFFSET, EIP_OFFSET, EXIT_SIZE, STATE_SIZE, encode_exit_v6},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{EnteredEvent32, FrameInput32},
    },
    process::HostError,
};

pub(super) struct PreparedDetectionState32 {
    input: FrameInput32,
    state: [u8; STATE_SIZE],
    diagnostic: [u8; EXIT_SIZE],
    entered: [u8; EXIT_SIZE],
    restart_entered: [u8; EXIT_SIZE],
    work: [u8; WORK_SIZE],
}

impl PreparedDetectionState32 {
    pub(super) fn new(record: &ConsumedDetection32, arena: &[u8]) -> Result<Self, HostError> {
        let mut entered = [0; EXIT_SIZE];
        encode_exit_v6(
            &ExecutionExit {
                retired: record.retired(),
                reason: ExitReason::NeedCode,
            },
            &mut entered,
        )
        .map_err(|_| HostError::Infrastructure)?;
        let version = u16::from_le_bytes([record.exit()[4], record.exit()[5]]).max(6);
        entered[4..6].copy_from_slice(&version.to_le_bytes());
        let mut restart_entered = entered;
        if let Some(restart) = record.instruction_restart() {
            restart_entered[20..24].copy_from_slice(&restart.retired.to_le_bytes());
        }
        Ok(Self {
            input: record.input(),
            state: *record.state(),
            diagnostic: *record.exit(),
            entered,
            restart_entered,
            work: arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE]
                .try_into()
                .unwrap(),
        })
    }

    pub(super) const fn input(&self) -> FrameInput32 {
        self.input
    }

    pub(super) fn attempted_restart(&mut self, record: &ConsumedDetection32) {
        if let Some(restart) = record.instruction_restart() {
            self.input = restart.attempted_input(record.input());
            self.correct_retirement(restart.retired, restart.work_consumed);
            self.write_input();
        }
    }

    pub(super) fn unattempted_restart(&mut self, record: &ConsumedDetection32) {
        if let Some(restart) = record.instruction_restart() {
            self.input = restart.unattempted_input(record.input());
            self.correct_retirement(restart.retired, restart.work_consumed);
            // neutral authority absence restores the trusted pre-attempt debug shadow too.
            self.state[4..6].copy_from_slice(&3_u16.to_le_bytes());
            self.state[12..16].copy_from_slice(&u32::from(restart.ss_debug_shadow).to_le_bytes());
            self.write_input();
        }
    }

    fn correct_retirement(&mut self, retired: u32, work: Option<u32>) {
        self.diagnostic[20..24].copy_from_slice(&retired.to_le_bytes());
        self.entered = self.restart_entered;
        if let Some(work) = work {
            self.work[16..20].copy_from_slice(&work.to_le_bytes());
        }
    }

    fn write_input(&mut self) {
        self.state[EIP_OFFSET..EIP_OFFSET + 4].copy_from_slice(&self.input.eip.to_le_bytes());
        self.state[EFLAGS_OFFSET..EFLAGS_OFFSET + 4]
            .copy_from_slice(&self.input.flags.to_le_bytes());
    }

    pub(super) fn entered(&mut self, entered: EnteredEvent32) {
        self.state[4..6].copy_from_slice(&3_u16.to_le_bytes());
        self.state[12..16].fill(0);
        for (offset, value) in [
            (32, entered.esp),
            (EIP_OFFSET, entered.target),
            (EFLAGS_OFFSET, entered.flags),
        ] {
            self.state[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        self.diagnostic = self.entered;
    }

    pub(super) fn publish(self, arena: &mut [u8]) {
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&self.state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&self.diagnostic);
        arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE].copy_from_slice(&self.work);
    }
}
