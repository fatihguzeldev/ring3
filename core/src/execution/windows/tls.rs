use std::collections::BTreeMap;

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
    // only successfully retained contexts are registered; no teb mirror is exposed.
    values: BTreeMap<u32, [u32; 64]>,
}

impl Default for Tls {
    fn default() -> Self {
        Self {
            allocated: 0,
            values: BTreeMap::from([(thread::BASE, [0; 64])]),
        }
    }
}

impl Tls {
    pub(super) fn register(&mut self, teb: thread::Teb) {
        assert!(self.values.insert(teb.0, [0; 64]).is_none());
    }

    fn clear_slot(&mut self, index: usize) {
        for values in self.values.values_mut() {
            values[index] = 0;
        }
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if !self.values.contains_key(&teb.0) {
            return Err(DispatchError::Unsupported);
        }
        if matches!(call, Call::Alloc) {
            let index = self.allocated.trailing_ones();
            if index == 64 {
                teb.set_last_error(memory, 259)?;
                return Ok(u32::MAX);
            }
            self.clear_slot(index as usize);
            self.allocated |= 1_u64 << index;
            return Ok(index);
        }
        let index = arguments[0];
        if index >= 64 || (matches!(call, Call::Free) && self.allocated & (1_u64 << index) == 0) {
            teb.set_last_error(memory, 87)?;
            return Ok(0);
        }
        match call {
            Call::Free => {
                self.clear_slot(index as usize);
                self.allocated &= !(1_u64 << index);
                Ok(1)
            }
            Call::Get => {
                let value = self.values[&teb.0][index as usize];
                teb.set_last_error(memory, 0)?;
                Ok(value)
            }
            Call::Set => {
                self.values.get_mut(&teb.0).unwrap()[index as usize] = arguments[1];
                Ok(1)
            }
            Call::Alloc => unreachable!(),
        }
    }
}
