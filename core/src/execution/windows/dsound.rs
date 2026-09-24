use super::{DispatchError, Process32, Register32, callbacks};
use crate::PeImportSymbol;

pub(super) const BASE: u32 = 0x7001_6000;
const DESCRIPTION: &[u8] = b"Primary Sound Driver\0";

#[derive(Clone, Copy)]
pub(super) enum Call {
    EnumerateA,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        (offset == 0x538).then_some(Self::EnumerateA)
    }

    pub(super) fn resolve(symbol: PeImportSymbol<'_>) -> Option<u32> {
        match symbol {
            PeImportSymbol::Ordinal(2)
            | PeImportSymbol::ByName {
                name: "DirectSoundEnumerateA",
                ..
            } => Some(super::API_BASE + 0x538),
            _ => None,
        }
    }

    pub(super) fn arguments() -> usize {
        2
    }
}

pub(super) fn initialize(memory: &mut super::GuestMemory) -> Result<(), super::MemoryError> {
    memory.map_zeroed(
        u64::from(BASE),
        super::PAGE_SIZE,
        super::Permissions::READ_WRITE,
    )?;
    memory.write(u64::from(BASE), DESCRIPTION)?;
    memory.protect(u64::from(BASE), super::PAGE_SIZE, super::Permissions::READ)
}

impl Process32 {
    pub(super) fn enumerate_sound(&mut self, args: &[u32]) -> Result<bool, DispatchError> {
        self.threads
            .state_mut(self.cpu.fs_base())?
            .callbacks
            .check_entry(args[0])?;
        if !self.sound_data_mapped {
            initialize(&mut self.memory)?;
            self.sound_data_mapped = true;
        }
        let stack = self.cpu.register(Register32::Esp);
        self.threads
            .state_mut(self.cpu.fs_base())?
            .callbacks
            .enter(
                &mut self.cpu,
                &mut self.memory,
                callbacks::Frame {
                    stack,
                    caller: stack,
                    cleanup: 12,
                    creation: None,
                    cbt_hook: None,
                    module: None,
                    dialog: None,
                    destroy: None,
                    paint: false,
                    sound_enumeration: true,
                },
                args[0],
                &[
                    0,
                    BASE,
                    BASE + u32::try_from(DESCRIPTION.len()).expect("fixed description") - 1,
                    args[1],
                ],
            )?;
        Ok(true)
    }
}
