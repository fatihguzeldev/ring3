//! Fixed private recording storage shared with generated code during one lease.
//! Rust accesses it only outside generated execution.

pub(crate) const ACTIVE: u64 = 0;
pub(crate) const USED: u64 = 4;
pub(crate) const LIMIT: u64 = 8;
pub(crate) const OVERFLOW: u64 = 12;
pub(crate) const BYPASS_ID: u64 = 16;
pub(crate) const BYPASS_RETIRED: u64 = 24;
pub(crate) const BYPASS_PENDING: u64 = 28;
pub(crate) const EVENTS: u64 = 32;
pub(crate) const EVENT_SIZE: u64 = 24;
pub(crate) const CAPACITY: usize = 129;
pub(crate) const BLOCK: u32 = 1;
pub(crate) const NUMERIC: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct TraceEvent {
    pub kind: u32,
    pub pc: u32,
    pub end: u32,
    pub offset: u32,
    pub owner: u64,
}

#[repr(C)]
pub(crate) struct TraceArena {
    pub active: u32,
    pub used: u32,
    pub limit: u32,
    pub overflow: u32,
    pub bypass_id: u64,
    pub bypass_retired: u32,
    pub bypass_pending: u32,
    pub events: [TraceEvent; CAPACITY],
}

impl Default for TraceArena {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceArena {
    pub const fn new() -> Self {
        Self {
            active: 0,
            used: 0,
            limit: CAPACITY as u32,
            overflow: 0,
            bypass_id: 0,
            bypass_retired: 0,
            bypass_pending: 0,
            events: [TraceEvent {
                kind: 0,
                pc: 0,
                end: 0,
                offset: 0,
                owner: 0,
            }; CAPACITY],
        }
    }

    /// A new capture does not revoke an independently pending side-exit bypass.
    pub fn begin(&mut self) {
        self.active = 1;
        self.used = 0;
        self.limit = CAPACITY as u32;
        self.overflow = 0;
        self.events.fill(TraceEvent::default());
    }

    pub fn deactivate(&mut self) {
        self.active = 0;
    }

    pub fn push(&mut self, event: TraceEvent) -> bool {
        if self.active != 1 || self.overflow != 0 {
            return false;
        }
        if self.limit != CAPACITY as u32 || self.used >= self.limit {
            self.overflow = 1;
            self.active = 0;
            return false;
        }
        self.events[self.used as usize] = event;
        self.used += 1;
        if self.used == CAPACITY as u32 {
            self.active = 0;
        }
        true
    }

    pub fn snapshot(&self) -> Option<&[TraceEvent]> {
        (self.active <= 1
            && self.limit == CAPACITY as u32
            && self.overflow == 0
            && self.used <= self.limit)
            .then(|| &self.events[..self.used as usize])
    }
}

const _: () = {
    assert!(std::mem::size_of::<TraceEvent>() == EVENT_SIZE as usize);
    assert!(std::mem::offset_of!(TraceEvent, owner) == 16);
    assert!(std::mem::offset_of!(TraceArena, active) == ACTIVE as usize);
    assert!(std::mem::offset_of!(TraceArena, used) == USED as usize);
    assert!(std::mem::offset_of!(TraceArena, limit) == LIMIT as usize);
    assert!(std::mem::offset_of!(TraceArena, overflow) == OVERFLOW as usize);
    assert!(std::mem::offset_of!(TraceArena, bypass_id) == BYPASS_ID as usize);
    assert!(std::mem::offset_of!(TraceArena, bypass_retired) == BYPASS_RETIRED as usize);
    assert!(std::mem::offset_of!(TraceArena, bypass_pending) == BYPASS_PENDING as usize);
    assert!(std::mem::offset_of!(TraceArena, events) == EVENTS as usize);
    assert!(std::mem::size_of::<TraceArena>() == EVENTS as usize + EVENT_SIZE as usize * CAPACITY);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_clears_capture_and_preserves_pending_bypass() {
        let mut arena = TraceArena::new();
        arena.bypass_id = 17;
        arena.bypass_retired = 23;
        arena.bypass_pending = 1;
        arena.begin();
        assert!(arena.push(TraceEvent {
            kind: BLOCK,
            owner: 17,
            ..Default::default()
        }));
        arena.deactivate();
        arena.begin();
        assert_eq!(arena.snapshot(), Some(&[][..]));
        assert_eq!(
            (arena.bypass_id, arena.bypass_retired, arena.bypass_pending),
            (17, 23, 1)
        );
        assert_eq!(arena.events[0], TraceEvent::default());
    }

    #[test]
    fn capacity_contains_a_final_witness_and_stops_without_overflow() {
        let mut arena = TraceArena::new();
        arena.begin();
        for _ in 0..CAPACITY {
            assert!(arena.push(TraceEvent::default()));
        }
        assert_eq!(arena.snapshot().unwrap().len(), CAPACITY);
        assert_eq!(arena.active, 0);
        assert!(!arena.push(TraceEvent::default()));
        assert_eq!(arena.overflow, 0);
        assert_eq!(arena.snapshot().unwrap().len(), CAPACITY);
    }

    #[test]
    fn malformed_active_bounds_decline_without_indexing_the_arena() {
        let mut arena = TraceArena::new();
        arena.begin();
        arena.used = u32::MAX;
        assert!(arena.snapshot().is_none());
        assert!(!arena.push(TraceEvent::default()));
        assert_eq!((arena.active, arena.overflow), (0, 1));
        assert!(arena.snapshot().is_none());
    }
}
