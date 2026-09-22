use std::collections::BTreeMap;

use super::{DispatchError, GuestMemory, thread};

const MAX_HOOKS: usize = 4096;
const FIRST_HANDLE: u32 = 0x7400_0004;
const LAST_HANDLE: u32 = 0x74ff_fffc;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Install,
    Remove,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x24c => Some(Self::Install),
            0x250 => Some(Self::Remove),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Install => 4,
            Self::Remove => 1,
        }
    }
}

pub(super) struct Hooks {
    // monotonic handles retain registration order; callbacks run only on delivery.
    callbacks: BTreeMap<u32, u32>,
    next: u32,
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            callbacks: BTreeMap::new(),
            next: FIRST_HANDLE,
        }
    }
}

impl Hooks {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::Remove) {
            return if self.callbacks.remove(&arguments[0]).is_some() {
                Ok(1)
            } else {
                failure(memory, 1404)
            };
        }
        if arguments[0] != u32::MAX || arguments[2] != 0 || arguments[3] != thread::CURRENT_ID {
            return Err(DispatchError::Unsupported);
        }
        if arguments[1] == 0 {
            return failure(memory, 1427);
        }
        if self.callbacks.len() == MAX_HOOKS || self.next > LAST_HANDLE {
            return failure(memory, 8);
        }
        let handle = self.next;
        self.callbacks.insert(handle, arguments[1]);
        self.next += 4;
        Ok(handle)
    }
}

fn failure(memory: &mut GuestMemory, error: u32) -> Result<u32, DispatchError> {
    thread::set_last_error(memory, error)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callbacks_retain_reverse_registration_order_and_handles_never_wrap() {
        let mut memory = GuestMemory::new(1);
        thread::initialize(&mut memory, 0, 0).unwrap();
        let mut hooks = Hooks {
            next: LAST_HANDLE - 4,
            ..Hooks::default()
        };
        for callback in [0x0040_1234, 0x3000_5678] {
            assert!(
                hooks
                    .dispatch(Call::Install, &[u32::MAX, callback, 0, 1], &mut memory)
                    .is_ok()
            );
        }
        assert_eq!(
            hooks
                .callbacks
                .iter()
                .rev()
                .map(|(&h, &p)| (h, p))
                .collect::<Vec<_>>(),
            [(LAST_HANDLE, 0x3000_5678), (LAST_HANDLE - 4, 0x0040_1234)]
        );
        assert!(matches!(
            hooks.dispatch(Call::Remove, &[LAST_HANDLE], &mut memory),
            Ok(1)
        ));
        assert!(matches!(
            hooks.dispatch(Call::Install, &[u32::MAX, 1, 0, 1], &mut memory),
            Ok(0)
        ));
        assert_eq!(thread::last_error(&memory).unwrap(), 8);
        assert_eq!(hooks.next, LAST_HANDLE + 4);
        assert_eq!(hooks.callbacks.len(), 1);
    }
}
