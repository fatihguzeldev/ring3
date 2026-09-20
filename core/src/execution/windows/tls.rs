use super::{DispatchError, GuestMemory, thread};

#[derive(Clone, Copy)]
pub(super) enum Call {
    Alloc,
    Free,
    Get,
    Set,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x68 => Some(Self::Alloc),
            0x6c => Some(Self::Free),
            0x70 => Some(Self::Get),
            0x74 => Some(Self::Set),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Alloc => 0,
            Self::Free | Self::Get => 1,
            Self::Set => 2,
        }
    }
}

pub(super) struct Tls {
    allocated: u64,
    // values belong to the only guest thread; no teb mirror is exposed.
    values: [u32; 64],
}

impl Default for Tls {
    fn default() -> Self {
        Self {
            allocated: 0,
            values: [0; 64],
        }
    }
}

impl Tls {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if matches!(call, Call::Alloc) {
            let index = self.allocated.trailing_ones();
            if index == 64 {
                thread::set_last_error(memory, 259)?;
                return Ok(u32::MAX);
            }
            self.values[index as usize] = 0;
            self.allocated |= 1_u64 << index;
            return Ok(index);
        }
        let index = arguments[0];
        if index >= 64 || (matches!(call, Call::Free) && self.allocated & (1_u64 << index) == 0) {
            thread::set_last_error(memory, 87)?;
            return Ok(0);
        }
        match call {
            Call::Free => {
                self.values[index as usize] = 0;
                self.allocated &= !(1_u64 << index);
                Ok(1)
            }
            Call::Get => {
                let value = self.values[index as usize];
                thread::set_last_error(memory, 0)?;
                Ok(value)
            }
            Call::Set => {
                self.values[index as usize] = arguments[1];
                Ok(1)
            }
            Call::Alloc => unreachable!(),
        }
    }
}
