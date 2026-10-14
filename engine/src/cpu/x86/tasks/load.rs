use super::TaskState;

/// execution values loaded from either tss format, separate from record-only fields.
/// word tasks use the ring3 model's deterministic upper-word and fs/gs choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TaskLoadState32 {
    pub eip: u32,
    pub flags: u32,
    pub general: [u32; 8],
    pub selectors: [u16; 6],
    pub ldtr: u16,
    pub task_trap: bool,
}

impl TaskLoadState32 {
    pub(crate) fn from_record(record: TaskState) -> Self {
        match record {
            TaskState::Word(state) => Self {
                eip: u32::from(state.ip),
                flags: u32::from(state.flags),
                // intel leaves gpr upper words unspecified; ring3 chooses zero.
                general: state.general.map(u32::from),
                // no fs/gs fields exist in a word tss; ring3 loads null selectors.
                selectors: [
                    state.selectors[0],
                    state.selectors[1],
                    state.selectors[2],
                    state.selectors[3],
                    0,
                    0,
                ],
                ldtr: state.ldtr,
                task_trap: false,
            },
            TaskState::Dword(state) => Self {
                eip: state.eip,
                flags: state.flags,
                general: state.general,
                selectors: state.selectors,
                ldtr: state.ldtr,
                task_trap: state.task_trap,
            },
        }
    }
}

#[cfg(test)]
#[path = "load/tests.rs"]
mod tests;
