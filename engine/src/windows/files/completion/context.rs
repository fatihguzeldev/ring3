use super::*;

pub(crate) struct CompletionContext32 {
    pub(super) ports: Vec<Port32>,
    pub(super) associations: Vec<Association32>,
    pub(super) packets: Vec<Option<PacketSlot32>>,
    pub(super) free: Vec<usize>,
    pub(super) threads: Vec<Option<Thread32>>,
    max_ports: usize,
    max_associations: usize,
    next: u64,
    pub(super) revision: u64,
    pub(super) wait_order: u64,
}
impl CompletionContext32 {
    pub(crate) fn new(
        ports: usize,
        associations: usize,
        packets: usize,
        threads: usize,
    ) -> Result<Self, FileError32> {
        if ports == 0
            || associations == 0
            || packets == 0
            || threads == 0
            || ports > 65536
            || associations > 65536
            || packets > 65536
            || threads > 4096
        {
            return Err(FileError32::InvalidLimits);
        }
        let mut values = Self {
            ports: Vec::new(),
            associations: Vec::new(),
            packets: Vec::new(),
            free: Vec::new(),
            threads: Vec::new(),
            max_ports: ports,
            max_associations: associations,
            next: 1,
            revision: 1,
            wait_order: 0,
        };
        values
            .ports
            .try_reserve_exact(ports)
            .map_err(|_| FileError32::Allocation)?;
        values
            .associations
            .try_reserve_exact(associations)
            .map_err(|_| FileError32::Allocation)?;
        values
            .packets
            .try_reserve_exact(packets)
            .map_err(|_| FileError32::Allocation)?;
        values.packets.resize_with(packets, || None);
        values
            .free
            .try_reserve_exact(packets)
            .map_err(|_| FileError32::Allocation)?;
        values.free.extend((0..packets).rev());
        values
            .threads
            .try_reserve_exact(threads)
            .map_err(|_| FileError32::Allocation)?;
        values.threads.resize_with(threads, || None);
        Ok(values)
    }
    pub(super) fn next_revision(&self) -> Result<u64, FileError32> {
        self.revision.checked_add(1).ok_or(FileError32::Exhausted)
    }
    pub(super) fn port(&self, id: CompletionPortId32) -> Result<&Port32, FileError32> {
        self.ports
            .iter()
            .find(|port| port.id == id)
            .ok_or(FileError32::InvalidHandle)
    }
    pub(super) fn port_mut(&mut self, id: CompletionPortId32) -> &mut Port32 {
        self.ports.iter_mut().find(|port| port.id == id).unwrap()
    }
    pub(crate) fn create(&mut self, concurrency: u32) -> Result<CompletionPortId32, FileError32> {
        self.check_create(None)?;
        if self.ports.len() >= self.max_ports {
            return Err(FileError32::Capacity);
        }
        let next = self.next.checked_add(1).ok_or(FileError32::Exhausted)?;
        let revision = self.next_revision()?;
        let id = CompletionPortId32(self.next);
        self.ports.push(Port32 {
            id,
            references: 1,
            concurrency: concurrency.max(1),
            head: None,
            tail: None,
            queued: 0,
        });
        self.next = next;
        self.revision = revision;
        Ok(id)
    }
    pub(crate) fn check_create(
        &self,
        target: Option<CompletionTarget32>,
    ) -> Result<(), FileError32> {
        if self.ports.len() >= self.max_ports {
            return Err(FileError32::Capacity);
        }
        self.next.checked_add(1).ok_or(FileError32::Exhausted)?;
        self.revision
            .checked_add(1 + u64::from(target.is_some()))
            .ok_or(FileError32::Exhausted)?;
        if let Some(target) = target {
            self.check_associate(target, None)?;
        }
        Ok(())
    }
    pub(crate) fn check_retain(&self, id: CompletionPortId32) -> Result<(), FileError32> {
        self.port(id)?
            .references
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        self.next_revision()?;
        Ok(())
    }
    pub(crate) fn retain(&mut self, id: CompletionPortId32) -> Result<(), FileError32> {
        self.check_retain(id)?;
        self.port_mut(id).references += 1;
        self.revision += 1;
        Ok(())
    }
    pub(crate) fn check_release(&self, id: CompletionPortId32) -> Result<(), FileError32> {
        if self.port(id)?.references == 0 {
            return Err(FileError32::InvalidHandle);
        }
        self.next_revision()?;
        Ok(())
    }
    pub(crate) fn release(&mut self, id: CompletionPortId32) -> Result<(), FileError32> {
        self.check_release(id)?;
        self.port_mut(id).references -= 1;
        if self.port(id)?.references == 0 {
            while let Some(slot) = self.port(id)?.head {
                self.remove_head(id, slot);
            }
        }
        self.revision += 1;
        self.collect(id);
        Ok(())
    }
    pub(crate) fn is_closed(&self, id: CompletionPortId32) -> bool {
        !self.port(id).is_ok_and(|port| port.references != 0)
    }
    pub(crate) fn check_associate(
        &self,
        target: CompletionTarget32,
        port: Option<CompletionPortId32>,
    ) -> Result<(), FileError32> {
        if let Some(id) = port
            && self.is_closed(id)
        {
            return Err(FileError32::InvalidHandle);
        }
        if let Some(existing) = self
            .associations
            .iter()
            .find(|value| value.target == target)
        {
            if existing.port.is_some() && existing.port != port {
                return Err(FileError32::InvalidParameter);
            }
        } else if self.associations.len() >= self.max_associations {
            return Err(FileError32::Capacity);
        }
        self.next_revision()?;
        Ok(())
    }
    pub(crate) fn associate(
        &mut self,
        target: CompletionTarget32,
        id: CompletionPortId32,
        key: u32,
    ) -> Result<(), FileError32> {
        self.check_associate(target, Some(id))?;
        if let Some(existing) = self
            .associations
            .iter_mut()
            .find(|value| value.target == target)
        {
            if existing.port.is_none() {
                existing.port = Some(id);
                existing.key = key;
                self.revision += 1;
            }
            return Ok(());
        }
        self.associations.push(Association32 {
            target,
            port: Some(id),
            key,
            modes: 0,
        });
        self.revision += 1;
        Ok(())
    }
    pub(crate) fn association(
        &self,
        target: CompletionTarget32,
    ) -> Option<(CompletionPortId32, u32, u8)> {
        self.associations
            .iter()
            .find(|value| value.target == target)
            .and_then(|value| value.port.map(|port| (port, value.key, value.modes)))
    }
    pub(crate) fn modes(&self, target: CompletionTarget32) -> u8 {
        self.associations
            .iter()
            .find(|value| value.target == target)
            .map_or(0, |value| value.modes)
    }
    pub(crate) fn set_modes(
        &mut self,
        target: CompletionTarget32,
        modes: u8,
    ) -> Result<(), FileError32> {
        if modes & !3 != 0 {
            return Err(FileError32::InvalidParameter);
        }
        self.next_revision()?;
        if let Some(association) = self
            .associations
            .iter_mut()
            .find(|value| value.target == target)
        {
            association.modes |= modes;
        } else {
            if self.associations.len() >= self.max_associations {
                return Err(FileError32::Capacity);
            }
            self.associations.push(Association32 {
                target,
                port: None,
                key: 0,
                modes,
            });
        }
        self.revision += 1;
        Ok(())
    }
    pub(crate) fn release_target(&mut self, target: CompletionTarget32) {
        if let Some(index) = self
            .associations
            .iter()
            .position(|value| value.target == target)
        {
            let id = self.associations.remove(index).port;
            if let Some(id) = id {
                self.collect(id);
            }
        }
    }
    pub(super) fn collect(&mut self, id: CompletionPortId32) {
        if self.port(id).is_ok_and(|port| port.references == 0)
            && !self.associations.iter().any(|value| value.port == Some(id))
            && !self
                .threads
                .iter()
                .flatten()
                .any(|thread| thread.port == id && thread.waiting.is_some())
        {
            while let Some(slot) = self.port(id).unwrap().head {
                self.remove_head(id, slot);
            }
            self.ports.retain(|port| port.id != id);
            for row in &mut self.threads {
                if row.is_some_and(|thread| thread.port == id) {
                    *row = None;
                }
            }
        }
    }
    pub(crate) fn retire_thread(&mut self, thread: u32) {
        if let Some(index) = self
            .threads
            .iter()
            .position(|row| row.is_some_and(|row| row.id == thread))
        {
            let port = self.threads[index].take().unwrap().port;
            self.collect(port);
        }
    }
    pub(crate) fn clear(&mut self) {
        self.ports.clear();
        self.associations.clear();
        self.threads.fill(None);
        self.packets.iter_mut().for_each(|slot| *slot = None);
        self.free.clear();
        self.free.extend((0..self.packets.len()).rev());
    }
}
