use super::super::DirectInputRuntime32;
use super::PendingEnumeration32;
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageInitialization, Permissions},
    process::{WindowsCallback32, WindowsCallbackProvider32, WindowsProviderOutcome32},
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn start_enumeration(
        &mut self,
        memory: &mut AddressSpace,
        procedure: u32,
        context: u32,
        interface: InterfaceKind32,
        rows: Vec<Vec<u8>>,
    ) -> Result<WindowsProviderOutcome32, H> {
        if procedure == 0 {
            return Err(H::INVALID_PARAM);
        }
        memory.check_span(GuestAddress(procedure), 1, crate::memory::Access::Execute)?;
        if rows.is_empty() {
            return Ok(WindowsProviderOutcome32::Return(0));
        }
        if self.pending.len() == self.pending.capacity() {
            return Err(H::OUT_OF_MEMORY);
        }
        let token = self.sequence.checked_add(1).ok_or(H::OUT_OF_MEMORY)?;
        let range = memory.find_free_range(self.config.scratch, 1, PAGE_SIZE)?;
        let address = GuestAddress(range.first as u32 * PAGE_SIZE);
        memory.install_pages_with_word_writes(
            &[PageInitialization {
                address,
                permissions: Permissions::READ_WRITE,
                bytes: &rows[0],
            }],
            &[],
        )?;
        self.pending.push(PendingEnumeration32 {
            token,
            procedure: GuestAddress(procedure),
            context,
            interface,
            rows,
            cursor: 0,
            scratch: address,
            range,
            mapping: memory.mapping_version_at(address)?,
            memory: memory.identity(),
            semantics: None,
            resuming: false,
            next_row: None,
        });
        self.sequence = token;
        Ok(self.enumeration_callback())
    }
    pub(in crate::process::windows::dinput) fn enumeration_callback(
        &self,
    ) -> WindowsProviderOutcome32 {
        let pending = self.pending.last().unwrap();
        let mut arguments = [0; 16];
        arguments[0] = pending.scratch.0;
        arguments[1] = pending.context;
        let count = if let Some(semantics) = &pending.semantics {
            arguments[1] = semantics.current.unwrap().0;
            arguments[2] = semantics.flags[pending.cursor];
            arguments[3] = (pending.rows.len() - pending.cursor - 1) as u32;
            arguments[4] = pending.context;
            5
        } else {
            2
        };
        WindowsProviderOutcome32::Callback(WindowsCallback32 {
            procedure: pending.procedure,
            arguments,
            count,
            continuation: pending.token,
            provider: WindowsCallbackProvider32::DirectInput,
        })
    }
    pub(in crate::process) fn complete_enumeration(
        &mut self,
        memory: &mut AddressSpace,
        token: u64,
        result: u32,
        sync: Option<&mut crate::windows::synchronization::SynchronizationContext32>,
    ) -> Result<WindowsProviderOutcome32, super::super::DirectInputError32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(super::super::DirectInputError32::InvalidToken)?;
        if memory.identity() != pending.memory
            || memory
                .mapping_version_at(pending.scratch)
                .map_err(super::super::DirectInputError32::Memory)?
                != pending.mapping
            || memory
                .permissions_at(pending.scratch)
                .map_err(super::super::DirectInputError32::Memory)?
                != Permissions::READ_WRITE
        {
            return Err(super::super::DirectInputError32::InvalidRequest);
        }
        if !pending.resuming {
            let next = if result == 0 || pending.cursor + 1 == pending.rows.len() {
                None
            } else {
                Some(pending.cursor + 1)
            };
            let pending = self.pending.last_mut().unwrap();
            pending.resuming = true;
            pending.next_row = next;
        }
        let pending = self.pending.last().unwrap();
        let finished = pending.next_row.is_none();
        if let Some(pointer) = pending
            .semantics
            .as_ref()
            .and_then(|semantics| semantics.current)
        {
            let range = [pending.range];
            let ranges = if finished { &range[..] } else { &[][..] };
            let parent = pending.semantics.as_ref().unwrap().parent;
            let extra = if finished { &[parent][..] } else { &[][..] };
            self.release_with_unmaps(memory, sync, pointer, ranges, extra)
                .map_err(|_| super::super::DirectInputError32::InvalidRequest)?;
            if finished {
                self.pending.pop();
                return Ok(WindowsProviderOutcome32::Return(0));
            }
            self.pending
                .last_mut()
                .unwrap()
                .semantics
                .as_mut()
                .unwrap()
                .current = None;
        }
        let pending = self.pending.last().unwrap();
        let Some(next) = pending.next_row else {
            memory
                .unmap_ranges_with_word_writes(&[pending.range], &[])
                .map_err(super::super::DirectInputError32::Memory)?;
            self.pending.pop();
            return Ok(WindowsProviderOutcome32::Return(0));
        };
        memory
            .write(pending.scratch, &pending.rows[next])
            .map_err(super::super::DirectInputError32::Memory)?;
        self.pending.last_mut().unwrap().cursor = next;
        self.prepare_semantic_device(memory)
            .map_err(|_| super::super::DirectInputError32::Allocation)?;
        let pending = self.pending.last_mut().unwrap();
        pending.resuming = false;
        pending.next_row = None;
        Ok(self.enumeration_callback())
    }
}
