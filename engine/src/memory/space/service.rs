use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};

#[derive(Default)]
pub(super) struct GuardService32 {
    state: AtomicU8,
    fault: Mutex<Option<GuardPage32>>,
}

impl AddressSpace {
    pub(crate) fn begin_guard_service(&self) -> Result<(), MemoryError> {
        self.service_guard
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| MemoryError::InvalidRange)
    }

    pub(crate) fn guard_service_fault(&self) -> Option<GuardPage32> {
        if self.service_guard.state.load(Ordering::Acquire) != 2 {
            return None;
        }
        *self.service_guard.fault.lock().unwrap()
    }

    pub(crate) fn end_guard_service(&self) -> Option<GuardPage32> {
        match self
            .service_guard
            .state
            .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) | Err(0) => return None,
            Err(_) => {}
        }
        let mut fault = self.service_guard.fault.lock().unwrap();
        let recorded = fault.take();
        self.service_guard.state.store(0, Ordering::Release);
        recorded
    }

    pub(super) fn record_guard_service_fault(&self, address: GuestAddress) {
        if self.service_guard.state.load(Ordering::Acquire) != 1 {
            return;
        }
        let mut fault = self.service_guard.fault.lock().unwrap();
        if self.service_guard.state.load(Ordering::Acquire) == 1 {
            *fault = self.guard_page(address);
            self.service_guard.state.store(2, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_guard_witness_is_first_fault_only_and_never_crosses_scopes() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AddressSpace>();
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 2).unwrap(),
                Permissions::READ_WRITE.with_guard(),
            )
            .unwrap();
        assert!(memory.read(GuestAddress(0x1000), &mut [0]).is_err());
        assert_eq!(memory.guard_service_fault(), None);
        memory.begin_guard_service().unwrap();
        assert!(memory.read(GuestAddress(0x1234), &mut [0]).is_err());
        assert!(memory.write(GuestAddress(0x2345), &[0]).is_err());
        let first = memory.guard_service_fault().unwrap();
        assert_eq!(first.address(), GuestAddress(0x1000));
        assert_eq!(memory.end_guard_service(), Some(first));
        assert_eq!(memory.guard_service_fault(), None);
        memory.begin_guard_service().unwrap();
        assert_eq!(memory.end_guard_service(), None);
        assert!(first.is_current(&memory));
    }
}
