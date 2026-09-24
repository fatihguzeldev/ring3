use std::collections::BTreeMap;

mod entry;
mod scheduling;
pub(super) use entry::{ENTER, RETURN};

use super::{
    API_BASE, Access, Cpu32, DispatchError, GuestMemory, Permissions, Register32, STACK_SIZE,
    callbacks, eh, guest, synchronization::SyncObjects, thread,
};

const MAX_THREADS: u32 = 32;
const SLOT_SIZE: u32 = STACK_SIZE + 4096;
pub(super) const START: u32 = 0x1100_0000;
pub(super) const END: u32 = START + MAX_THREADS * SLOT_SIZE;

#[derive(Default)]
pub(super) struct Threads {
    primary: State,
    children: BTreeMap<u32, Context>,
    schedule: scheduling::Schedule,
}

#[derive(Default)]
pub(super) struct State {
    priority: thread::Priority,
    pub(super) callbacks: callbacks::Callbacks,
    pub(super) exception: Option<eh::Pending>,
}

struct Context {
    runnable: bool,
    id: u32,
    teb: u32,
    cpu: Cpu32,
    state: State,
    entry: entry::Entry,
}

impl Threads {
    pub(super) fn state_mut(&mut self, teb: u32) -> Result<&mut State, DispatchError> {
        if teb == thread::BASE {
            return Ok(&mut self.primary);
        }
        self.children
            .values_mut()
            .find(|context| context.teb == teb)
            .map(|context| &mut context.state)
            .ok_or(DispatchError::Unsupported)
    }

    pub(super) fn priority(
        &mut self,
        call: thread::PriorityCall,
        args: &[u32],
        teb: thread::Teb,
        handles: &SyncObjects,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let priority = if args[0] == u32::MAX - 1 {
            &mut self.state_mut(teb.0)?.priority
        } else if let Some(context) = self
            .children
            .get_mut(&args[0])
            .filter(|_| handles.is_thread(args[0]))
        {
            &mut context.state.priority
        } else {
            teb.set_last_error(memory, 6)?;
            return Ok(if matches!(call, thread::PriorityCall::Get) {
                0x7fff_ffff
            } else {
                0
            });
        };
        priority.dispatch(call, args)
    }

    pub(super) fn id(&self, teb: thread::Teb) -> Option<u32> {
        if teb.0 == thread::BASE {
            return Some(thread::CURRENT_ID);
        }
        self.children
            .values()
            .find(|context| context.teb == teb.0)
            .map(|context| context.id)
    }

    pub(super) fn create_suspended(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
        handles: &mut SyncObjects,
    ) -> Result<(u32, thread::Teb), DispatchError> {
        if args[0] != 0 || args[1] != 0 || args[4] != 4 {
            return Err(DispatchError::Unsupported);
        }
        let slot = u32::try_from(self.children.len()).unwrap();
        if slot == MAX_THREADS || handles.next_handle().is_none() {
            return Err(DispatchError::Unsupported);
        }
        guest::check(memory, args[2], 1, Access::Execute)?;
        if args[5] != 0 {
            guest::check(memory, args[5], 4, Access::Write)?;
        }
        let id = slot + 2;
        let low = START + slot * SLOT_SIZE;
        let high = low + STACK_SIZE;
        memory.map_zeroed(
            u64::from(low),
            u64::from(SLOT_SIZE),
            Permissions::READ_WRITE,
        )?;
        let initialized = (|| {
            thread::initialize_contents(memory, high, id, low, high)?;
            guest::write_word(memory, high - 8, API_BASE + 0x54c)?;
            guest::write_word(memory, high - 4, args[3])?;
            if args[5] != 0 {
                guest::write_word(memory, args[5], id)?;
            }
            Ok::<(), super::MemoryError>(())
        })();
        if let Err(error) = initialized {
            memory.unmap(u64::from(low), u64::from(SLOT_SIZE))?;
            return Err(error.into());
        }
        let mut cpu = Cpu32::new(ENTER);
        cpu.set_register(Register32::Esp, high - 8);
        cpu.set_fs_base(high);
        cpu.set_x87_control_word(0x027f);
        let handle = handles.insert_thread();
        self.children.insert(
            handle,
            Context {
                runnable: false,
                id,
                teb: high,
                cpu,
                state: State::default(),
                entry: entry::Entry::new(args[2], high - 8),
            },
        );
        Ok((handle, thread::Teb(high)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::PAGE_SIZE;
    use super::*;

    #[test]
    fn closed_child_retains_its_unexecuted_cpu_and_independent_stack() {
        let mut memory = GuestMemory::new(40);
        memory
            .map_zeroed(0x4000, PAGE_SIZE, Permissions::READ_EXECUTE)
            .unwrap();
        let mut threads = Threads::default();
        let mut handles = SyncObjects::default();
        for slot in 0..2 {
            let (handle, teb) = threads
                .create_suspended(&[0, 0, 0x4000, slot, 4, 0], &mut memory, &mut handles)
                .unwrap_or_else(|_| panic!("suspended creation failed"));
            let high = START + slot * SLOT_SIZE + STACK_SIZE;
            assert_eq!(teb.0, high);
            let mut expected = Cpu32::new(ENTER);
            expected.set_register(Register32::Esp, high - 8);
            expected.set_fs_base(high);
            expected.set_x87_control_word(0x027f);
            assert_eq!(threads.children[&handle].cpu, expected);
            assert!(matches!(
                handles.dispatch(
                    super::super::synchronization::Call::Close,
                    &[handle],
                    slot + 2,
                    teb,
                    &mut memory
                ),
                Ok(1)
            ));
            assert_eq!(threads.children[&handle].cpu, expected);
            assert_eq!(threads.id(teb), Some(slot + 2));
            threads
                .children
                .get_mut(&handle)
                .unwrap()
                .cpu
                .set_fs_base(0x5000_0000);
            assert_eq!(threads.id(teb), Some(slot + 2));
            assert_eq!(threads.id(thread::Teb(0x5000_0000)), None);
            assert!(threads.state_mut(teb.0).is_ok());
            assert!(threads.state_mut(0x5000_0000).is_err());
            assert!(memory.fetch(u64::from(high - 8), &mut [0]).is_err());
            assert!(memory.fetch(u64::from(high), &mut [0]).is_err());
        }
        assert_eq!(threads.children.len(), 2);
    }
}
