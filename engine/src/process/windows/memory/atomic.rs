use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::EngineInstance,
    windows::memory::MemoryApi32,
};
impl EngineInstance {
    pub(super) fn memory_interlocked(
        &mut self,
        api: MemoryApi32,
        args: &[u32],
    ) -> Result<u32, MemoryError32> {
        let memory = self.memory.as_mut().unwrap();
        let address = GuestAddress(args[0]);
        memory.check_span(address, 4, Access::Write)?;
        let mut bytes = [0; 4];
        memory.read(address, &mut bytes)?;
        let previous = u32::from_le_bytes(bytes);
        let next = match api {
            MemoryApi32::InterlockedIncrement => previous.wrapping_add(1),
            MemoryApi32::InterlockedDecrement => previous.wrapping_sub(1),
            _ => args[1],
        };
        // a provider call executes without guest thread interleaving, and the canonical write invalidates code.
        memory.write(address, &next.to_le_bytes())?;
        Ok(if api == MemoryApi32::InterlockedExchange {
            previous
        } else {
            next
        })
    }
}
