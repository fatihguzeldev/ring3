use super::*;
use crate::windows::security::{FILE_GENERIC_MAPPING, Token32};

pub(crate) fn pipe_name(path: &[u16]) -> Result<Option<Vec<u16>>, FileError32> {
    if path.len() < 9 {
        return Ok(None);
    }
    let prefix = crate::windows::text::ordinal_key(&path[..9]);
    if prefix != [92, 92, 46, 92, 80, 73, 80, 69, 92]
        && prefix != [92, 92, 63, 92, 80, 73, 80, 69, 92]
    {
        return Ok(None);
    }
    if path.len() > 256 || path.len() == 9 || path[9..].iter().any(|unit| matches!(unit, 0 | 92)) {
        return Err(FileError32::InvalidName);
    }
    Ok(Some(crate::windows::text::ordinal_key(&path[9..])))
}
impl PipeContext32 {
    fn free_slots(&self, count: usize) -> Result<(usize, [usize; 2]), FileError32> {
        let instance = self
            .instances
            .iter()
            .position(Option::is_none)
            .ok_or(FileError32::Capacity)?;
        let mut endpoints = [0; 2];
        let mut found = 0;
        for (index, slot) in self.endpoints.iter().enumerate() {
            if slot.is_none() {
                endpoints[found] = index;
                found += 1;
                if found == count {
                    break;
                }
            }
        }
        if found != count {
            return Err(FileError32::Capacity);
        }
        Ok((instance, endpoints))
    }
    pub(crate) fn prepare_server(
        &self,
        name: &[u16],
        options: PipeOptions32,
        security: SecurityDescriptor32,
        token: &Token32,
    ) -> Result<PreparedPipeCreate32, FileError32> {
        let mut options = options;
        if options.timeout == 0 {
            options.timeout = 50;
        }
        if !(1..=3).contains(&options.access)
            || options.maximum == 0
            || options.maximum > 255
            || options.read_message && !options.message
        {
            return Err(FileError32::InvalidParameter);
        }
        let name = pipe_name(name)?.ok_or(FileError32::InvalidName)?;
        let mut security = security;
        let mut matching = self
            .instances
            .iter()
            .flatten()
            .filter(|value| value.name.as_ref() == Some(&name) && value.server.is_some());
        if let Some(first) = matching.next() {
            if options.first {
                return Err(FileError32::AccessDenied);
            }
            if first.message != options.message
                || first.access != options.access
                || first.maximum != options.maximum
                || first.timeout != options.timeout
            {
                return Err(FileError32::AccessDenied);
            }
            first
                .security
                .check_access(token, 4, FILE_GENERIC_MAPPING)?;
            security = first.security.snapshot()?;
            if options.maximum != 255 && matching.count() + 1 >= options.maximum as usize {
                return Err(FileError32::PipeBusy);
            }
        }
        self.prepare_instance(Some(name), options, security, false)
    }
    pub(crate) fn prepare_anonymous(
        &self,
        size: usize,
        security: SecurityDescriptor32,
    ) -> Result<PreparedPipeCreate32, FileError32> {
        self.prepare_instance(
            None,
            PipeOptions32 {
                access: 1,
                in_buffer: size,
                out_buffer: 0,
                ..Default::default()
            },
            security,
            true,
        )
    }
    fn prepare_instance(
        &self,
        name: Option<Vec<u16>>,
        options: PipeOptions32,
        security: SecurityDescriptor32,
        anonymous: bool,
    ) -> Result<PreparedPipeCreate32, FileError32> {
        let count = if anonymous { 2 } else { 1 };
        let (slot, endpoint_slots) = self.free_slots(count)?;
        self.next_revision()?;
        self.next
            .checked_add(count as u64)
            .ok_or(FileError32::Exhausted)?;
        let inbound = if options.access & 1 != 0 {
            options.in_buffer.max(1)
        } else {
            0
        };
        let outbound = if options.access & 2 != 0 {
            options.out_buffer.max(1)
        } else {
            0
        };
        let capacity = inbound.checked_add(outbound).ok_or(FileError32::Capacity)?;
        if capacity > self.limits.bytes - self.reserved {
            return Err(FileError32::Capacity);
        }
        let channels = [
            channel::Channel32::new(outbound, self.limits.messages)?,
            channel::Channel32::new(inbound, self.limits.messages)?,
        ];
        let first = PipeEndpointId32(self.next);
        let second = anonymous.then_some(PipeEndpointId32(self.next + 1));
        let instance = Instance32 {
            name,
            server: Some(first),
            client: second,
            session: 1,
            state: if anonymous {
                PipeState32::Connected
            } else {
                PipeState32::Listening
            },
            message: options.message,
            access: options.access,
            maximum: options.maximum,
            timeout: options.timeout,
            channels,
            security,
        };
        let server = Endpoint32 {
            id: first,
            instance: slot,
            references: 1,
            client: false,
            session: 1,
            access: options.access,
            read_message: options.read_message,
            nonblocking: options.nonblocking,
            overlapped: options.overlapped,
        };
        let client = second.map(|id| Endpoint32 {
            id,
            instance: slot,
            references: 1,
            client: true,
            session: 1,
            access: 2,
            read_message: false,
            nonblocking: false,
            overlapped: false,
        });
        Ok(PreparedPipeCreate32 {
            revision: self.revision,
            slot,
            endpoint_slots,
            instance,
            server,
            client,
            capacity,
        })
    }
    pub(crate) fn commit_create(
        &mut self,
        prepared: PreparedPipeCreate32,
    ) -> (PipeEndpointId32, Option<PipeEndpointId32>) {
        assert_eq!(prepared.revision, self.revision);
        let first = prepared.server.id;
        let second = prepared.client.as_ref().map(|endpoint| endpoint.id);
        self.instances[prepared.slot] = Some(prepared.instance);
        self.endpoints[prepared.endpoint_slots[0]] = Some(prepared.server);
        if let Some(client) = prepared.client {
            self.endpoints[prepared.endpoint_slots[1]] = Some(client);
        }
        self.next += if second.is_some() { 2 } else { 1 };
        self.reserved += prepared.capacity;
        self.revision += 1;
        (first, second)
    }
    pub(crate) fn prepare_client(
        &self,
        name: &[u16],
        desired: u32,
        overlapped: bool,
        token: &Token32,
    ) -> Result<PreparedPipeClient32, FileError32> {
        let name = pipe_name(name)?.ok_or(FileError32::InvalidName)?;
        let mut found = false;
        let mut selected = None;
        for (index, instance) in self.instances.iter().enumerate() {
            if let Some(instance) = instance
                && instance.name.as_ref() == Some(&name)
                && instance.server.is_some()
            {
                found = true;
                if instance.state == PipeState32::Listening {
                    selected = Some(index);
                    break;
                }
            }
        }
        let slot = selected.ok_or(if found {
            FileError32::PipeBusy
        } else {
            FileError32::NotFound
        })?;
        let instance = self.instances[slot].as_ref().unwrap();
        let granted = instance
            .security
            .check_access(token, desired, FILE_GENERIC_MAPPING)?;
        let access = granted & 3;
        let allowed = if instance.access == 1 {
            2
        } else if instance.access == 2 {
            1
        } else {
            3
        };
        if access & !allowed != 0 {
            return Err(FileError32::AccessDenied);
        }
        let endpoint_slot = self
            .endpoints
            .iter()
            .position(Option::is_none)
            .ok_or(FileError32::Capacity)?;
        self.next_revision()?;
        self.next.checked_add(1).ok_or(FileError32::Exhausted)?;
        Ok(PreparedPipeClient32 {
            revision: self.revision,
            slot: endpoint_slot,
            endpoint: Endpoint32 {
                id: PipeEndpointId32(self.next),
                instance: slot,
                references: 1,
                client: true,
                session: instance.session,
                access,
                read_message: false,
                nonblocking: false,
                overlapped,
            },
            granted,
        })
    }
    pub(crate) fn commit_client(&mut self, prepared: PreparedPipeClient32) -> PipeEndpointId32 {
        assert_eq!(prepared.revision, self.revision);
        let id = prepared.endpoint.id;
        let instance = self.instances[prepared.endpoint.instance].as_mut().unwrap();
        instance.client = Some(id);
        instance.state = PipeState32::Connected;
        self.endpoints[prepared.slot] = Some(prepared.endpoint);
        self.next += 1;
        self.revision += 1;
        id
    }
    pub(crate) fn available(&self, name: &[u16]) -> Result<bool, FileError32> {
        let name = pipe_name(name)?.ok_or(FileError32::InvalidName)?;
        let mut found = false;
        for instance in self.instances.iter().flatten() {
            if instance.name.as_ref() == Some(&name) && instance.server.is_some() {
                found = true;
                if instance.state == PipeState32::Listening {
                    return Ok(true);
                }
            }
        }
        if found {
            Ok(false)
        } else {
            Err(FileError32::NotFound)
        }
    }
    pub(crate) fn default_timeout(&self, name: &[u16]) -> Result<u32, FileError32> {
        let name = pipe_name(name)?.ok_or(FileError32::InvalidName)?;
        self.instances
            .iter()
            .flatten()
            .find(|value| value.name.as_ref() == Some(&name) && value.server.is_some())
            .map(|value| value.timeout)
            .ok_or(FileError32::NotFound)
    }
    pub(crate) fn connection(&self, id: PipeEndpointId32) -> Result<PipeConnection32, FileError32> {
        let endpoint = self.endpoint(id)?;
        if endpoint.client {
            return Err(FileError32::InvalidHandle);
        }
        let instance = self.instances[endpoint.instance].as_ref().unwrap();
        Ok(match instance.state {
            PipeState32::Connected if instance.client.is_some() => PipeConnection32::Connected,
            PipeState32::Connected => PipeConnection32::Broken,
            _ => PipeConnection32::Listening,
        })
    }
    pub(crate) fn listen(&mut self, id: PipeEndpointId32) -> Result<(), FileError32> {
        let prepared = self.prepare_listen(id)?;
        self.commit_listen(prepared);
        Ok(())
    }
    pub(crate) fn prepare_listen(
        &self,
        id: PipeEndpointId32,
    ) -> Result<PreparedPipeListen32, FileError32> {
        let endpoint = self.endpoint(id)?;
        if endpoint.client {
            return Err(FileError32::InvalidHandle);
        }
        self.next_revision()?;
        Ok(PreparedPipeListen32 {
            revision: self.revision,
            instance: endpoint.instance,
        })
    }
    pub(crate) fn commit_listen(&mut self, prepared: PreparedPipeListen32) {
        assert_eq!(prepared.revision, self.revision);
        let instance = self.instances[prepared.instance].as_mut().unwrap();
        if instance.state == PipeState32::Disconnected {
            instance.state = PipeState32::Listening;
        }
        self.revision += 1;
    }
    pub(crate) fn disconnect(&mut self, id: PipeEndpointId32) -> Result<(), FileError32> {
        let endpoint = self.endpoint(id)?;
        if endpoint.client {
            return Err(FileError32::InvalidHandle);
        }
        let slot = endpoint.instance;
        let instance = self.instances[slot].as_ref().unwrap();
        if instance.state != PipeState32::Connected {
            return Err(FileError32::PipeNotConnected);
        }
        let session = instance
            .session
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let revision = self.next_revision()?;
        let instance = self.instances[slot].as_mut().unwrap();
        instance.session = session;
        instance.client = None;
        instance.state = PipeState32::Disconnected;
        for channel in &mut instance.channels {
            channel.clear();
        }
        self.revision = revision;
        Ok(())
    }
}
