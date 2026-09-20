use std::collections::BTreeMap;

use super::super::Access;
use super::{DispatchError, GuestMemory, guest};

const MAX_OBJECTS: usize = 4096;
const SIZE: usize = 24;
const THREAD_ID: u32 = 1;

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

#[derive(Default)]
pub(super) struct CriticalSections {
    // only one guest thread exists; a positive depth identifies its ownership.
    objects: BTreeMap<u64, u32>,
}

impl CriticalSections {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        pointer: u32,
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
            write_object(memory, pointer, representation(0))?;
            self.objects.insert(address, 0);
            return Ok(None);
        }
        let &depth = self
            .objects
            .get(&address)
            .ok_or(DispatchError::Unsupported)?;
        let mut words = [0; 6];
        guest::read_words(memory, pointer, &mut words)?;
        if words != representation(depth) {
            return Err(DispatchError::Unsupported);
        }
        let next = match call {
            Call::Enter | Call::TryEnter if depth < i32::MAX as u32 => depth + 1,
            Call::Leave if depth > 0 => depth - 1,
            Call::Delete if depth == 0 => 0,
            _ => return Err(DispatchError::Unsupported),
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

fn representation(depth: u32) -> [u32; 6] {
    // debug lists and wait semaphores are absent in this single-thread profile.
    [
        0,
        depth.wrapping_sub(1),
        depth,
        if depth == 0 { 0 } else { THREAD_ID },
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
                    .dispatch(Call::Initialize, 0x1000 + index * 24, &mut memory)
                    .is_ok()
            );
        }
        let extra = 0x1000 + 4096 * 24;
        assert!(matches!(
            sections.dispatch(Call::Initialize, extra, &mut memory),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(sections.objects.len(), MAX_OBJECTS);
        let mut bytes = [1; SIZE];
        memory.read(u64::from(extra), &mut bytes).unwrap();
        assert_eq!(bytes, [0; SIZE]);
        assert!(sections.dispatch(Call::Delete, 0x1000, &mut memory).is_ok());
        assert!(
            sections
                .dispatch(Call::Initialize, extra, &mut memory)
                .is_ok()
        );
        let depth = i32::MAX as u32;
        sections.objects.insert(u64::from(extra), depth);
        assert!(write_object(&mut memory, extra, representation(depth)).is_ok());
        for call in [Call::Enter, Call::TryEnter] {
            assert!(matches!(
                sections.dispatch(call, extra, &mut memory),
                Err(DispatchError::Unsupported)
            ));
            assert_eq!(sections.objects[&u64::from(extra)], depth);
            let mut words = [0; 6];
            guest::read_words(&memory, extra, &mut words).unwrap();
            assert_eq!(words, representation(depth));
        }
    }
}
