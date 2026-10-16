use crate::windows::synchronization::EventBinding32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultimediaTimerCallback32 {
    pub timer: u32,
    pub generation: u64,
    pub occurrence: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultimediaTimerTarget32 {
    Function { procedure: u32, user: u32 },
    SetEvent(EventBinding32),
    PulseEvent(EventBinding32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultimediaTimerError32 {
    InvalidParameter,
    Capacity,
    Allocation,
    Exhausted,
    Stale,
}
#[derive(Clone, Copy)]
struct Timer32 {
    id: u32,
    generation: u64,
    target: MultimediaTimerTarget32,
    deadline: u64,
    interval: u64,
    periodic: bool,
    synchronous: bool,
    active: bool,
    callback: Option<MultimediaTimerCallback32>,
}
pub struct PreparedMultimediaTimer32 {
    revision: u64,
    timer: Timer32,
}
impl PreparedMultimediaTimer32 {
    pub fn id(&self) -> u32 {
        self.timer.id
    }
}
#[derive(Clone, Copy)]
pub struct MultimediaTimerDue32 {
    revision: u64,
    pub identity: MultimediaTimerCallback32,
    pub target: MultimediaTimerTarget32,
    pub occurrences: u64,
    deadline: u64,
    pub periodic: bool,
}
pub struct MultimediaTimerState32 {
    timers: Vec<Timer32>,
    limit: usize,
    revision: u64,
    generation: u64,
    occurrence: u64,
    next_id: u32,
}
impl Default for MultimediaTimerState32 {
    fn default() -> Self {
        Self::new(1024)
    }
}
impl MultimediaTimerState32 {
    pub fn new(limit: usize) -> Self {
        Self {
            timers: Vec::new(),
            limit,
            revision: 0,
            generation: 0,
            occurrence: 0,
            next_id: 0,
        }
    }
    pub fn prepare_create(
        &mut self,
        now: u64,
        delay: u32,
        resolution: u32,
        flags: u32,
        target: MultimediaTimerTarget32,
    ) -> Result<PreparedMultimediaTimer32, MultimediaTimerError32> {
        if !(1..=65535).contains(&delay)
            || resolution > 65535
            || flags & !0x131 != 0
            || flags & 0x30 == 0x30
            || matches!(
                target,
                MultimediaTimerTarget32::Function { procedure: 0, .. }
            )
        {
            return Err(MultimediaTimerError32::InvalidParameter);
        }
        let expected = match target {
            MultimediaTimerTarget32::Function { .. } => 0,
            MultimediaTimerTarget32::SetEvent(_) => 0x10,
            MultimediaTimerTarget32::PulseEvent(_) => 0x20,
        };
        if flags & 0x30 != expected {
            return Err(MultimediaTimerError32::InvalidParameter);
        }
        if self.timers.len() == self.limit {
            return Err(MultimediaTimerError32::Capacity);
        }
        self.revision
            .checked_add(1)
            .ok_or(MultimediaTimerError32::Exhausted)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(MultimediaTimerError32::Exhausted)?;
        let interval = u64::from(delay) * 1_000_000;
        let deadline = now
            .checked_add(interval)
            .ok_or(MultimediaTimerError32::Exhausted)?;
        self.timers
            .try_reserve(1)
            .map_err(|_| MultimediaTimerError32::Allocation)?;
        let mut id = self.next_id;
        for _ in 0..=self.timers.len() {
            id = id.wrapping_add(1).max(1);
            if !self.timers.iter().any(|timer| timer.id == id) {
                break;
            }
        }
        Ok(PreparedMultimediaTimer32 {
            revision: self.revision,
            timer: Timer32 {
                id,
                generation,
                target,
                deadline,
                interval,
                periodic: flags & 1 != 0,
                synchronous: flags & 0x100 != 0,
                active: true,
                callback: None,
            },
        })
    }
    pub fn commit_create(&mut self, prepared: PreparedMultimediaTimer32) -> u32 {
        assert_eq!(prepared.revision, self.revision);
        let timer = prepared.timer;
        self.next_id = timer.id;
        self.generation = timer.generation;
        self.revision += 1;
        self.timers.push(timer);
        timer.id
    }
    pub fn deadline(&self) -> Option<u64> {
        self.timers
            .iter()
            .filter(|timer| timer.active && timer.callback.is_none())
            .map(|timer| timer.deadline)
            .min()
    }
    pub fn prepare_due(
        &self,
        now: u64,
    ) -> Result<Option<MultimediaTimerDue32>, MultimediaTimerError32> {
        let Some(timer) = self
            .timers
            .iter()
            .filter(|timer| timer.active && timer.callback.is_none() && timer.deadline <= now)
            .min_by_key(|timer| (timer.deadline, timer.id))
        else {
            return Ok(None);
        };
        self.revision
            .checked_add(1)
            .ok_or(MultimediaTimerError32::Exhausted)?;
        let occurrence = self
            .occurrence
            .checked_add(1)
            .ok_or(MultimediaTimerError32::Exhausted)?;
        let occurrences = if timer.periodic {
            (now - timer.deadline) / timer.interval + 1
        } else {
            1
        };
        let deadline = if timer.periodic {
            timer
                .deadline
                .checked_add(
                    occurrences
                        .checked_mul(timer.interval)
                        .ok_or(MultimediaTimerError32::Exhausted)?,
                )
                .ok_or(MultimediaTimerError32::Exhausted)?
        } else {
            timer.deadline
        };
        Ok(Some(MultimediaTimerDue32 {
            revision: self.revision,
            identity: MultimediaTimerCallback32 {
                timer: timer.id,
                generation: timer.generation,
                occurrence,
            },
            target: timer.target,
            occurrences,
            deadline,
            periodic: timer.periodic,
        }))
    }
    pub fn commit_due(&mut self, due: MultimediaTimerDue32) -> Option<EventBinding32> {
        assert_eq!(due.revision, self.revision);
        let index = self
            .timers
            .iter()
            .position(|timer| {
                timer.id == due.identity.timer && timer.generation == due.identity.generation
            })
            .unwrap();
        self.revision += 1;
        self.occurrence = due.identity.occurrence;
        let timer = &mut self.timers[index];
        timer.deadline = due.deadline;
        timer.active = due.periodic;
        if matches!(due.target, MultimediaTimerTarget32::Function { .. }) {
            timer.callback = Some(due.identity);
            None
        } else if !due.periodic {
            match self.timers.swap_remove(index).target {
                MultimediaTimerTarget32::SetEvent(binding)
                | MultimediaTimerTarget32::PulseEvent(binding) => Some(binding),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }
    pub fn can_kill(
        &self,
        id: u32,
    ) -> Result<(Option<MultimediaTimerCallback32>, Option<EventBinding32>), MultimediaTimerError32>
    {
        let timer = self
            .timers
            .iter()
            .find(|timer| timer.id == id && timer.active)
            .ok_or(MultimediaTimerError32::InvalidParameter)?;
        self.revision
            .checked_add(1)
            .ok_or(MultimediaTimerError32::Exhausted)?;
        Ok((
            if timer.synchronous {
                timer.callback
            } else {
                None
            },
            match timer.target {
                MultimediaTimerTarget32::SetEvent(binding)
                | MultimediaTimerTarget32::PulseEvent(binding) => Some(binding),
                _ => None,
            },
        ))
    }
    pub fn commit_kill(&mut self, id: u32) {
        let index = self
            .timers
            .iter()
            .position(|timer| timer.id == id && timer.active)
            .unwrap();
        self.revision += 1;
        self.timers[index].active = false;
        if self.timers[index].callback.is_none() {
            self.timers.swap_remove(index);
        }
    }
    pub fn callback_pending(&self, identity: MultimediaTimerCallback32) -> bool {
        self.timers
            .iter()
            .any(|timer| timer.callback == Some(identity))
    }
    pub fn finish_callback(
        &mut self,
        identity: MultimediaTimerCallback32,
    ) -> Result<(), MultimediaTimerError32> {
        let index = self
            .timers
            .iter()
            .position(|timer| timer.callback == Some(identity))
            .ok_or(MultimediaTimerError32::Stale)?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(MultimediaTimerError32::Exhausted)?;
        self.timers[index].callback = None;
        if !self.timers[index].active {
            self.timers.swap_remove(index);
        }
        self.revision = revision;
        Ok(())
    }
    pub fn append_bindings(
        &self,
        output: &mut Vec<EventBinding32>,
    ) -> Result<(), MultimediaTimerError32> {
        output
            .try_reserve(self.timers.len())
            .map_err(|_| MultimediaTimerError32::Allocation)?;
        output.extend(self.timers.iter().filter_map(|timer| match timer.target {
            MultimediaTimerTarget32::SetEvent(binding)
            | MultimediaTimerTarget32::PulseEvent(binding) => Some(binding),
            _ => None,
        }));
        Ok(())
    }
    pub fn clear(&mut self) {
        self.timers.clear();
    }
}
