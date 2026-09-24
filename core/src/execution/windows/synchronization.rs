use std::collections::BTreeMap;

use super::{DispatchError, GuestMemory, thread};

const MAX_OBJECTS: usize = 4096;
const FIRST_HANDLE: u32 = 0x7200_0004;
const LAST_HANDLE: u32 = 0x72ff_fffc;

#[derive(Clone, Copy)]
pub(super) enum Call {
    CreateMutex,
    Wait,
    ReleaseMutex,
    Close,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x210 => Some(Self::CreateMutex),
            0x214 => Some(Self::Wait),
            0x218 => Some(Self::ReleaseMutex),
            0x21c => Some(Self::Close),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::CreateMutex => 3,
            Self::Wait => 2,
            Self::ReleaseMutex | Self::Close => 1,
        }
    }
}

pub(super) struct Mutexes {
    objects: BTreeMap<u32, Mutex>,
    handles: BTreeMap<u32, u32>,
    next: u32,
}

struct Mutex {
    // a positive depth belongs to the sole guest thread.
    depth: u32,
    handles: usize,
}

impl Default for Mutexes {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            handles: BTreeMap::new(),
            next: FIRST_HANDLE,
        }
    }
}

impl Mutexes {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::CreateMutex) {
            return self.create(arguments, memory);
        }
        let handle = arguments[0];
        if matches!(call, Call::Wait | Call::Close) && handle >= u32::MAX - 1 {
            return Err(DispatchError::Unsupported);
        }
        let Some(&object_id) = self.handles.get(&handle) else {
            return failure(
                memory,
                6,
                if matches!(call, Call::Wait) {
                    u32::MAX
                } else {
                    0
                },
            );
        };
        let object = self.objects.get_mut(&object_id).unwrap();
        match call {
            Call::Wait => {
                let next = object
                    .depth
                    .checked_add(1)
                    .ok_or(DispatchError::Unsupported)?;
                object.depth = next;
                Ok(0)
            }
            Call::ReleaseMutex if object.depth == 0 => failure(memory, 288, 0),
            Call::ReleaseMutex => {
                object.depth -= 1;
                Ok(1)
            }
            Call::Close => {
                self.handles.remove(&handle);
                object.handles -= 1;
                if object.handles == 0 {
                    self.objects.remove(&object_id);
                }
                Ok(1)
            }
            Call::CreateMutex => unreachable!(),
        }
    }

    fn create(
        &mut self,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if arguments[0] != 0 || arguments[2] != 0 {
            return Err(DispatchError::Unsupported);
        }
        if self.handles.len() == MAX_OBJECTS || self.next > LAST_HANDLE {
            return failure(memory, 8, 0);
        }
        thread::set_last_error(memory, 0)?;
        let handle = self.next;
        self.objects.insert(
            handle,
            Mutex {
                depth: u32::from(arguments[1] != 0),
                handles: 1,
            },
        );
        self.handles.insert(handle, handle);
        self.next += 4;
        Ok(handle)
    }
}

fn failure(memory: &mut GuestMemory, error: u32, value: u32) -> Result<u32, DispatchError> {
    thread::set_last_error(memory, error)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_handles_do_not_recycle_an_exhausted_identity_range() {
        let mut memory = GuestMemory::new(1);
        thread::initialize(&mut memory, 0, 0).unwrap();
        let mut mutexes = Mutexes {
            next: LAST_HANDLE,
            ..Mutexes::default()
        };
        assert!(matches!(
            mutexes.dispatch(Call::CreateMutex, &[0, 0, 0], &mut memory),
            Ok(LAST_HANDLE)
        ));
        assert!(matches!(
            mutexes.dispatch(Call::Close, &[LAST_HANDLE], &mut memory),
            Ok(1)
        ));
        assert!(matches!(
            mutexes.dispatch(Call::CreateMutex, &[0, 0, 0], &mut memory),
            Ok(0)
        ));
        assert_eq!(thread::last_error(&memory).unwrap(), 8);
        assert_eq!(mutexes.next, LAST_HANDLE + 4);
        assert!(mutexes.objects.is_empty());
    }

    #[test]
    fn recursion_overflow_preserves_ownership_and_error_state() {
        let mut memory = GuestMemory::new(1);
        thread::initialize(&mut memory, 0, 0).unwrap();
        thread::set_last_error(&mut memory, 77).unwrap();
        let mut mutexes = Mutexes::default();
        mutexes.objects.insert(
            FIRST_HANDLE,
            Mutex {
                depth: u32::MAX,
                handles: 1,
            },
        );
        mutexes.handles.insert(FIRST_HANDLE, FIRST_HANDLE);
        assert!(matches!(
            mutexes.dispatch(Call::Wait, &[FIRST_HANDLE, 0], &mut memory),
            Err(DispatchError::Unsupported)
        ));
        assert_eq!(mutexes.objects.get(&FIRST_HANDLE).unwrap().depth, u32::MAX);
        assert_eq!(thread::last_error(&memory).unwrap(), 77);
        assert!(matches!(
            mutexes.dispatch(Call::ReleaseMutex, &[FIRST_HANDLE], &mut memory),
            Ok(1)
        ));
        assert!(matches!(
            mutexes.dispatch(Call::Wait, &[FIRST_HANDLE, 0], &mut memory),
            Ok(0)
        ));
    }
}
