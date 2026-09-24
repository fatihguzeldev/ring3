use std::collections::BTreeMap;

use super::{DispatchError, GuestMemory, MemoryError, thread};

const MAX_HANDLES: usize = 4096;
const FIRST_HANDLE: u32 = 0x7200_0004;
const LAST_HANDLE: u32 = 0x72ff_fffc;

#[derive(Clone, Copy)]
pub(super) enum Call {
    CreateMutex,
    CreateEvent,
    SetEvent,
    ResetEvent,
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
            0x53c => Some(Self::CreateEvent),
            0x540 => Some(Self::SetEvent),
            0x544 => Some(Self::ResetEvent),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::CreateMutex => 3,
            Self::CreateEvent => 4,
            Self::Wait => 2,
            Self::ReleaseMutex | Self::Close | Self::SetEvent | Self::ResetEvent => 1,
        }
    }
}

pub(super) struct SyncObjects {
    objects: BTreeMap<u32, Object>,
    handles: BTreeMap<u32, u32>,
    next: u32,
}

struct Object {
    state: State,
    handles: usize,
    name: Option<Vec<u8>>,
}

enum State {
    // a positive depth belongs to the sole guest thread.
    Mutex { depth: u32 },
    Event { manual_reset: bool, signaled: bool },
    Thread,
}

impl Default for SyncObjects {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            handles: BTreeMap::new(),
            next: FIRST_HANDLE,
        }
    }
}

impl SyncObjects {
    pub(super) fn is_thread(&self, handle: u32) -> bool {
        self.handles
            .get(&handle)
            .is_some_and(|id| matches!(self.objects[id].state, State::Thread))
    }

    pub(super) fn next_handle(&self) -> Option<u32> {
        (self.handles.len() < MAX_HANDLES && self.next <= LAST_HANDLE).then_some(self.next)
    }

    pub(super) fn insert_thread(&mut self) -> u32 {
        let handle = self
            .next_handle()
            .expect("thread creation preflights its handle");
        self.objects.insert(
            handle,
            Object {
                state: State::Thread,
                handles: 1,
                name: None,
            },
        );
        self.handles.insert(handle, handle);
        self.next += 4;
        handle
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::CreateMutex | Call::CreateEvent) {
            return self.create(call, arguments, memory);
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
        match (call, &mut object.state) {
            (Call::Wait, State::Thread) => {
                if arguments[1] == 0 {
                    Ok(258)
                } else {
                    Err(DispatchError::Unsupported)
                }
            }
            (Call::Wait, State::Mutex { depth }) => {
                let next = depth.checked_add(1).ok_or(DispatchError::Unsupported)?;
                *depth = next;
                Ok(0)
            }
            (
                Call::Wait,
                State::Event {
                    manual_reset,
                    signaled,
                },
            ) => {
                if *signaled {
                    if !*manual_reset {
                        *signaled = false;
                    }
                    Ok(0)
                } else if arguments[1] == 0 {
                    Ok(258)
                } else {
                    Err(DispatchError::Unsupported)
                }
            }
            (Call::SetEvent | Call::ResetEvent, State::Event { signaled, .. }) => {
                *signaled = matches!(call, Call::SetEvent);
                Ok(1)
            }
            (Call::ReleaseMutex, State::Mutex { depth: 0 }) => failure(memory, 288, 0),
            (Call::ReleaseMutex, State::Mutex { depth }) => {
                *depth -= 1;
                Ok(1)
            }
            (Call::Close, _) => {
                self.handles.remove(&handle);
                object.handles -= 1;
                if object.handles == 0 {
                    self.objects.remove(&object_id);
                }
                Ok(1)
            }
            (Call::CreateMutex | Call::CreateEvent, _) => unreachable!(),
            _ => failure(memory, 6, 0),
        }
    }

    fn create(
        &mut self,
        call: Call,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if arguments[0] != 0 {
            return Err(DispatchError::Unsupported);
        }
        let (state, name_pointer) = match call {
            Call::CreateEvent => (
                State::Event {
                    manual_reset: arguments[1] != 0,
                    signaled: arguments[2] != 0,
                },
                arguments[3],
            ),
            _ => (
                State::Mutex {
                    depth: u32::from(arguments[1] != 0),
                },
                arguments[2],
            ),
        };
        let name = read_name(memory, name_pointer)?;
        let existing = name.as_ref().and_then(|name| {
            self.objects
                .iter()
                .find(|(_, object)| object.name.as_ref() == Some(name))
                .map(|(&id, _)| id)
        });
        if let Some(id) = existing
            && std::mem::discriminant(&self.objects[&id].state) != std::mem::discriminant(&state)
        {
            return failure(memory, 6, 0);
        }
        if self.next_handle().is_none() {
            return failure(memory, 8, 0);
        }
        thread::set_last_error(memory, if existing.is_some() { 183 } else { 0 })?;
        let handle = self.next;
        let object_id = if let Some(id) = existing {
            self.objects.get_mut(&id).unwrap().handles += 1;
            id
        } else {
            self.objects.insert(
                handle,
                Object {
                    state,
                    handles: 1,
                    name,
                },
            );
            handle
        };
        self.handles.insert(handle, object_id);
        self.next += 4;
        Ok(handle)
    }
}

fn read_name(memory: &GuestMemory, address: u32) -> Result<Option<Vec<u8>>, DispatchError> {
    if address == 0 {
        return Ok(None);
    }
    let mut name = Vec::new();
    for offset in 0..260 {
        let current = address
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(current), &mut byte)?;
        if byte[0] == 0 && !name.is_empty() {
            return Ok(Some(name));
        }
        if byte[0] == 0 || !byte[0].is_ascii() || byte[0] == b'\\' {
            return Err(DispatchError::Unsupported);
        }
        name.push(byte[0]);
    }
    Err(DispatchError::Unsupported)
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
        let mut mutexes = SyncObjects {
            next: LAST_HANDLE,
            ..SyncObjects::default()
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
        let mut mutexes = SyncObjects::default();
        mutexes.objects.insert(
            FIRST_HANDLE,
            Object {
                state: State::Mutex { depth: u32::MAX },
                handles: 1,
                name: None,
            },
        );
        mutexes.handles.insert(FIRST_HANDLE, FIRST_HANDLE);
        assert!(matches!(
            mutexes.dispatch(Call::Wait, &[FIRST_HANDLE, 0], &mut memory),
            Err(DispatchError::Unsupported)
        ));
        assert!(matches!(
            mutexes.objects.get(&FIRST_HANDLE).unwrap().state,
            State::Mutex { depth: u32::MAX }
        ));
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
