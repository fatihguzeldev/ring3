use super::types::{
    MAX_EVENTS, MAX_WAIT_OBJECTS, MAX_WAITS, MessageWait32, STATE_WORDS, WaitMode32, WaitResult32,
};
use super::{SynchronizationError32 as Error, SynchronizationLimits32};

pub(super) struct Object32 {
    pub id: u64,
    pub name: Option<Vec<u16>>,
    pub manual: bool,
    pub handles: u32,
    pub bindings: u32,
    pub kind: ObjectKind32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObjectKind32 {
    Event,
    Mutex {
        owner: u32,
        depth: u32,
        abandoned: bool,
    },
    Semaphore {
        count: u32,
        maximum: u32,
    },
    Thread {
        id: u32,
        terminated: bool,
    },
    Timer {
        due: Option<u64>,
        period: u64,
        thread: u32,
        procedure: u32,
        argument: u32,
        generation: u64,
        queued: bool,
    },
}
#[derive(Clone, Copy)]
pub(super) struct Handle32 {
    pub value: u32,
    pub event: usize,
    pub access: u32,
    pub inherit: bool,
}
#[derive(Clone, Copy)]
pub(super) struct Binding32 {
    pub id: u64,
    pub event: usize,
    pub object: u64,
}
#[derive(Clone, Copy)]
pub(super) struct Wait32 {
    pub id: u64,
    pub handles: [u32; MAX_WAIT_OBJECTS],
    pub events: [usize; MAX_WAIT_OBJECTS],
    pub count: usize,
    pub mode: WaitMode32,
    pub deadline: Option<u64>,
    pub alertable: bool,
    pub message: Option<MessageWait32>,
    pub result: Option<WaitResult32>,
    pub thread: u32,
}

#[derive(Clone, Copy, Default)]
pub(super) struct QueueObservation32 {
    pub thread: u32,
    pub current: u32,
    pub new: u32,
    pub generation: u64,
}

pub struct SynchronizationContext32 {
    pub(super) epoch: u64,
    pub(super) revision: u64,
    pub(super) now: u64,
    pub(super) objects: Vec<Option<Object32>>,
    pub(super) handles: Vec<Handle32>,
    pub(super) bindings: Vec<Option<Binding32>>,
    pub(super) waits: Vec<Option<Wait32>>,
    pub(super) handle_limit: usize,
    pub(super) thread_handle_limit: usize,
    pub(super) next_handle: u32,
    pub(super) next_object: u64,
    pub(super) next_binding: u64,
    pub(super) next_wait: u64,
    pub(super) signaled: [u64; STATE_WORDS],
    pub(super) queues: Vec<QueueObservation32>,
    pub(super) apcs: Vec<u32>,
    pub(super) current_thread: u32,
    pub(super) typed_objects: usize,
}
impl SynchronizationContext32 {
    #[cfg(test)]
    pub(crate) fn set_revision_for_test(&mut self, revision: u64) {
        self.revision = revision;
    }
    pub fn new(epoch: u64, limits: SynchronizationLimits32, now_ns: u64) -> Result<Self, Error> {
        if epoch == 0
            || !(1..=MAX_EVENTS as u32).contains(&limits.events)
            || !(1..=65_536).contains(&limits.handles)
            || !(1..=65_536).contains(&limits.bindings)
            || !(1..=MAX_WAITS as u32).contains(&limits.waits)
        {
            return Err(Error::InvalidLimits);
        }
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(limits.handles as usize)
            .map_err(|_| Error::Allocation)?;
        let mut queues = Vec::new();
        queues
            .try_reserve_exact(4096)
            .map_err(|_| Error::Allocation)?;
        let mut apcs = Vec::new();
        apcs.try_reserve_exact(4096)
            .map_err(|_| Error::Allocation)?;
        Ok(Self {
            epoch,
            revision: 0,
            now: now_ns,
            objects: slots(limits.events as usize)?,
            handles,
            bindings: slots(limits.bindings as usize)?,
            waits: slots(limits.waits as usize)?,
            handle_limit: limits.handles as usize,
            thread_handle_limit: limits.handles as usize,
            next_handle: 0x1000_0000,
            next_object: 0,
            next_binding: 0,
            next_wait: 0,
            signaled: [0; STATE_WORDS],
            queues,
            apcs,
            current_thread: 1,
            typed_objects: 0,
        })
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn now_ns(&self) -> u64 {
        self.now
    }
    pub fn reset(&mut self, epoch: u64, now_ns: u64) -> Result<(), Error> {
        if epoch <= self.epoch || now_ns < self.now {
            return Err(Error::Stale);
        }
        self.epoch = epoch;
        self.revision = 0;
        self.now = now_ns;
        self.handles.clear();
        self.objects.iter_mut().for_each(|event| *event = None);
        self.bindings.fill(None);
        self.waits.fill(None);
        self.signaled.fill(0);
        self.queues.clear();
        self.apcs.clear();
        self.current_thread = 1;
        self.typed_objects = 0;
        Ok(())
    }
    pub fn queue_state(&self, thread: u32) -> (u32, u32, u64) {
        self.queues
            .iter()
            .find(|queue| queue.thread == thread)
            .map_or((0, 0, 0), |queue| {
                (queue.current, queue.new, queue.generation)
            })
    }
    pub(super) fn commit_queue(&mut self, queue: QueueObservation32) {
        if let Some(existing) = self
            .queues
            .iter_mut()
            .find(|item| item.thread == queue.thread)
        {
            *existing = queue;
        } else {
            assert!(self.queues.len() < self.queues.capacity());
            self.queues.push(queue);
        }
    }
    pub(super) fn next_revision(&self) -> Result<u64, Error> {
        self.revision.checked_add(1).ok_or(Error::Exhausted)
    }
    pub(super) fn handle_index(&self, value: u32, access: u32) -> Result<usize, Error> {
        let index = self
            .handles
            .binary_search_by_key(&value, |handle| handle.value)
            .map_err(|_| Error::InvalidHandle)?;
        if self.handles[index].access & access != access {
            return Err(Error::AccessDenied);
        }
        Ok(index)
    }
    pub(super) fn handle_value(&self) -> Result<u32, Error> {
        if self.handles.len() == self.handle_limit {
            return Err(Error::Capacity);
        }
        self.next_handle
            .checked_add(4)
            .filter(|value| *value < 0x8000_0000)
            .ok_or(Error::Exhausted)
    }
    pub(super) fn reclaim_event(&mut self, slot: usize) {
        if self.objects[slot]
            .as_ref()
            .is_some_and(|event| event.handles == 0 && event.bindings == 0)
            && self.objects[slot].as_ref().is_some_and(|object| {
                !matches!(object.kind,ObjectKind32::Mutex {owner,..} if owner!=0)
                    && !matches!(
                        object.kind,
                        ObjectKind32::Thread {
                            terminated: false,
                            ..
                        }
                    )
            })
        {
            if self.objects[slot]
                .as_ref()
                .is_some_and(|object| !matches!(object.kind, ObjectKind32::Event))
            {
                self.typed_objects -= 1;
            }
            self.objects[slot] = None;
            set(&mut self.signaled, slot, false);
        }
    }
}
pub(super) fn bit(state: &[u64; STATE_WORDS], slot: usize) -> bool {
    state[slot / 64] & (1 << (slot % 64)) != 0
}
pub(super) fn set(state: &mut [u64; STATE_WORDS], slot: usize, value: bool) {
    let mask = 1 << (slot % 64);
    if value {
        state[slot / 64] |= mask;
    } else {
        state[slot / 64] &= !mask;
    }
}
fn slots<T>(count: usize) -> Result<Vec<Option<T>>, Error> {
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(count)
        .map_err(|_| Error::Allocation)?;
    slots.resize_with(count, || None);
    Ok(slots)
}
