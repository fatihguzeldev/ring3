use super::{saved::SavedThread32, *};
use crate::{
    abi::{
        arena::*,
        x86::{encode_current_segment_context, encode_exit_v3, encode_state_current},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{SegmentRegister, State32},
    },
    memory::{Access, GuestAddress, PAGE_SIZE, PageInitialization, Permissions, WordWrite32},
    process::{EngineInstance, HostError},
};

pub(in crate::process) const THREAD_RETURN_ID: u32 = 0x000f_ffff;
pub(in crate::process) const INVOCATION_RETURN_ID: u32 = 0x000f_fffe;

impl EngineInstance {
    pub fn configure_threads(&mut self, limits: ThreadLimits32) -> Result<(), HostError> {
        self.memory()?;
        if self.image_started
            || self.pending_call.is_some()
            || self.callback.is_some()
            || self.threads.threads.len() != 1
        {
            return Err(HostError::Thread(ThreadError32::Busy));
        }
        let threads = ThreadRuntime32::new(limits).map_err(HostError::Thread)?;
        if let Some(sync) = self.synchronization.as_mut() {
            sync.context_mut()
                .set_thread_handle_limit(limits.handles as usize)
                .map_err(HostError::Synchronization)?;
        }
        self.threads = threads;
        Ok(())
    }
    pub(super) fn create_windows_thread(&mut self, args: &[u32]) -> Result<u32, HostError> {
        let return_pc = self
            .module_gates()?
            .iter()
            .find(|gate| gate.id == THREAD_RETURN_ID)
            .map(|gate| gate.entry.0)
            .ok_or(HostError::Thread(ThreadError32::Unsupported))?;
        self.create_windows_thread_at_gate(args, return_pc)
    }
    pub(super) fn create_windows_thread_at_gate(
        &mut self,
        args: &[u32],
        return_pc: u32,
    ) -> Result<u32, HostError> {
        self.memory()?
            .check_span(GuestAddress(return_pc), 1, Access::Execute)
            .map_err(HostError::Memory)?;
        if args.len() != 6 || args[4] & !0x10004 != 0 {
            return Err(HostError::Thread(ThreadError32::InvalidParameter));
        }
        let retained = self
            .threads
            .threads
            .iter()
            .filter(|thread| {
                thread.state != ThreadState32::Terminated
                    || thread.id == self.threads.current_id()
                    || self
                        .synchronization
                        .as_ref()
                        .is_some_and(|runtime| runtime.context().has_thread_object(thread.id))
            })
            .count();
        if retained >= self.threads.limits.threads as usize {
            return Err(HostError::Thread(ThreadError32::Capacity));
        }
        let id = self.threads.next_id;
        let created_filetime = self.windows_filetime()?;
        let next = id
            .checked_add(1)
            .ok_or(HostError::Thread(ThreadError32::Exhausted))?;
        let inherit = if args[0] == 0 {
            false
        } else {
            let mut bytes = [0; 12];
            self.memory()?
                .read(GuestAddress(args[0]), &mut bytes)
                .map_err(HostError::Memory)?;
            if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != 12
                || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 0
            {
                return Err(HostError::Thread(ThreadError32::Unsupported));
            }
            u32::from_le_bytes(bytes[8..].try_into().unwrap()) != 0
        };
        let handle = self
            .synchronization()?
            .prepare_thread_handle(id, inherit)
            .map_err(HostError::Synchronization)?;
        let output = if args[5] == 0 {
            None
        } else {
            self.memory()?
                .check_span(GuestAddress(args[5]), 4, Access::Write)
                .map_err(HostError::Memory)?;
            Some(WordWrite32 {
                address: GuestAddress(args[5]),
                value: id,
            })
        };
        let defaults = self
            .loaded_windows_modules()
            .find(|module| !module.is_dll())
            .and_then(|module| module.stack_sizes());
        let default_commit = defaults
            .map_or(self.threads.limits.stack_pages * PAGE_SIZE, |sizes| sizes.1)
            .max(PAGE_SIZE);
        let default_reserve = defaults
            .map_or(
                self.threads.limits.stack_reserve_pages * PAGE_SIZE,
                |sizes| sizes.0,
            )
            .max(default_commit);
        let (commit, reserve) = if args[1] == 0 {
            (default_commit, default_reserve)
        } else if args[4] & 0x10000 != 0 {
            (default_commit, args[1].max(default_commit))
        } else {
            (args[1], default_reserve.max(args[1]))
        };
        let commit = commit.div_ceil(PAGE_SIZE);
        let reserve = reserve
            .checked_add(65535)
            .ok_or(HostError::Thread(ThreadError32::Capacity))?
            / 65536
            * 16;
        let mut templates = Vec::new();
        templates
            .try_reserve_exact(self.loaded_windows_modules().len())
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        let mut slots = 0u32;

        for module in self.loaded_windows_modules() {
            if let Some(tls) = module.tls() {
                let mut word = [0; 4];
                self.memory()?
                    .read(tls.index_address, &mut word)
                    .map_err(HostError::Memory)?;
                let slot = u32::from_le_bytes(word);
                slots = slots.max(
                    slot.checked_add(1)
                        .ok_or(HostError::Thread(ThreadError32::Capacity))?,
                );
                let length = (tls.template.len() as u64)
                    .checked_add(u64::from(tls.zero_fill))
                    .ok_or(HostError::Thread(ThreadError32::Capacity))?;
                let pages = u32::try_from(length.max(1).div_ceil(u64::from(PAGE_SIZE)))
                    .map_err(|_| HostError::Thread(ThreadError32::Capacity))?;
                let mut template = Vec::new();
                template
                    .try_reserve_exact(tls.template.len())
                    .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
                template.extend_from_slice(&tls.template);
                templates.push((module.id(), slot, pages, tls.alignment.max(1), template));
            }
        }
        let array_pages = slots
            .max(self.windows_static_tls_capacity())
            .max(1)
            .checked_mul(4)
            .ok_or(HostError::Thread(ThreadError32::Capacity))?
            .div_ceil(PAGE_SIZE);
        let mut prefix = 1u32
            .checked_add(array_pages)
            .ok_or(HostError::Thread(ThreadError32::Capacity))?;
        for (_, _, pages, alignment, _) in &mut templates {
            let stride = (*alignment).div_ceil(PAGE_SIZE).max(1);
            prefix = prefix
                .div_ceil(stride)
                .checked_mul(stride)
                .and_then(|v| v.checked_add(*pages))
                .ok_or(HostError::Thread(ThreadError32::Capacity))?;
        }
        let total = prefix
            .checked_add(reserve)
            .and_then(|v| v.checked_mul(PAGE_SIZE))
            .ok_or(HostError::Thread(ThreadError32::Capacity))?;
        let guard_pages = u32::from(reserve > commit);
        let mapped = prefix
            .checked_add(commit)
            .and_then(|pages| pages.checked_add(guard_pages))
            .ok_or(HostError::Thread(ThreadError32::Capacity))?;
        if mapped
            > self
                .memory()?
                .capacity_pages()
                .saturating_sub(self.memory()?.mapped_pages())
        {
            return Err(HostError::Thread(ThreadError32::Capacity));
        }
        let mut page_bytes = Vec::new();
        page_bytes
            .try_reserve_exact(mapped as usize)
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        page_bytes.resize_with(mapped as usize, || [0u8; PAGE_SIZE as usize]);
        let external = self.guest_address_reservations()?;
        let base = self
            .windows_memory
            .reserve_thread_storage(self.memory.as_mut().unwrap(), &external, total)
            .map_err(HostError::WindowsMemory)?;
        let reservation = PageRange::new(GuestAddress(base), total / PAGE_SIZE).unwrap();
        let result = (|| {
            let array = base + PAGE_SIZE;
            let stack_end = base + total;
            let stack_base = stack_end - commit * PAGE_SIZE;
            let mapped_stack_base = stack_base - guard_pages * PAGE_SIZE;
            let teb = &mut page_bytes[0];
            for (offset, value) in [
                (0, u32::MAX),
                (4, stack_end),
                (8, stack_base),
                (0x18, base),
                (0x20, 1),
                (0x24, id),
                (0x2c, array),
            ] {
                teb[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            if let Some(primary) = self.module_teb_address() {
                let mut process_id = [0; 4];
                self.memory()?
                    .read(GuestAddress(primary + 0x20), &mut process_id)
                    .map_err(HostError::Memory)?;
                teb[0x20..0x24].copy_from_slice(&process_id);
                let mut peb = [0; 4];
                self.memory()?
                    .read(GuestAddress(primary + 0x30), &mut peb)
                    .map_err(HostError::Memory)?;
                teb[0x30..0x34].copy_from_slice(&peb);
            }
            let mut next_page = 1 + array_pages;
            let mut static_tls = Vec::new();
            static_tls
                .try_reserve_exact(templates.len())
                .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
            for (module, slot, pages, alignment, template) in templates {
                let stride = alignment.div_ceil(PAGE_SIZE).max(1);
                next_page = next_page.div_ceil(stride) * stride;
                let address = base + next_page * PAGE_SIZE;
                let array_offset = slot as usize * 4;
                page_bytes[1 + array_offset / PAGE_SIZE as usize]
                    [array_offset % PAGE_SIZE as usize..array_offset % PAGE_SIZE as usize + 4]
                    .copy_from_slice(&address.to_le_bytes());
                for (offset, byte) in template.iter().enumerate() {
                    page_bytes[next_page as usize + offset / PAGE_SIZE as usize]
                        [offset % PAGE_SIZE as usize] = *byte;
                }
                next_page += pages;
                static_tls.push(super::static_tls::ThreadStaticTls32 {
                    module,
                    slot,
                    range: PageRange::new(GuestAddress(address), pages).unwrap(),
                    external: false,
                    mapping_version: 0,
                });
            }
            let end = (mapped as usize - 1, PAGE_SIZE as usize - 12);
            page_bytes[end.0][end.1..end.1 + 4].copy_from_slice(&return_pc.to_le_bytes());
            page_bytes[end.0][end.1 + 4..end.1 + 8].copy_from_slice(&args[3].to_le_bytes());
            let mut arena = [0; ARENA_SIZE];
            crate::abi::arena::initialize(&mut arena).map_err(|_| HostError::Infrastructure)?;
            let mut state = State32 {
                eip: args[2],
                ..State32::default()
            };
            state.registers[4] = stack_end - 12;
            encode_state_current(
                &state,
                &mut arena[STATE_OFFSET..STATE_OFFSET + crate::abi::x86::STATE_SIZE],
            )
            .map_err(|_| HostError::Infrastructure)?;
            encode_exit_v3(
                &ExecutionExit {
                    retired: 0,
                    reason: ExitReason::NeedCode,
                },
                &mut arena[EXIT_OFFSET..EXIT_OFFSET + crate::abi::x86::EXIT_SIZE],
            )
            .map_err(|_| HostError::Infrastructure)?;
            let mut segments = self.segment_context()?;
            segments.state.bases[SegmentRegister::Fs.index()] = base;
            let (records, bases) = arena.split_at_mut(SEGMENT_BASES_RECORD_OFFSET);
            encode_current_segment_context(
                &segments,
                &mut records[SEGMENTS_OFFSET..SEGMENTS_OFFSET + crate::abi::x86::SEGMENTS_SIZE],
                &mut bases[..crate::abi::x86::SEGMENT_BASES_RECORD_SIZE],
            )
            .map_err(|_| HostError::Infrastructure)?;
            let saved = SavedThread32::empty(self, arena);
            let mut reserved = Vec::new();
            reserved
                .try_reserve_exact(1)
                .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
            reserved.push(reservation);
            let mut pages = Vec::new();
            pages
                .try_reserve_exact(mapped as usize)
                .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
            for (index, bytes) in page_bytes.iter().enumerate() {
                let address = if index < prefix as usize {
                    base + index as u32 * PAGE_SIZE
                } else {
                    mapped_stack_base + (index - prefix as usize) as u32 * PAGE_SIZE
                };
                pages.push(PageInitialization {
                    address: GuestAddress(address),
                    permissions: if guard_pages != 0 && index == prefix as usize {
                        Permissions::READ_WRITE.with_guard()
                    } else {
                        Permissions::READ_WRITE
                    },
                    bytes,
                });
            }
            self.windows_memory
                .install_thread_pages(
                    self.memory.as_mut().unwrap(),
                    base,
                    &pages,
                    output.as_slice(),
                )
                .map_err(HostError::WindowsMemory)?;
            let handle = self
                .synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_object_handle(handle);
            for entry in &mut static_tls {
                entry.mapping_version = self
                    .memory
                    .as_ref()
                    .unwrap()
                    .mapping_version_at(GuestAddress(entry.range.first as u32 * PAGE_SIZE))
                    .expect("child TLS pages were installed atomically");
            }
            let current = self.threads.current_id();
            let sync = self.synchronization.as_ref().unwrap().context();
            self.threads.threads.retain(|thread| {
                thread.state != ThreadState32::Terminated
                    || thread.id == current
                    || sync.has_thread_object(thread.id)
            });
            self.threads.threads.push(ThreadRecord32 {
                id,
                teb: Some(base),
                stack: Some(PageRange::new(GuestAddress(stack_base), commit).unwrap()),
                stack_reserved: Some(
                    PageRange::new(GuestAddress(stack_end - reserve * PAGE_SIZE), reserve).unwrap(),
                ),
                state: if args[4] & 4 != 0 {
                    ThreadState32::Suspended
                } else {
                    ThreadState32::Runnable
                },
                suspend_count: u32::from(args[4] & 4 != 0),
                exit_code: 259,
                reserved,
                message_dispatch: false,
                tls_expansion: None,
                tls_values: Vec::new(),
                tls_array: Some(PageRange::new(GuestAddress(array), array_pages).unwrap()),
                static_tls,
                saved: Some(saved),
                active_invocations: Vec::new(),
                started: false,
                entered: false,
                lifecycle: None,
                exit_requested: None,
                priority: 0,
                created_filetime,
                exited_filetime: 0,
                user_ns: 0,
                duration_epoch: 0,
                context_override: None,
            });
            self.threads.next_id = next;
            Ok(handle)
        })();
        if result.is_err() {
            self.windows_memory.revoke(reservation);
        }
        result
    }
}
