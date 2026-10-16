use super::*;
impl PipeContext32 {
    pub(crate) fn read_ready(&self, id: PipeEndpointId32) -> bool {
        let Ok(endpoint) = self.endpoint(id) else {
            return true;
        };
        let instance = self.instances[endpoint.instance].as_ref().unwrap();
        if endpoint.client && endpoint.session != instance.session
            || instance.state != PipeState32::Connected
            || endpoint.access & 1 == 0
        {
            return true;
        }
        let channel = &instance.channels[usize::from(!endpoint.client)];
        channel.length != 0
            || !channel.messages.is_empty()
            || !self.peer_open(endpoint)
            || endpoint.nonblocking
    }
    pub(crate) fn write_ready(&self, id: PipeEndpointId32, length: usize) -> bool {
        let Ok(endpoint) = self.endpoint(id) else {
            return true;
        };
        let instance = self.instances[endpoint.instance].as_ref().unwrap();
        if endpoint.client && endpoint.session != instance.session
            || instance.state != PipeState32::Connected
            || endpoint.access & 2 == 0
            || !self.peer_open(endpoint)
            || endpoint.nonblocking
        {
            return true;
        }
        let channel = &instance.channels[usize::from(endpoint.client)];
        let capacity = self.limits.bytes - (self.reserved - channel.bytes.len());
        length > capacity
            || (channel
                .length
                .checked_add(length)
                .is_some_and(|size| size <= capacity)
                && (!instance.message || channel.messages.len() < self.limits.messages))
    }
    pub(crate) fn flush_ready(&self, id: PipeEndpointId32) -> bool {
        let Ok(endpoint) = self.endpoint(id) else {
            return true;
        };
        let instance = self.instances[endpoint.instance].as_ref().unwrap();
        instance.state != PipeState32::Connected
            || !self.peer_open(endpoint)
            || instance.channels[usize::from(endpoint.client)].length == 0
    }
}
