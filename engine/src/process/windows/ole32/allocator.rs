use crate::{
    process::{EngineInstance, HostError},
    windows::{memory::MemoryError32, ole32::Ole32Api32},
};

#[derive(Default)]
pub(in crate::process) struct Ole32TaskMemory32 {
    allocations: Vec<u32>,
}

impl EngineInstance {
    pub(super) fn ole32_task_memory_call(
        &mut self,
        api: Ole32Api32,
        args: &[u32],
    ) -> Result<u32, HostError> {
        self.memory()?;
        let result = (|| match api {
            Ole32Api32::CoTaskMemAlloc => self.allocate_ole32_task_memory(args[0]),
            Ole32Api32::CoTaskMemRealloc if args[0] == 0 => {
                self.allocate_ole32_task_memory(args[1])
            }
            Ole32Api32::CoTaskMemRealloc if args[1] == 0 => {
                self.free_ole32_task_memory(args[0]).map(|()| 0)
            }
            Ole32Api32::CoTaskMemRealloc => {
                let index = self
                    .ole32_task_memory
                    .allocations
                    .binary_search(&args[0])
                    .map_err(|_| HostError::WindowsMemory(MemoryError32::InvalidParameter))?;
                let pointer = self.resize_windows_runtime_bytes(args[0], args[1])?;
                self.ole32_task_memory.allocations.remove(index);
                let at = self
                    .ole32_task_memory
                    .allocations
                    .binary_search(&pointer)
                    .unwrap_err();
                self.ole32_task_memory.allocations.insert(at, pointer);
                Ok(pointer)
            }
            Ole32Api32::CoTaskMemFree => self.free_ole32_task_memory(args[0]).map(|()| 0),
            _ => Err(HostError::InvalidRequest),
        })();
        match result {
            Err(HostError::WindowsMemory(MemoryError32::Capacity | MemoryError32::Allocation)) => {
                Ok(0)
            }
            result => result,
        }
    }

    fn allocate_ole32_task_memory(&mut self, size: u32) -> Result<u32, HostError> {
        self.ole32_task_memory
            .allocations
            .try_reserve(1)
            .map_err(|_| HostError::WindowsMemory(MemoryError32::Allocation))?;
        let pointer = self.allocate_windows_runtime_bytes(size)?;
        let index = self
            .ole32_task_memory
            .allocations
            .binary_search(&pointer)
            .unwrap_err();
        self.ole32_task_memory.allocations.insert(index, pointer);
        Ok(pointer)
    }

    fn free_ole32_task_memory(&mut self, pointer: u32) -> Result<(), HostError> {
        if pointer == 0 {
            return Ok(());
        }
        let index = self
            .ole32_task_memory
            .allocations
            .binary_search(&pointer)
            .map_err(|_| HostError::WindowsMemory(MemoryError32::InvalidParameter))?;
        self.release_windows_runtime_bytes(pointer)?;
        self.ole32_task_memory.allocations.remove(index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::GuestAddress;

    #[test]
    fn task_allocator_works_before_com_initialization_and_preserves_reallocated_contents() {
        let mut engine = EngineInstance::new(128, 7).unwrap();
        let pointer = engine
            .ole32_task_memory_call(Ole32Api32::CoTaskMemAlloc, &[8])
            .unwrap();
        assert_ne!(pointer, 0);
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pointer), b"contents")
            .unwrap();
        let next = engine
            .ole32_task_memory_call(Ole32Api32::CoTaskMemRealloc, &[pointer, 8192])
            .unwrap();
        assert_ne!(next, 0);
        let mut bytes = [0; 8];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(next), &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"contents");
        assert_eq!(
            engine
                .ole32_task_memory_call(Ole32Api32::CoTaskMemRealloc, &[next, u32::MAX])
                .unwrap(),
            0
        );
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(next), &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"contents");
        assert_eq!(
            engine
                .ole32_task_memory_call(Ole32Api32::CoTaskMemFree, &[next])
                .unwrap(),
            0
        );
        assert!(engine.ole32_task_memory.allocations.is_empty());
        assert!(
            engine
                .ole32_task_memory_call(Ole32Api32::CoTaskMemFree, &[next])
                .is_err()
        );
        assert_eq!(
            engine
                .ole32_task_memory_call(Ole32Api32::CoTaskMemFree, &[0])
                .unwrap(),
            0
        );
    }
}
