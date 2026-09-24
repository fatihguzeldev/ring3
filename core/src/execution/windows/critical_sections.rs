use std::collections::BTreeMap;

use super::super::Access;
use super::{DispatchError, GuestMemory, Process32, Register32, guest, thread};

const MAX_OBJECTS: usize = 4096;
const SIZE: usize = 24;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Initialize,
    Enter,
    TryEnter,
    Leave,
    Delete,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x34 => Some(Self::Initialize),
            0x38 => Some(Self::Enter),
            0x3c => Some(Self::TryEnter),
            0x60 => Some(Self::Leave),
            0x64 => Some(Self::Delete),
            _ => None,
        }
    }
}

impl Process32 {
    pub(super) fn critical_section(
        &mut self,
        call: Call,
        pointer: u32,
    ) -> Result<(), DispatchError> {
        let actor = self
            .threads
            .id(thread::Teb(self.cpu.fs_base()))
            .ok_or(DispatchError::Unsupported)?;
        if let Some(value) =
            self.critical_sections
                .dispatch(call, pointer, actor, &mut self.memory)?
        {
            self.cpu.set_register(Register32::Eax, value);
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct CriticalSections {
    objects: BTreeMap<u64, Ownership>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Ownership {
    owner: u32,
    depth: u32,
}

impl CriticalSections {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        pointer: u32,
        actor: u32,
        memory: &mut GuestMemory,
    ) -> Result<Option<u32>, DispatchError> {
        let address = u64::from(pointer);
        if matches!(call, Call::Initialize) {
            guest::check(memory, pointer, SIZE, Access::Write)?;
            if self.objects.len() == MAX_OBJECTS
                || self
                    .objects
                    .range(..address + SIZE as u64)
                    .next_back()
                    .is_some_and(|(&start, _)| start + SIZE as u64 > address)
            {
                return Err(DispatchError::Unsupported);
            }
            let unowned = Ownership::default();
            write_object(memory, pointer, representation(unowned))?;
            self.objects.insert(address, unowned);
            return Ok(None);
        }
        let &state = self
            .objects
            .get(&address)
            .ok_or(DispatchError::Unsupported)?;
        let mut words = [0; 6];
        guest::read_words(memory, pointer, &mut words)?;
        if words != representation(state) {
            return Err(DispatchError::Unsupported);
        }
        if matches!(call, Call::TryEnter) && state.depth != 0 && state.owner != actor {
            return Ok(Some(0));
        }
        let next = match call {
            Call::Enter | Call::TryEnter
                if (state.depth == 0 || state.owner == actor) && state.depth < i32::MAX as u32 =>
            {
                state.depth + 1
            }
            Call::Leave if state.depth > 0 && state.owner == actor => state.depth - 1,
            Call::Delete if state.depth == 0 => 0,
            _ => return Err(DispatchError::Unsupported),
        };
        let next = Ownership {
            owner: if next == 0 { 0 } else { actor },
            depth: next,
        };
        let deleting = matches!(call, Call::Delete);
        write_object(
            memory,
            pointer,
            if deleting {
                [0; 6]
            } else {
                representation(next)
            },
        )?;
        if deleting {
            self.objects.remove(&address);
        } else {
            self.objects.insert(address, next);
        }
        Ok(matches!(call, Call::TryEnter).then_some(1))
    }
}

fn representation(state: Ownership) -> [u32; 6] {
    // debug lists and wait semaphores are absent in this immediate-acquisition profile.
    [
        0,
        state.depth.wrapping_sub(1),
        state.depth,
        state.owner,
        0,
        0,
    ]
}

fn write_object(
    memory: &mut GuestMemory,
    pointer: u32,
    words: [u32; 6],
) -> Result<(), DispatchError> {
    guest::check(memory, pointer, SIZE, Access::Write)?;
    let mut bytes = [0; SIZE];
    for (chunk, word) in bytes.chunks_exact_mut(4).zip(words) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    memory.write(u64::from(pointer), &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{PAGE_SIZE, Permissions};

    #[test]
    fn object_cap_is_reclaimed_and_recursion_overflow_is_atomic() {
        let mut memory = GuestMemory::new(25);
        memory
            .map_zeroed(0x1000, PAGE_SIZE * 25, Permissions::READ_WRITE)
            .unwrap();
        let mut sections = CriticalSections::default();
        for index in 0..4096_u32 {
            assert!(
                sections
                    .dispatch(Call::Initialize, 0x1000 + index * 24, 1, &mut memory)
                    .is_ok()
            );
        }
        let extra = 0x1000 + 4096 * 24;
        assert!(matches!(
            sections.dispatch(Call::Initialize, extra, 1, &mut memory),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(sections.objects.len(), MAX_OBJECTS);
        let mut bytes = [1; SIZE];
        memory.read(u64::from(extra), &mut bytes).unwrap();
        assert_eq!(bytes, [0; SIZE]);
        assert!(
            sections
                .dispatch(Call::Delete, 0x1000, 1, &mut memory)
                .is_ok()
        );
        assert!(
            sections
                .dispatch(Call::Initialize, extra, 1, &mut memory)
                .is_ok()
        );
        let depth = Ownership {
            owner: 1,
            depth: i32::MAX as u32,
        };
        sections.objects.insert(u64::from(extra), depth);
        assert!(write_object(&mut memory, extra, representation(depth)).is_ok());
        for call in [Call::Enter, Call::TryEnter] {
            assert!(matches!(
                sections.dispatch(call, extra, 1, &mut memory),
                Err(DispatchError::Unsupported)
            ));
            assert_eq!(sections.objects[&u64::from(extra)], depth);
            let mut words = [0; 6];
            guest::read_words(&memory, extra, &mut words).unwrap();
            assert_eq!(words, representation(depth));
        }
    }
}
