use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::EngineInstance,
    windows::memory::MemoryApi32,
};
impl EngineInstance {
    pub(super) fn memory_probe(
        &self,
        api: MemoryApi32,
        args: &[u32],
    ) -> Result<u32, MemoryError32> {
        use MemoryApi32::*;
        let memory = self.memory.as_ref().unwrap();
        if matches!(api, IsBadStringPtrA | IsBadStringPtrW) {
            let width = if api == IsBadStringPtrW { 2u32 } else { 1 };
            for index in 0..args[1] {
                let Some(at) = index
                    .checked_mul(width)
                    .and_then(|offset| args[0].checked_add(offset))
                else {
                    return Ok(1);
                };
                let mut unit = [0; 2];
                if memory
                    .read(GuestAddress(at), &mut unit[..width as usize])
                    .is_err()
                {
                    return Ok(1);
                }
                if u16::from_le_bytes(unit) == 0 {
                    return Ok(0);
                }
            }
            return Ok(0);
        }
        let (length, access) = match api {
            IsBadCodePtr => (1, Access::Read),
            IsBadWritePtr => (args[1] as usize, Access::Write),
            _ => (args[1] as usize, Access::Read),
        };
        Ok(u32::from(
            memory
                .check_span(GuestAddress(args[0]), length, access)
                .is_err(),
        ))
    }
}
