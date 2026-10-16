use super::*;

impl PipeContext32 {
    fn io_endpoint(
        &self,
        id: PipeEndpointId32,
        write: bool,
    ) -> Result<(usize, usize), FileError32> {
        let endpoint = self.endpoint(id)?;
        let instance = &self.instances[endpoint.instance].as_ref().unwrap();
        if endpoint.client && endpoint.session != instance.session {
            return Err(FileError32::PipeNotConnected);
        }
        if endpoint.access & if write { 2 } else { 1 } == 0 {
            return Err(FileError32::AccessDenied);
        }
        if matches!(instance.state, PipeState32::Disconnected) {
            return Err(FileError32::PipeNotConnected);
        }
        if instance.state == PipeState32::Listening {
            return Err(FileError32::PipeListening);
        }
        let channel = usize::from(if write {
            endpoint.client
        } else {
            !endpoint.client
        });
        Ok((endpoint.instance, channel))
    }
    pub(crate) fn prepare_read(
        &self,
        id: PipeEndpointId32,
        length: usize,
    ) -> Result<PipeIo32<PreparedPipeRead32>, FileError32> {
        let (slot, index) = self.io_endpoint(id, false)?;
        let endpoint = self.endpoint(id)?;
        let instance = self.instances[slot].as_ref().unwrap();
        let channel = &instance.channels[index];
        if channel.length == 0 && channel.messages.is_empty() {
            if !self.peer_open(endpoint) {
                return Err(FileError32::BrokenPipe);
            }
            return if endpoint.nonblocking {
                Err(FileError32::NoData)
            } else {
                Ok(PipeIo32::Pending)
            };
        }
        let available = if endpoint.read_message {
            channel.messages.front().copied().unwrap_or(0)
        } else {
            channel.length
        };
        let count = length.min(available);
        self.next_revision()?;
        Ok(PipeIo32::Ready(PreparedPipeRead32 {
            revision: self.revision,
            instance: slot,
            channel: index,
            bytes: channel.copy(count)?,
            more: endpoint.read_message && count < available,
            consume_empty: length != 0 && available == 0 && channel.messages.front() == Some(&0),
        }))
    }
    pub(crate) fn commit_read(&mut self, prepared: PreparedPipeRead32) {
        assert_eq!(prepared.revision, self.revision);
        let channel =
            &mut self.instances[prepared.instance].as_mut().unwrap().channels[prepared.channel];
        if !prepared.bytes.is_empty() || prepared.consume_empty {
            channel.consume(prepared.bytes.len(), prepared.consume_empty);
        }
        self.revision += 1;
    }
    pub(crate) fn prepare_write(
        &self,
        id: PipeEndpointId32,
        bytes: &[u8],
    ) -> Result<PipeIo32<PreparedPipeWrite32>, FileError32> {
        let (slot, index) = self.io_endpoint(id, true)?;
        let endpoint = self.endpoint(id)?;
        let instance = self.instances[slot].as_ref().unwrap();
        if !self.peer_open(endpoint) {
            return Err(FileError32::NoData);
        }
        let channel = &instance.channels[index];
        let original_empty = bytes.is_empty();
        let bytes = if endpoint.nonblocking {
            let free = channel.bytes.len() - channel.length;
            let count = if instance.message
                && (bytes.len() > free || channel.messages.len() >= self.limits.messages)
            {
                0
            } else {
                bytes.len().min(free)
            };
            &bytes[..count]
        } else {
            bytes
        };
        if bytes.len() > self.limits.bytes {
            return Err(FileError32::Capacity);
        }
        if instance.message
            && channel.messages.len() >= self.limits.messages
            && !endpoint.nonblocking
        {
            return Ok(PipeIo32::Pending);
        }
        if bytes.len() > self.limits.bytes - (self.reserved - channel.bytes.len()) {
            return Err(FileError32::Capacity);
        }
        let needed = channel
            .length
            .checked_add(bytes.len())
            .ok_or(FileError32::Capacity)?;
        let growth = needed.saturating_sub(channel.bytes.len());
        let mut replacement = None;
        if growth != 0 {
            let available = self.limits.bytes - self.reserved;
            if growth > available {
                return Ok(PipeIo32::Pending);
            }
            let capacity = needed
                .max(channel.bytes.len().saturating_mul(2))
                .min(channel.bytes.len() + available);
            let mut buffer = Vec::new();
            buffer
                .try_reserve_exact(capacity)
                .map_err(|_| FileError32::Allocation)?;
            buffer.resize(capacity, 0);
            let content = channel.copy(channel.length)?;
            buffer[..content.len()].copy_from_slice(&content);
            replacement = Some(buffer);
        }
        let mut content = Vec::new();
        content
            .try_reserve_exact(bytes.len())
            .map_err(|_| FileError32::Allocation)?;
        content.extend_from_slice(bytes);
        self.next_revision()?;
        Ok(PipeIo32::Ready(PreparedPipeWrite32 {
            revision: self.revision,
            instance: slot,
            channel: index,
            bytes: content,
            replacement,
            message: instance.message
                && (!bytes.is_empty() || original_empty)
                && channel.messages.len() < self.limits.messages,
        }))
    }
    pub(crate) fn commit_write(&mut self, prepared: PreparedPipeWrite32) {
        assert_eq!(prepared.revision, self.revision);
        let channel =
            &mut self.instances[prepared.instance].as_mut().unwrap().channels[prepared.channel];
        if let Some(bytes) = prepared.replacement {
            self.reserved += bytes.len() - channel.bytes.len();
            channel.bytes = bytes;
            channel.head = 0;
        }
        channel.append(&prepared.bytes, prepared.message);
        self.revision += 1;
    }
    pub(crate) fn peek(
        &self,
        id: PipeEndpointId32,
        length: usize,
    ) -> Result<(Vec<u8>, u32, u32), FileError32> {
        let endpoint = self.endpoint(id)?;
        if !endpoint.client
            && self.instances[endpoint.instance].as_ref().unwrap().state != PipeState32::Connected
        {
            return Err(FileError32::BadPipe);
        }
        let (slot, index) = self.io_endpoint(id, false)?;
        let instance = self.instances[slot].as_ref().unwrap();
        let channel = &instance.channels[index];
        let count = length.min(if instance.message {
            channel.messages.front().copied().unwrap_or(0)
        } else {
            channel.length
        });
        if channel.length == 0 && !self.peer_open(self.endpoint(id)?) {
            return Err(FileError32::BrokenPipe);
        }
        Ok((
            channel.copy(count)?,
            channel.length as u32,
            if instance.message {
                channel
                    .messages
                    .front()
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(count) as u32
            } else {
                0
            },
        ))
    }
}
