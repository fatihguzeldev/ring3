use std::collections::BTreeSet;

use super::{DispatchError, GuestMemory};

mod brushes;

pub(super) const SCREEN_WIDTH: u32 = 640;
pub(super) const SCREEN_HEIGHT: u32 = 480;
const FIRST_HANDLE: u32 = 0x6000_0000;
const LAST_HANDLE: u32 = 0x6fff_fffc;
const MAX_LIVE: usize = 1024;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Get,
    Release,
    Caps,
    SystemBrush,
    Object,
    Delete,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0xa0 => Some(Self::Get),
            0xa4 => Some(Self::Release),
            0xa8 => Some(Self::Caps),
            0xb0 => Some(Self::SystemBrush),
            0xb4 => Some(Self::Object),
            0xb8 => Some(Self::Delete),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Get | Self::SystemBrush | Self::Delete => 1,
            Self::Release | Self::Caps => 2,
            Self::Object => 3,
        }
    }
}

pub(super) struct Gdi {
    live: BTreeSet<u32>,
    next: u32,
    brushes: brushes::Brushes,
}

impl Default for Gdi {
    fn default() -> Self {
        Self {
            live: BTreeSet::new(),
            next: FIRST_HANDLE,
            brushes: brushes::Brushes::default(),
        }
    }
}

impl Gdi {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::Object | Call::Delete) && self.live.contains(&arguments[0]) {
            return Err(DispatchError::Unsupported);
        }
        match call {
            Call::SystemBrush => self.brushes.system(arguments[0]),
            Call::Object => self
                .brushes
                .get(arguments[0], arguments[1], arguments[2], memory),
            // system-owned brushes survive deletion for the process lifetime.
            Call::Delete => Ok(u32::from(self.brushes.color(arguments[0]).is_some())),
            Call::Get => {
                if arguments[0] != 0 {
                    return Err(DispatchError::Unsupported);
                }
                if self.live.len() == MAX_LIVE || self.next > LAST_HANDLE {
                    return Ok(0);
                }
                let handle = self.next;
                self.live.insert(handle);
                self.next += 4;
                Ok(handle)
            }
            Call::Release => Ok(u32::from(self.live.remove(&arguments[1]))),
            Call::Caps => {
                if !self.live.contains(&arguments[0]) {
                    return Ok(0);
                }
                match arguments[1] {
                    2 | 14 => Ok(1),
                    8 => Ok(SCREEN_WIDTH),
                    10 => Ok(SCREEN_HEIGHT),
                    12 => Ok(32),
                    24 => Ok(u32::MAX),
                    88 | 90 => Ok(96),
                    _ => Err(DispatchError::Unsupported),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_handle_namespace_never_wraps_or_revives_released_contexts() {
        let mut memory = GuestMemory::new(0);
        let mut gdi = Gdi {
            next: LAST_HANDLE,
            ..Gdi::default()
        };
        assert!(matches!(
            gdi.dispatch(Call::Get, &[0], &mut memory),
            Ok(LAST_HANDLE)
        ));
        assert!(matches!(gdi.dispatch(Call::Get, &[0], &mut memory), Ok(0)));
        assert!(matches!(
            gdi.dispatch(Call::Release, &[0, LAST_HANDLE], &mut memory),
            Ok(1)
        ));
        assert!(matches!(gdi.dispatch(Call::Get, &[0], &mut memory), Ok(0)));
        assert!(matches!(
            gdi.dispatch(Call::Caps, &[LAST_HANDLE, 8], &mut memory),
            Ok(0)
        ));
    }
}
