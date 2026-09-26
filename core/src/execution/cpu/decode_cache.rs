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
        if let Some(entry) = slot
            && entry.instruction.ip32() == ip
        {
            let length = entry.instruction.len();
            if memory
                .matches_executable(u64::from(ip), &entry.bytes[..length])
                .is_ok_and(|matches| matches)
            {
                return Ok(entry.instruction);
            }
        }
        // a shortened replacement may no longer need the cached instruction's second page.
        let mut bytes = [0; 15];
        let instruction = decode_bytes(memory, ip, &mut bytes)?;
        *slot = Some(Entry { instruction, bytes });
        Ok(instruction)
    }
}
