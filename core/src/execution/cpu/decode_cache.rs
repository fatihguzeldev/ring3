use super::{GuestMemory, Instruction, StopReason, decode_bytes};

const CAPACITY: usize = 4096;

#[derive(Clone, Copy)]
struct Entry {
    instruction: Instruction,
    bytes: [u8; 15],
}

pub(in crate::execution) struct DecodeCache {
    entries: Box<[Option<Entry>]>,
}

impl Default for DecodeCache {
    fn default() -> Self {
        Self {
            entries: vec![None; CAPACITY].into_boxed_slice(),
        }
    }
}

impl DecodeCache {
    pub(super) fn decode(
        &mut self,
        memory: &GuestMemory,
        ip: u32,
    ) -> Result<Instruction, StopReason> {
        let slot = &mut self.entries[ip as usize % CAPACITY];
        let mut bytes = [0; 15];
        if let Some(entry) = slot
            && entry.instruction.ip32() == ip
        {
            let length = entry.instruction.len();
            if memory.fetch(u64::from(ip), &mut bytes[..length]).is_ok()
                && bytes[..length] == entry.bytes[..length]
            {
                return Ok(entry.instruction);
            }
        }
        // a shortened replacement may no longer need the cached instruction's second page.
        let instruction = decode_bytes(memory, ip, &mut bytes)?;
        *slot = Some(Entry { instruction, bytes });
        Ok(instruction)
    }
}
