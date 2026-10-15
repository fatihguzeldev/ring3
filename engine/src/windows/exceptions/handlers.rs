#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionError32 {
    InvalidParameter,
    NoAccess,
    Capacity,
    Allocation,
    Exhausted,
    Busy,
    Stale,
    InvalidContext,
    InvalidDisposition,
    Noncontinuable,
    InvalidChain,
}
impl ExceptionError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::NoAccess => 998,
            Self::Capacity | Self::Allocation | Self::Exhausted => 8,
            Self::Busy => 170,
            _ => 87,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandlerId32(pub u32);
#[derive(Clone, Copy)]
struct Handler32 {
    id: HandlerId32,
    procedure: u32,
}
pub struct HandlerRegistry32 {
    exceptions: Vec<Handler32>,
    continuations: Vec<Handler32>,
    next: u32,
    maximum: usize,
    filter: u32,
}
impl HandlerRegistry32 {
    pub fn new(maximum: usize) -> Result<Self, ExceptionError32> {
        if maximum == 0 || maximum > 65536 {
            return Err(ExceptionError32::InvalidParameter);
        }
        Ok(Self {
            exceptions: Vec::new(),
            continuations: Vec::new(),
            next: 0x40000001,
            maximum,
            filter: 0,
        })
    }
    pub fn add(
        &mut self,
        continuation: bool,
        first: bool,
        procedure: u32,
    ) -> Result<HandlerId32, ExceptionError32> {
        if procedure == 0 {
            return Err(ExceptionError32::InvalidParameter);
        }
        if self.exceptions.len() + self.continuations.len() >= self.maximum {
            return Err(ExceptionError32::Capacity);
        }
        let next = self
            .next
            .checked_add(1)
            .ok_or(ExceptionError32::Exhausted)?;
        let entries = if continuation {
            &mut self.continuations
        } else {
            &mut self.exceptions
        };
        entries
            .try_reserve(1)
            .map_err(|_| ExceptionError32::Allocation)?;
        let id = HandlerId32(self.next);
        let handler = Handler32 { id, procedure };
        if first {
            entries.insert(0, handler);
        } else {
            entries.push(handler);
        }
        self.next = next;
        Ok(id)
    }
    pub fn remove(&mut self, continuation: bool, id: HandlerId32) -> bool {
        let entries = if continuation {
            &mut self.continuations
        } else {
            &mut self.exceptions
        };
        if let Some(index) = entries.iter().position(|entry| entry.id == id) {
            entries.remove(index);
            true
        } else {
            false
        }
    }
    pub fn snapshot(&self, continuation: bool) -> Result<Vec<HandlerId32>, ExceptionError32> {
        let entries = if continuation {
            &self.continuations
        } else {
            &self.exceptions
        };
        let mut result = Vec::new();
        result
            .try_reserve_exact(entries.len())
            .map_err(|_| ExceptionError32::Allocation)?;
        result.extend(entries.iter().map(|entry| entry.id));
        Ok(result)
    }
    pub fn procedure(&self, continuation: bool, id: HandlerId32) -> Option<u32> {
        let entries = if continuation {
            &self.continuations
        } else {
            &self.exceptions
        };
        entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.procedure)
    }
    pub fn set_filter(&mut self, filter: u32) -> u32 {
        std::mem::replace(&mut self.filter, filter)
    }
    pub fn filter(&self) -> u32 {
        self.filter
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handler_order_identity_and_live_removal_are_distinct() {
        let mut registry = HandlerRegistry32::new(3).unwrap();
        let a = registry.add(false, false, 0x1000).unwrap();
        let b = registry.add(false, true, 0x2000).unwrap();
        let c = registry.add(true, true, 0x3000).unwrap();
        let snapshot = registry.snapshot(false).unwrap();
        assert_eq!(snapshot, [b, a]);
        assert_eq!(
            registry.add(false, false, 1),
            Err(ExceptionError32::Capacity)
        );
        assert!(!registry.remove(true, a));
        assert!(registry.remove(false, a));
        assert_eq!(registry.procedure(false, a), None);
        assert_eq!(registry.snapshot(true).unwrap(), [c]);
        let d = registry.add(false, false, 0x4000).unwrap();
        assert_ne!(a, d);
        assert_eq!(registry.snapshot(false).unwrap(), [b, d]);
        assert_eq!(registry.set_filter(0x5000), 0);
        assert_eq!(registry.set_filter(0), 0x5000);
    }
}
