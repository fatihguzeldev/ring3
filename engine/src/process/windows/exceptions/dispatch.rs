use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::{
        CallbackFrame32,
        exceptions::{
            ExceptionError32,
            context::{CONTEXT_SIZE32, Win32Context32},
        },
    },
};
impl EngineInstance {
    pub(super) fn advance_exception_delivery(
        &mut self,
        delivery: &mut Delivery32,
        initial: Option<&[u8]>,
    ) -> Result<(), HostError> {
        loop {
            let next = match delivery.phase {
                ExceptionPhase32::Vectored | ExceptionPhase32::Continue => {
                    let mut procedure = None;
                    while delivery.cursor < delivery.handlers.len() {
                        let id = delivery.handlers[delivery.cursor];
                        delivery.cursor += 1;
                        if let Some(entry) = self
                            .exceptions
                            .handlers
                            .procedure(delivery.phase == ExceptionPhase32::Continue, id)
                        {
                            procedure = Some((entry, 0, 0, false));
                            break;
                        }
                    }
                    procedure
                }
                ExceptionPhase32::Structured | ExceptionPhase32::Unwind => {
                    if delivery
                        .unwind
                        .is_some_and(|unwind| unwind.target == delivery.chain)
                    {
                        return self.finish_exception_unwind(delivery, initial);
                    }
                    if self.advance_nested_exception_registration(delivery)? {
                        continue;
                    }
                    self.exception_chain_entry(delivery)?
                        .map(|(frame, next, procedure)| (procedure, frame, next, true))
                }
                ExceptionPhase32::Filter => {
                    let procedure = self.exceptions.handlers.filter();
                    if procedure == 0 {
                        None
                    } else {
                        Some((procedure, 0, 0, false))
                    }
                }
                ExceptionPhase32::Unhandled | ExceptionPhase32::Resumed => None,
            };
            if let Some((procedure, establisher, next, cdecl)) = next {
                self.memory()?
                    .check_span_after_private_changes(
                        GuestAddress(procedure),
                        1,
                        Access::Execute,
                        delivery.initial_page_change.as_slice(),
                    )
                    .map_err(HostError::Memory)?;
                let mut state = delivery.original.state;
                state.registers[4] = delivery.stack;
                let args: &[u32] = if cdecl {
                    &[
                        delivery.record_address,
                        establisher,
                        delivery.context_address,
                        delivery.dispatcher_address,
                    ]
                } else {
                    &[delivery.pointers_address]
                };
                let frame = CallbackFrame32::prepare(state, procedure, delivery.return_pc, args)
                    .map_err(|_| HostError::Exception(ExceptionError32::NoAccess))?;
                let prepared = Self::prepare_completed_call(*frame.state())?;
                let mut words = [0u8; 68];
                for (word, write) in words.chunks_exact_mut(4).zip(frame.writes()) {
                    word.copy_from_slice(&write.value.to_le_bytes());
                }
                let registration = if cdecl {
                    Some(self.prepare_exception_registration(delivery, establisher)?)
                } else {
                    None
                };
                let record_bytes = delivery.record.bytes();
                let registration_bytes = registration.map(|registration| registration.bytes());
                let mut payload = [0u8; 832];
                if let Some(initial) = initial {
                    payload.copy_from_slice(initial);
                    payload[..80].copy_from_slice(&record_bytes);
                    if let Some(bytes) = &registration_bytes {
                        payload[808..820].copy_from_slice(bytes);
                    }
                }
                let mut spans = [(GuestAddress(0), &[][..]); 7];
                spans[0] = (
                    frame.writes()[0].address,
                    &words[..frame.writes().len() * 4],
                );
                let mut count = 1;
                if initial.is_some() {
                    spans[count] = (GuestAddress(delivery.stack), &payload);
                    count += 1;
                }
                if initial.is_none() {
                    spans[count] = (GuestAddress(delivery.record_address), &record_bytes);
                    count += 1;
                }
                if initial.is_some()
                    && let Some(address) = delivery.record_output
                {
                    spans[count] = (GuestAddress(address), record_bytes.as_slice());
                    count += 1;
                }
                if initial.is_none()
                    && let (Some(registration), Some(bytes)) = (registration, &registration_bytes)
                {
                    spans[count] = (GuestAddress(registration.address), bytes);
                    count += 1;
                }
                let chain = registration
                    .map(|registration| registration.address)
                    .or(delivery.chain_write);
                let chain_word = chain.unwrap_or(0).to_le_bytes();
                if chain.is_some() {
                    spans[count] = (
                        GuestAddress(
                            self.current_teb_address()
                                .ok_or(HostError::Infrastructure)?,
                        ),
                        &chain_word,
                    );
                    count += 1;
                }
                let stack_limit = delivery
                    .initial_stack_limit
                    .map_or(0, |word| word.value)
                    .to_le_bytes();
                if let Some(word) = delivery.initial_stack_limit {
                    spans[count] = (word.address, &stack_limit);
                    count += 1;
                }
                self.memory
                    .as_mut()
                    .unwrap()
                    .apply_private_pages(delivery.initial_page_change.as_slice(), &spans[..count])
                    .map_err(HostError::Memory)?;
                delivery.initial_page_change = None;
                delivery.initial_stack_limit = None;
                delivery.chain_write = None;
                delivery.active = Some(ActiveHandler32 {
                    procedure,
                    return_esp: frame.return_esp() - if cdecl { args.len() as u32 * 4 } else { 0 },
                    establisher,
                    next,
                    registration,
                    registration_popped: false,
                    registration_relink: None,
                });
                self.publish_prepared_call(prepared);
                return Ok(());
            }
            match delivery.phase {
                ExceptionPhase32::Vectored => {
                    delivery.phase = ExceptionPhase32::Structured;
                    delivery.cursor = 0;
                }
                ExceptionPhase32::Structured => delivery.phase = ExceptionPhase32::Filter,
                ExceptionPhase32::Continue => return self.resume_exception_context(delivery),
                ExceptionPhase32::Unwind => return self.finish_exception_unwind(delivery, initial),
                _ => {
                    delivery.phase = ExceptionPhase32::Unhandled;
                    delivery.active = None;
                    if initial.is_none() {
                        self.publish_exception_chain_return(delivery)?;
                    }
                    if let Some(bytes) = initial {
                        let record = delivery.record.bytes();
                        let mut spans = [(GuestAddress(0), &[][..]); 3];
                        spans[0] = (GuestAddress(delivery.stack), bytes);
                        let mut count = 1;
                        if let Some(address) = delivery.record_output {
                            spans[count] = (GuestAddress(address), &record);
                            count += 1;
                        }
                        let stack_limit = delivery
                            .initial_stack_limit
                            .map_or(0, |word| word.value)
                            .to_le_bytes();
                        if let Some(word) = delivery.initial_stack_limit {
                            spans[count] = (word.address, &stack_limit);
                            count += 1;
                        }
                        self.memory
                            .as_mut()
                            .unwrap()
                            .apply_private_pages(
                                delivery.initial_page_change.as_slice(),
                                &spans[..count],
                            )
                            .map_err(HostError::Memory)?;
                        delivery.initial_page_change = None;
                        delivery.initial_stack_limit = None;
                    }
                    return Ok(());
                }
            }
        }
    }
    pub(super) fn complete_exception_handler(
        &mut self,
        delivery: &mut Delivery32,
        result: u32,
    ) -> Result<bool, HostError> {
        let active = delivery
            .active
            .ok_or(HostError::Exception(ExceptionError32::Stale))?;
        if let Some(registration) = active.registration {
            if active.registration_popped {
                self.restore_popped_exception_registration(delivery, active, registration)?;
            } else {
                self.restore_exception_registration(delivery, registration)?;
            }
        }
        delivery.record = self.read_exception_record(delivery.record_address)?;
        if delivery.nested_boundary == Some(active.establisher) {
            delivery.record.flags &= !0x10;
            delivery.nested_boundary = None;
        }
        if delivery.filter_return {
            let mut state = delivery.original;
            state.state.registers[0] = result;
            let prepared = Self::prepare_completed_call(state.state)?;
            delivery.extended.restore(self.arena.as_mut().get_mut());
            self.publish_windows_context(&state)?;
            self.publish_prepared_call(prepared);
            delivery.active = None;
            delivery.phase = ExceptionPhase32::Resumed;
            return Ok(true);
        }
        let continuing = match delivery.phase {
            ExceptionPhase32::Vectored | ExceptionPhase32::Filter => match result {
                u32::MAX => true,
                0 => false,
                1 if delivery.phase == ExceptionPhase32::Filter => false,
                _ => return Err(HostError::Exception(ExceptionError32::InvalidDisposition)),
            },
            ExceptionPhase32::Structured => match result {
                0 => true,
                1 => false,
                2 => {
                    let (boundary, _) = self.exception_dispatcher_registration(delivery, active)?;
                    delivery.nested_boundary = Some(
                        delivery
                            .nested_boundary
                            .map_or(boundary, |old| old.max(boundary)),
                    );
                    delivery.record.flags |= 0x10;
                    false
                }
                _ => return Err(HostError::Exception(ExceptionError32::InvalidDisposition)),
            },
            ExceptionPhase32::Continue => match result {
                u32::MAX => {
                    self.resume_exception_context(delivery)?;
                    return Ok(true);
                }
                0 => false,
                _ => return Err(HostError::Exception(ExceptionError32::InvalidDisposition)),
            },
            ExceptionPhase32::Unwind => {
                let next = match result {
                    1 => active.next,
                    3 => self.exception_dispatcher_registration(delivery, active)?.1,
                    _ => return Err(HostError::Exception(ExceptionError32::InvalidDisposition)),
                };
                delivery.chain_write = Some(next);
                false
            }
            ExceptionPhase32::Unhandled | ExceptionPhase32::Resumed => {
                return Err(HostError::Exception(ExceptionError32::InvalidDisposition));
            }
        };
        if continuing {
            if delivery.record.flags & 1 != 0 {
                return Err(HostError::Exception(ExceptionError32::Noncontinuable));
            }
            let handlers = self
                .exceptions
                .handlers
                .snapshot(true)
                .map_err(HostError::Exception)?;
            delivery.handlers = handlers;
            delivery.cursor = 0;
            delivery.phase = ExceptionPhase32::Continue;
        } else if matches!(
            delivery.phase,
            ExceptionPhase32::Structured | ExceptionPhase32::Unwind
        ) {
            delivery.visited.push(active.establisher);
            delivery.chain = if delivery.phase == ExceptionPhase32::Unwind {
                delivery.chain_write.unwrap_or(active.next)
            } else {
                active.next
            };
        } else if delivery.phase == ExceptionPhase32::Filter {
            delivery.phase = ExceptionPhase32::Unhandled;
        }
        delivery.active = None;
        self.advance_exception_delivery(delivery, None)?;
        Ok(delivery.phase == ExceptionPhase32::Resumed)
    }
    fn resume_exception_context(&mut self, delivery: &mut Delivery32) -> Result<(), HostError> {
        let mut bytes = [0; CONTEXT_SIZE32];
        self.memory()?
            .read(GuestAddress(delivery.context_address), &mut bytes)
            .map_err(HostError::Memory)?;
        let context = Win32Context32::from_bytes(&bytes)
            .map_err(|_| HostError::Exception(ExceptionError32::InvalidContext))?;
        let state = self.apply_windows_context(&context, &delivery.original)?;
        let prepared = Self::prepare_completed_call(state.state)?;
        self.publish_exception_chain_return(delivery)?;
        delivery.extended.restore(self.arena.as_mut().get_mut());
        self.publish_windows_context(&state)?;
        self.publish_prepared_call(prepared);
        delivery.phase = ExceptionPhase32::Resumed;
        Ok(())
    }
    fn finish_exception_unwind(
        &mut self,
        delivery: &mut Delivery32,
        initial: Option<&[u8]>,
    ) -> Result<(), HostError> {
        let unwind = delivery.unwind.ok_or(HostError::Infrastructure)?;
        if unwind.target != 0 && delivery.chain != unwind.target {
            return Err(HostError::Exception(ExceptionError32::InvalidChain));
        }
        let mut bytes = [0; CONTEXT_SIZE32];
        if let Some(initial) = initial {
            bytes.copy_from_slice(&initial[80..796]);
        } else {
            self.memory()?
                .read(GuestAddress(delivery.context_address), &mut bytes)
                .map_err(HostError::Memory)?;
        }
        let context = Win32Context32::from_bytes(&bytes)
            .map_err(|_| HostError::Exception(ExceptionError32::InvalidContext))?;
        let state = self.apply_windows_context(&context, &delivery.original)?;
        let prepared = Self::prepare_completed_call(state.state)?;
        self.publish_exception_chain_return(delivery)?;
        delivery.extended.restore(self.arena.as_mut().get_mut());
        self.publish_windows_context(&state)?;
        self.publish_prepared_call(prepared);
        delivery.chain_write = None;
        delivery.phase = ExceptionPhase32::Resumed;
        Ok(())
    }
    fn publish_exception_chain_return(
        &mut self,
        delivery: &mut Delivery32,
    ) -> Result<(), HostError> {
        if delivery.chain_write.is_some() || delivery.record_output.is_some() {
            let record = delivery.record.bytes();
            let chain = delivery.chain_write.unwrap_or(0).to_le_bytes();
            let mut spans = [(GuestAddress(delivery.record_address), record.as_slice()); 2];
            let mut count = 1;
            if delivery.chain_write.is_some() {
                let teb = self
                    .current_teb_address()
                    .ok_or(HostError::Infrastructure)?;
                spans[count] = (GuestAddress(teb), chain.as_slice());
                count += 1;
            }
            self.memory
                .as_mut()
                .unwrap()
                .write_spans(&spans[..count])
                .map_err(HostError::Memory)?;
            delivery.chain_write = None;
        }
        Ok(())
    }
}
