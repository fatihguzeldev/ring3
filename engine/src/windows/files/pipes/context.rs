use super::*;

pub(crate) struct PipeContext32 {
    pub(super) limits: PipeLimits32,
    pub(super) instances: Vec<Option<Instance32>>,
    pub(super) endpoints: Vec<Option<Endpoint32>>,
    pub(super) reserved: usize,
    pub(super) next: u64,
    pub(super) revision: u64,
}
impl PipeContext32 {
    pub(crate) fn new(limits: PipeLimits32) -> Result<Self, FileError32> {
        if limits.instances == 0
            || limits.instances > 65536
            || limits.endpoints == 0
            || limits.endpoints > 65536
            || limits.bytes == 0
            || limits.bytes > 64 * 1024 * 1024
            || limits.messages == 0
            || limits.messages > 65536
        {
            return Err(FileError32::InvalidLimits);
        }
        let mut instances = Vec::new();
        instances
            .try_reserve_exact(limits.instances)
            .map_err(|_| FileError32::Allocation)?;
        instances.resize_with(limits.instances, || None);
        let mut endpoints = Vec::new();
        endpoints
            .try_reserve_exact(limits.endpoints)
            .map_err(|_| FileError32::Allocation)?;
        endpoints.resize_with(limits.endpoints, || None);
        Ok(Self {
            limits,
            instances,
            endpoints,
            reserved: 0,
            next: 1,
            revision: 1,
        })
    }
    pub(super) fn next_revision(&self) -> Result<u64, FileError32> {
        self.revision.checked_add(1).ok_or(FileError32::Exhausted)
    }
    pub(super) fn endpoint(&self, id: PipeEndpointId32) -> Result<&Endpoint32, FileError32> {
        self.endpoints
            .iter()
            .flatten()
            .find(|endpoint| endpoint.id == id)
            .ok_or(FileError32::InvalidHandle)
    }
    fn endpoint_mut(&mut self, id: PipeEndpointId32) -> &mut Endpoint32 {
        self.endpoints
            .iter_mut()
            .flatten()
            .find(|endpoint| endpoint.id == id)
            .unwrap()
    }
    pub(super) fn peer_open(&self, endpoint: &Endpoint32) -> bool {
        let instance = self.instances[endpoint.instance].as_ref().unwrap();
        if endpoint.client {
            instance.server.is_some()
        } else {
            instance.client.is_some()
        }
    }
    pub(crate) fn check_retain(&self, id: PipeEndpointId32) -> Result<(), FileError32> {
        self.endpoint(id)?
            .references
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        Ok(())
    }
    pub(crate) fn retain(&mut self, id: PipeEndpointId32) -> Result<(), FileError32> {
        self.check_retain(id)?;
        self.endpoint_mut(id).references += 1;
        Ok(())
    }
    pub(crate) fn check_release(&self, id: PipeEndpointId32) -> Result<(), FileError32> {
        if self.endpoint(id)?.references == 1 {
            self.next_revision()?;
        }
        Ok(())
    }
    pub(crate) fn check_release_many(
        &self,
        releases: &[(PipeEndpointId32, u32)],
        mutations: u64,
    ) -> Result<(), FileError32> {
        let mut finals = 0;
        for (index, &(id, count)) in releases.iter().enumerate() {
            if count == 0 || releases[..index].iter().any(|(prior, _)| *prior == id) {
                return Err(FileError32::InvalidParameter);
            }
            let endpoint = self.endpoint(id)?;
            if count > endpoint.references {
                return Err(FileError32::InvalidHandle);
            }
            finals += u64::from(count == endpoint.references);
        }
        self.revision
            .checked_add(mutations)
            .and_then(|value| value.checked_add(finals))
            .ok_or(FileError32::Exhausted)?;
        Ok(())
    }
    pub(crate) fn release(&mut self, id: PipeEndpointId32) -> Result<(), FileError32> {
        self.check_release(id)?;
        let endpoint = self.endpoint_mut(id);
        endpoint.references -= 1;
        if endpoint.references != 0 {
            return Ok(());
        }
        let slot = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.as_ref().is_some_and(|endpoint| endpoint.id == id))
            .unwrap();
        let endpoint = self.endpoints[slot].take().unwrap();
        let instance = self.instances[endpoint.instance].as_mut().unwrap();
        if instance.server == Some(id) {
            instance.server = None;
        }
        if instance.client == Some(id) {
            instance.client = None;
        }
        if !self
            .endpoints
            .iter()
            .flatten()
            .any(|value| value.instance == endpoint.instance)
        {
            let instance = self.instances[endpoint.instance].take().unwrap();
            self.reserved -= instance
                .channels
                .iter()
                .map(|channel| channel.bytes.len())
                .sum::<usize>();
        }
        self.revision += 1;
        Ok(())
    }
    pub(crate) fn current_instances(&self, id: PipeEndpointId32) -> Result<u32, FileError32> {
        let instance = self.instances[self.endpoint(id)?.instance]
            .as_ref()
            .unwrap();
        Ok(self
            .instances
            .iter()
            .flatten()
            .filter(|value| {
                value.server.is_some()
                    && match &instance.name {
                        Some(name) => value.name.as_ref() == Some(name),
                        None => std::ptr::eq(*value, instance),
                    }
            })
            .count() as u32)
    }
    pub(crate) fn options(&self, id: PipeEndpointId32) -> Result<PipeEndpointInfo32, FileError32> {
        let endpoint = self.endpoint(id)?;
        let instance = self.instances[endpoint.instance].as_ref().unwrap();
        Ok(PipeEndpointInfo32 {
            server: !endpoint.client,
            message: instance.message,
            read_message: endpoint.read_message,
            nonblocking: endpoint.nonblocking,
            overlapped: endpoint.overlapped,
            in_buffer: instance.channels[1].bytes.len() as u32,
            out_buffer: instance.channels[0].bytes.len() as u32,
            max_instances: instance.maximum,
        })
    }
    pub(crate) fn set_mode(&mut self, id: PipeEndpointId32, mode: u32) -> Result<(), FileError32> {
        if mode & !3 != 0 {
            return Err(FileError32::InvalidParameter);
        }
        if mode & 2 != 0 && !self.options(id)?.message {
            return Err(FileError32::InvalidParameter);
        }
        let revision = self.next_revision()?;
        let endpoint = self.endpoint_mut(id);
        endpoint.read_message = mode & 2 != 0;
        endpoint.nonblocking = mode & 1 != 0;
        self.revision = revision;
        Ok(())
    }
    pub(crate) fn security(
        &self,
        id: PipeEndpointId32,
    ) -> Result<&SecurityDescriptor32, FileError32> {
        Ok(&self.instances[self.endpoint(id)?.instance]
            .as_ref()
            .unwrap()
            .security)
    }
    pub(crate) fn clear(&mut self) {
        self.instances.iter_mut().for_each(|value| *value = None);
        self.endpoints.iter_mut().for_each(|value| *value = None);
        self.reserved = 0;
    }
}
