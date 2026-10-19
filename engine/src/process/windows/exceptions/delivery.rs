use super::*;
use crate::{
    abi::{arena::*, x86::*},
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::exceptions::{
        EXCEPTION_RETURN_GATE, ExceptionError32,
        context::{CONTEXT_ALL32, Win32Context32},
    },
};
impl EngineInstance {
    pub fn dispatch_resident_windows_exception(
        &mut self,
        key: u64,
        unit: u64,
        epoch: u64,
    ) -> Result<ExceptionDispatch32, HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.cpu_event.as_ref().is_none_or(|event| {
            !matches!(event.owner,
                PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. }
                if id == unit)
        }) {
            return Err(HostError::Exception(ExceptionError32::Stale));
        }
        self.dispatch_windows_exception(key, epoch)
    }

    pub fn dispatch_windows_exception(
        &mut self,
        key: u64,
        epoch: u64,
    ) -> Result<ExceptionDispatch32, HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let event = self
            .cpu_event
            .as_ref()
            .filter(|event| event.epoch == epoch && event.matches_context(self.arena()))
            .ok_or(HostError::Exception(ExceptionError32::Stale))?;
        if self.pending_call.is_some() {
            return Err(HostError::Exception(ExceptionError32::Busy));
        }
        if self.callback.as_ref().map(|callback| callback.token()) != event.callback_token {
            return Err(HostError::Exception(ExceptionError32::Stale));
        }
        let guard = event.guard_page;
        let owner = event.owner;
        if guard.is_some_and(|guard| !guard.is_current(self.memory.as_ref().unwrap())) {
            return Err(HostError::Exception(ExceptionError32::Stale));
        }
        let mut record = super::cpu::windows_cpu_exception_record(event)
            .ok_or(HostError::Exception(ExceptionError32::InvalidParameter))?;
        let original = ContextState32 {
            state: event.state,
            segments: event.segments,
            x87: event.x87,
            simd: event.simd,
            debug: [0; 6],
        };
        let mut stack_overflow = None;
        let thread = self.current_windows_thread_id()?;
        if let Some(guard) = guard {
            let prepared = Self::prepare_completed_call(original.state)?;
            let id = self
                .exceptions
                .next
                .checked_add(1)
                .ok_or(HostError::Exception(ExceptionError32::Exhausted))?;
            match self.grow_windows_stack_guard(guard)? {
                crate::process::windows::threads::StackGuardOutcome32::NotStack => {}
                crate::process::windows::threads::StackGuardOutcome32::Grown => {
                    self.commit_resource_mapping_protection(
                        crate::memory::PageRange::new(guard.address(), 1)
                            .expect("validated guard page"),
                    );
                    self.cpu_event = None;
                    self.exceptions.next = id;
                    self.publish_prepared_call(prepared);
                    return Ok(ExceptionDispatch32 {
                        id,
                        thread,
                        phase: ExceptionPhase32::Resumed,
                        procedure: 0,
                        record: 0,
                        context: 0,
                        return_pc: original.state.eip,
                        return_id: EXCEPTION_RETURN_GATE,
                        code: record.code,
                    });
                }
                crate::process::windows::threads::StackGuardOutcome32::Overflow(receipt) => {
                    record.code = 0xc00000fd;
                    record.count = 0;
                    record.information.fill(0);
                    stack_overflow = Some(receipt);
                }
            }
        }
        self.begin_exception_delivery_with_page_change(
            record,
            original,
            owner,
            Some(epoch),
            None,
            ExceptionInitial32 {
                filter: None,
                page_change: guard.map(crate::memory::GuardPage32::clear),
                stack_overflow,
                last_chance: false,
            },
        )
    }
    pub(in crate::process) fn begin_exception_delivery(
        &mut self,
        record: ExceptionRecord32,
        original: ContextState32,
        owner: PendingOwner,
        event: Option<u64>,
        unwind: Option<Unwind32>,
        filter: Option<Win32Context32>,
    ) -> Result<ExceptionDispatch32, HostError> {
        self.begin_exception_delivery_with_page_change(
            record,
            original,
            owner,
            event,
            unwind,
            ExceptionInitial32 {
                filter,
                page_change: None,
                stack_overflow: None,
                last_chance: false,
            },
        )
    }

    pub(super) fn begin_exception_delivery_with_page_change(
        &mut self,
        record: ExceptionRecord32,
        original: ContextState32,
        owner: PendingOwner,
        event: Option<u64>,
        unwind: Option<Unwind32>,
        initial: ExceptionInitial32,
    ) -> Result<ExceptionDispatch32, HostError> {
        self.revoke_owned_execution();
        let ExceptionInitial32 {
            filter,
            page_change: initial_page_change,
            stack_overflow,
            last_chance,
        } = initial;
        self.memory()?;
        let mut original = original;
        if let Some(unwind) = unwind {
            original.state.registers[0] = unwind.return_value;
        }
        let record_output = self.pending_call.as_ref().and_then(|pending| {
            let crate::cpu::ExitReason::Gate { id } = decode_exit(&pending.exit).ok()?.reason
            else {
                return None;
            };
            use crate::windows::exceptions::ExceptionApi32;
            let arguments = pending.frame.arguments();
            if id == ExceptionApi32::RtlRaiseException.id() {
                Some(arguments[0])
            } else if id == ExceptionApi32::RtlUnwind.id() && arguments[2] != 0 {
                Some(arguments[2])
            } else {
                None
            }
        });
        let original_esp = original.state.registers[4];
        if self
            .exceptions
            .deliveries
            .iter()
            .filter(|delivery| {
                delivery.thread != self.threads.current_id()
                    || delivery.return_is_live(self.threads.current_id(), original_esp)
            })
            .count()
            >= 64
        {
            return Err(HostError::Exception(ExceptionError32::Capacity));
        }
        let thread = self.current_windows_thread_id()?;
        let id = self
            .exceptions
            .next
            .checked_add(1)
            .ok_or(HostError::Exception(ExceptionError32::Exhausted))?;
        self.exceptions
            .deliveries
            .try_reserve(1)
            .map_err(|_| HostError::Infrastructure)?;
        let handlers = if last_chance {
            Vec::new()
        } else {
            self.exceptions
                .handlers
                .snapshot(false)
                .map_err(HostError::Exception)?
        };
        let return_pc = self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
            .find(|gate| gate.id == EXCEPTION_RETURN_GATE)
            .ok_or(HostError::Exception(ExceptionError32::InvalidParameter))?
            .entry
            .0;
        let teb = self
            .current_teb_address()
            .ok_or(HostError::Exception(ExceptionError32::InvalidChain))?;
        let mut tib = [0; 12];
        self.memory()?
            .read_after_private_changes(GuestAddress(teb), &mut tib, initial_page_change.as_slice())
            .map_err(HostError::Memory)?;
        let chain = u32::from_le_bytes(tib[..4].try_into().unwrap());
        let stack_high = u32::from_le_bytes(tib[4..8].try_into().unwrap());
        let stack_low = stack_overflow.as_ref().map_or(
            u32::from_le_bytes(tib[8..].try_into().unwrap()),
            |receipt| receipt.limit().value,
        );
        let current = self.windows_context_state()?;
        let stack = current.state.registers[4]
            .min(original.state.registers[4])
            .checked_sub(832)
            .ok_or(HostError::Exception(ExceptionError32::NoAccess))?
            & !15;
        if stack < stack_low || stack.checked_add(832).is_none_or(|end| end > stack_high) {
            return Err(HostError::Exception(ExceptionError32::NoAccess));
        }
        self.memory()?
            .check_span_after_private_changes(
                GuestAddress(stack),
                832,
                Access::Write,
                initial_page_change.as_slice(),
            )
            .map_err(HostError::Memory)?;
        let mut delivery = Delivery32 {
            id,
            thread,
            owner,
            record,
            original,
            extended: ExtendedCpuState::capture(self.arena())?,
            pending: None,
            event: None,
            phase: if last_chance {
                ExceptionPhase32::Unhandled
            } else if filter.is_some() {
                ExceptionPhase32::Filter
            } else if unwind.is_some() {
                ExceptionPhase32::Unwind
            } else {
                ExceptionPhase32::Vectored
            },
            handlers,
            cursor: 0,
            chain,
            nested_boundary: None,
            popped: Vec::new(),
            visited: Vec::new(),
            stack_low,
            stack_high,
            record_address: if unwind.is_some() {
                record_output.unwrap_or(stack)
            } else {
                stack
            },
            context_address: stack + 80,
            pointers_address: stack + 796,
            dispatcher_address: stack + 804,
            stack,
            return_pc,
            active: None,
            unwind,
            filter_return: filter.is_some(),
            chain_write: None,
            record_output,
            initial_page_change,
            initial_stack_limit: stack_overflow.as_ref().map(|receipt| receipt.limit()),
        };
        delivery
            .popped
            .try_reserve_exact(64)
            .map_err(|_| HostError::Infrastructure)?;
        delivery
            .visited
            .try_reserve_exact(4096)
            .map_err(|_| HostError::Infrastructure)?;
        let context = match filter {
            Some(context) => context,
            None => Win32Context32::capture(CONTEXT_ALL32, &original)
                .map_err(|_| HostError::Exception(ExceptionError32::InvalidContext))?,
        };
        let mut bytes = [0; 832];
        bytes[..80].copy_from_slice(&record.bytes());
        bytes[80..796].copy_from_slice(context.bytes());
        bytes[796..800].copy_from_slice(&delivery.record_address.to_le_bytes());
        bytes[800..804].copy_from_slice(&delivery.context_address.to_le_bytes());
        // Selection and all guest writes preflight together before publishing the delivery.
        delivery.pending = self.pending_call.take();
        if let Err(error) = self.advance_exception_delivery(&mut delivery, Some(&bytes)) {
            self.pending_call = delivery.pending.take();
            return Err(error);
        }
        if let Some(crate::memory::PrivatePageChange32::Protect { address, .. }) =
            initial_page_change
        {
            self.commit_resource_mapping_protection(
                crate::memory::PageRange::new(address, 1).expect("validated guard page"),
            );
        }
        if let Some(receipt) = stack_overflow {
            self.threads.commit_stack_overflow(receipt);
        }
        self.commit_popped_exception_registrations(&delivery);
        self.retire_popped_windows_exception_returns(original_esp);
        if delivery.phase == ExceptionPhase32::Resumed {
            return Ok(ExceptionDispatch32 {
                id,
                thread,
                phase: ExceptionPhase32::Resumed,
                procedure: 0,
                record: delivery.record_address,
                context: delivery.context_address,
                return_pc,
                return_id: EXCEPTION_RETURN_GATE,
                code: record.code,
            });
        }
        if let Some(epoch) = event {
            delivery.event = self.cpu_event.take().filter(|event| event.epoch == epoch);
        }
        self.exceptions.next = id;
        self.exceptions.deliveries.push(delivery);
        self.pending_windows_exception()
            .ok_or(HostError::Infrastructure)
    }
    pub fn pending_windows_exception(&self) -> Option<ExceptionDispatch32> {
        let current = self.threads.current_id();
        let esp = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .ok()?
            .registers[4];
        let delivery = self
            .exceptions
            .deliveries
            .iter()
            .rev()
            .find(|delivery| delivery.return_is_live(current, esp))?;
        Some(ExceptionDispatch32 {
            id: delivery.id,
            thread: delivery.thread,
            phase: delivery.phase,
            procedure: delivery.active.map_or(0, |active| active.procedure),
            record: delivery.record_address,
            context: delivery.context_address,
            return_pc: delivery.return_pc,
            return_id: EXCEPTION_RETURN_GATE,
            code: delivery.record.code,
        })
    }
    pub fn finish_windows_exception_handler(
        &mut self,
        key: u64,
        id: u64,
    ) -> Result<Option<ExceptionDispatch32>, HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.pending_call.is_some() {
            return Err(HostError::Exception(ExceptionError32::Busy));
        }
        let thread = self.threads.current_id();
        let state = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        let index = self
            .exceptions
            .deliveries
            .iter()
            .rposition(|delivery| delivery.return_is_live(thread, state.registers[4]))
            .filter(|&index| self.exceptions.deliveries[index].id == id)
            .ok_or(HostError::Exception(ExceptionError32::Stale))?;
        let delivery = &self.exceptions.deliveries[index];
        let active = delivery
            .active
            .ok_or(HostError::Exception(ExceptionError32::InvalidDisposition))?;
        let exit = decode_exit(&self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        if state.eip != delivery.return_pc
            || state.registers[4] != active.return_esp
            || exit.reason
                != (crate::cpu::ExitReason::Gate {
                    id: EXCEPTION_RETURN_GATE,
                })
        {
            return Err(HostError::Exception(ExceptionError32::Stale));
        }
        let result = state.registers[0];
        let mut handlers = Vec::new();
        handlers
            .try_reserve_exact(delivery.handlers.len())
            .map_err(|_| HostError::Infrastructure)?;
        handlers.extend_from_slice(&delivery.handlers);
        let before = (
            delivery.phase,
            delivery.cursor,
            delivery.chain,
            delivery.visited.len(),
            delivery.active,
            delivery.chain_write,
            delivery.record,
            delivery.nested_boundary,
            delivery.popped.len(),
        );
        self.revoke_owned_execution();
        let mut delivery = self.exceptions.deliveries.remove(index);
        let outcome = self.complete_exception_handler(&mut delivery, result);
        match outcome {
            Ok(true) => {
                self.commit_popped_exception_registrations(&delivery);
                Ok(None)
            }
            Ok(false) => {
                self.commit_popped_exception_registrations(&delivery);
                let index = self
                    .exceptions
                    .deliveries
                    .partition_point(|entry| entry.id < delivery.id);
                self.exceptions.deliveries.insert(index, delivery);
                Ok(self.pending_windows_exception())
            }
            Err(error) => {
                (
                    delivery.phase,
                    delivery.cursor,
                    delivery.chain,
                    _,
                    delivery.active,
                    delivery.chain_write,
                    delivery.record,
                    delivery.nested_boundary,
                    _,
                ) = before;
                delivery.visited.truncate(before.3);
                delivery.popped.truncate(before.8);
                delivery.handlers = handlers;
                self.exceptions.deliveries.insert(index, delivery);
                Err(error)
            }
        }
    }
}
