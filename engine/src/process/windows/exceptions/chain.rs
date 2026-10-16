use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::exceptions::ExceptionError32,
};
fn invalid() -> HostError {
    HostError::Exception(ExceptionError32::InvalidChain)
}
impl EngineInstance {
    pub(super) fn exception_chain_entry(
        &self,
        delivery: &Delivery32,
    ) -> Result<Option<(u32, u32, u32)>, HostError> {
        let frame = delivery.chain;
        if frame == u32::MAX {
            return Ok(None);
        }
        if delivery.visited.len() >= 4096 || delivery.visited.contains(&frame) {
            return Err(invalid());
        }
        let (next, handler) = self.exception_registration(delivery, frame)?;
        self.memory()?
            .check_span_after_private_changes(
                GuestAddress(handler),
                1,
                Access::Execute,
                delivery.initial_page_change.as_slice(),
            )
            .map_err(HostError::Memory)?;
        if let Some(module) = self.loaded_windows_modules().find(|module| {
            handler >= module.base()
                && u64::from(handler) < u64::from(module.base()) + u64::from(module.image_size())
        }) {
            if module.dll_characteristics() & 0x400 != 0 {
                return Err(invalid());
            }
            if let Some(config) = module
                .load_config()
                .filter(|config| !config.seh_handlers.is_empty())
                && config
                    .seh_handlers
                    .binary_search(&(handler - module.base()))
                    .is_err()
            {
                return Err(invalid());
            }
        }
        Ok(Some((frame, next, handler)))
    }
    pub(super) fn exception_registration(
        &self,
        delivery: &Delivery32,
        frame: u32,
    ) -> Result<(u32, u32), HostError> {
        if frame & 3 != 0
            || frame < delivery.stack_low
            || frame
                .checked_add(8)
                .is_none_or(|end| end > delivery.stack_high)
        {
            return Err(invalid());
        }
        let mut bytes = [0; 8];
        self.memory()?
            .read_after_private_changes(
                GuestAddress(frame),
                &mut bytes,
                delivery.initial_page_change.as_slice(),
            )
            .map_err(HostError::Memory)?;
        let next = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        if next != u32::MAX && next <= frame {
            return Err(invalid());
        }
        Ok((next, u32::from_le_bytes(bytes[4..].try_into().unwrap())))
    }

    pub(super) fn exception_dispatcher_registration(
        &self,
        delivery: &Delivery32,
        active: ActiveHandler32,
    ) -> Result<(u32, u32), HostError> {
        let mut bytes = [0; 4];
        self.memory()?
            .read(GuestAddress(delivery.dispatcher_address), &mut bytes)
            .map_err(HostError::Memory)?;
        let target = u32::from_le_bytes(bytes);
        if target < active.establisher
            || target == u32::MAX
            || delivery
                .unwind
                .is_some_and(|unwind| unwind.target != 0 && target >= unwind.target)
        {
            return Err(invalid());
        }
        let mut frame = active.establisher;
        for _ in 0..4096 {
            let (next, _) = self.exception_registration(delivery, frame)?;
            if frame == target {
                return Ok((frame, next));
            }
            if next > target {
                return Err(invalid());
            }
            frame = next;
        }
        Err(invalid())
    }
}
