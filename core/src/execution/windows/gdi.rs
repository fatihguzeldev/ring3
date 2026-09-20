use std::collections::BTreeSet;

use super::DispatchError;

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
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0xa0 => Some(Self::Get),
            0xa4 => Some(Self::Release),
            0xa8 => Some(Self::Caps),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Get => 1,
            Self::Release | Self::Caps => 2,
        }
    }
}

pub(super) struct Gdi {
    live: BTreeSet<u32>,
    next: u32,
}

impl Default for Gdi {
    fn default() -> Self {
        Self {
            live: BTreeSet::new(),
            next: FIRST_HANDLE,
        }
    }
}

impl Gdi {
    pub(super) fn dispatch(&mut self, call: Call, arguments: &[u32]) -> Result<u32, DispatchError> {
        match call {
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
        let mut gdi = Gdi {
            next: LAST_HANDLE,
            ..Gdi::default()
        };
        assert!(matches!(gdi.dispatch(Call::Get, &[0]), Ok(LAST_HANDLE)));
        assert!(matches!(gdi.dispatch(Call::Get, &[0]), Ok(0)));
        assert!(matches!(
            gdi.dispatch(Call::Release, &[0, LAST_HANDLE]),
            Ok(1)
        ));
        assert!(matches!(gdi.dispatch(Call::Get, &[0]), Ok(0)));
        assert!(matches!(gdi.dispatch(Call::Caps, &[LAST_HANDLE, 8]), Ok(0)));
    }
}
