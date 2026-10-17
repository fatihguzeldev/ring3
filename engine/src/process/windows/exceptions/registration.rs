use super::*;
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::exceptions::{EXCEPTION_NESTED_GATE, EXCEPTION_UNWIND_GATE, ExceptionError32},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HandlerRegistration32 {
    pub address: u32,
    previous: u32,
    handler: u32,
    establisher: u32,
    unwind: bool,
}
impl HandlerRegistration32 {
    pub(super) fn bytes(self) -> [u8; 12] {
        let mut bytes = [0; 12];
        for (index, word) in [self.previous, self.handler, self.establisher]
            .into_iter()
            .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}
impl EngineInstance {
    pub(super) fn prepare_exception_registration(
        &self,
        delivery: &Delivery32,
        establisher: u32,
    ) -> Result<HandlerRegistration32, HostError> {
        let unwind = delivery.phase == ExceptionPhase32::Unwind;
        let id = if unwind {
            EXCEPTION_UNWIND_GATE
        } else {
            EXCEPTION_NESTED_GATE
        };
        let handler = self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
            .find(|gate| gate.id == id)
            .ok_or(HostError::Infrastructure)?
            .entry
            .0;
        let previous = match delivery.chain_write {
            Some(previous) => previous,
            None => {
                let teb = self
                    .current_teb_address()
                    .ok_or(HostError::Infrastructure)?;
                let mut word = [0; 4];
                self.memory()?
                    .read_after_private_changes(
                        GuestAddress(teb),
                        &mut word,
                        delivery.initial_page_change.as_slice(),
                    )
                    .map_err(HostError::Memory)?;
                u32::from_le_bytes(word)
            }
        };
        Ok(HandlerRegistration32 {
            address: delivery.stack + 808,
            previous,
            handler,
            establisher,
            unwind,
        })
    }

    pub(super) fn restore_exception_registration(
        &self,
        delivery: &mut Delivery32,
        registration: HandlerRegistration32,
    ) -> Result<(), HostError> {
        self.check_exception_registration(registration)?;
        let mut bytes = [0; 4];
        self.memory()?
            .read(
                GuestAddress(
                    self.current_teb_address()
                        .ok_or(HostError::Infrastructure)?,
                ),
                &mut bytes,
            )
            .map_err(HostError::Memory)?;
        if u32::from_le_bytes(bytes) != registration.address {
            return Err(HostError::Exception(ExceptionError32::InvalidChain));
        }
        delivery.chain_write = Some(registration.previous);
        Ok(())
    }

    pub(super) fn check_exception_registration(
        &self,
        registration: HandlerRegistration32,
    ) -> Result<(), HostError> {
        let mut bytes = [0; 12];
        self.memory()?
            .read(GuestAddress(registration.address), &mut bytes)
            .map_err(HostError::Memory)?;
        if bytes != registration.bytes() {
            return Err(HostError::Exception(ExceptionError32::InvalidChain));
        }
        Ok(())
    }

    pub(super) fn advance_nested_exception_registration(
        &self,
        delivery: &mut Delivery32,
    ) -> Result<bool, HostError> {
        let Some((parent, registration)) = self
            .exceptions
            .deliveries
            .iter()
            .rev()
            .filter(|parent| parent.thread == delivery.thread)
            .filter_map(|parent| {
                parent
                    .active
                    .filter(|active| !active.registration_popped)
                    .and_then(|active| active.registration)
                    .map(|registration| (parent.id, registration))
            })
            .find(|(_, registration)| registration.address == delivery.chain)
        else {
            return Ok(false);
        };
        self.check_exception_registration(registration)?;
        if delivery.visited.len() >= 4096 || delivery.visited.contains(&registration.address) {
            return Err(HostError::Exception(ExceptionError32::InvalidChain));
        }
        delivery.visited.push(registration.address);
        if delivery.phase == ExceptionPhase32::Unwind {
            if !delivery.popped.contains(&parent) {
                delivery.popped.push(parent);
            }
            let next = if registration.unwind {
                if delivery.unwind.is_some_and(|unwind| {
                    unwind.target != 0 && registration.establisher >= unwind.target
                }) {
                    return Err(HostError::Exception(ExceptionError32::InvalidChain));
                }
                self.exception_registration(delivery, registration.establisher)?
                    .0
            } else {
                registration.previous
            };
            delivery.chain = next;
            delivery.chain_write = Some(next);
        } else {
            if !registration.unwind {
                delivery.record.flags |= 0x10;
                delivery.nested_boundary = Some(
                    delivery
                        .nested_boundary
                        .map_or(registration.establisher, |old| {
                            old.max(registration.establisher)
                        }),
                );
            }
            delivery.chain = registration.previous;
        }
        Ok(true)
    }

    pub(super) fn commit_popped_exception_registrations(&mut self, delivery: &Delivery32) {
        // i386 RtlUnwind changes the registration chain, then resumes its call site.
        // the handler that invoked it can still return through its checked receipt.
        for parent in &mut self.exceptions.deliveries {
            if delivery.popped.contains(&parent.id)
                && let Some(active) = &mut parent.active
            {
                active.registration_popped = true;
            }
        }
    }

    pub(in crate::process) fn retire_popped_windows_exception_returns(&mut self, esp: u32) {
        if self.exceptions.deliveries.is_empty() {
            return;
        }
        let thread = self.threads.current_id();
        self.exceptions
            .deliveries
            .retain(|delivery| delivery.thread != thread || delivery.return_is_live(thread, esp));
    }
}
