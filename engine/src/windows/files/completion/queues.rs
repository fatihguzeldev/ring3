use super::*;

impl CompletionContext32 {
    pub(crate) fn prepare_posts(
        &self,
        packets: &[(CompletionPortId32, CompletionPacket32)],
    ) -> Result<Option<PreparedCompletionPosts32>, FileError32> {
        if packets.is_empty() {
            return Ok(None);
        }
        for (port, _) in packets {
            self.port(*port)?;
        }
        let count = packets
            .iter()
            .filter(|(port, _)| !self.is_closed(*port))
            .count();
        if count == 0 {
            return Ok(None);
        }
        self.next_revision()?;
        if count > self.free.len() {
            return Err(FileError32::Capacity);
        }
        let mut planned = Vec::new();
        planned
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        for ((port, packet), slot) in packets
            .iter()
            .filter(|(port, _)| !self.is_closed(*port))
            .zip(self.free.iter().rev())
        {
            self.port(*port)?;
            planned.push((*port, *slot, *packet));
        }
        Ok(Some(PreparedCompletionPosts32 {
            revision: self.revision,
            packets: planned,
        }))
    }
    pub(crate) fn commit_posts(&mut self, prepared: Option<PreparedCompletionPosts32>) {
        let Some(prepared) = prepared else {
            return;
        };
        assert_eq!(prepared.revision, self.revision);
        for (port, slot, packet) in prepared.packets {
            self.append_packet(port, slot, packet);
        }
        self.revision += 1;
    }
    fn append_packet(&mut self, port: CompletionPortId32, slot: usize, packet: CompletionPacket32) {
        assert_eq!(self.free.pop(), Some(slot));
        let tail = self.port(port).unwrap().tail;
        self.packets[slot] = Some(PacketSlot32 { packet, next: None });
        if let Some(tail) = tail {
            self.packets[tail].as_mut().unwrap().next = Some(slot);
        }
        let port = self.port_mut(port);
        port.head = port.head.or(Some(slot));
        port.tail = Some(slot);
        port.queued += 1;
    }
    pub(crate) fn prepare_post(
        &self,
        port: CompletionPortId32,
        packet: CompletionPacket32,
    ) -> Result<PreparedCompletionPost32, FileError32> {
        self.port(port)?;
        let slot = if self.is_closed(port) {
            None
        } else {
            self.next_revision()?;
            Some(*self.free.last().ok_or(FileError32::Capacity)?)
        };
        Ok(PreparedCompletionPost32 {
            revision: self.revision,
            port,
            slot,
            packet,
        })
    }
    pub(crate) fn commit_post(&mut self, prepared: PreparedCompletionPost32) {
        assert_eq!(prepared.revision, self.revision);
        if let Some(slot) = prepared.slot {
            self.append_packet(prepared.port, slot, prepared.packet);
            self.revision += 1;
        }
    }
    pub(super) fn remove_head(&mut self, id: CompletionPortId32, slot: usize) {
        let entry = self.packets[slot].take().unwrap();
        let port = self.port_mut(id);
        assert_eq!(port.head, Some(slot));
        port.head = entry.next;
        port.queued -= 1;
        if port.head.is_none() {
            port.tail = None;
        }
        self.free.push(slot);
    }
    fn thread_slot(&self, thread: u32) -> Result<usize, FileError32> {
        if thread == 0 {
            return Err(FileError32::InvalidParameter);
        }
        self.threads
            .iter()
            .position(|row| row.is_some_and(|row| row.id == thread))
            .or_else(|| self.threads.iter().position(Option::is_none))
            .ok_or(FileError32::Capacity)
    }
    pub(crate) fn prepare_wait(
        &self,
        port: CompletionPortId32,
        thread: u32,
    ) -> Result<PreparedCompletionWait32, FileError32> {
        if self.is_closed(port) {
            return Err(FileError32::InvalidHandle);
        }
        self.next_revision()?;
        let order = self
            .wait_order
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        Ok(PreparedCompletionWait32 {
            revision: self.revision,
            port,
            thread,
            row: self.thread_slot(thread)?,
            order,
        })
    }
    pub(crate) fn commit_wait(&mut self, prepared: PreparedCompletionWait32) {
        assert_eq!(prepared.revision, self.revision);
        let previous = self.threads[prepared.row].map(|row| row.port);
        self.threads[prepared.row] = Some(Thread32 {
            id: prepared.thread,
            port: prepared.port,
            active: false,
            waiting: Some(prepared.order),
        });
        self.wait_order = prepared.order;
        self.revision += 1;
        if let Some(previous) = previous.filter(|port| *port != prepared.port) {
            self.collect(previous);
        }
    }
    pub(crate) fn ready(
        &self,
        port: CompletionPortId32,
        thread: u32,
        runnable: impl Fn(u32) -> bool,
    ) -> bool {
        let Ok(value) = self.port(port) else {
            return true;
        };
        if value.references == 0 {
            return true;
        }
        if value.queued == 0 {
            return false;
        }
        let active = self
            .threads
            .iter()
            .flatten()
            .filter(|row| row.id != thread && row.port == port && row.active && runnable(row.id))
            .count();
        if active >= value.concurrency as usize {
            return false;
        }
        let order = self
            .threads
            .iter()
            .flatten()
            .find(|row| row.id == thread && row.port == port)
            .and_then(|row| row.waiting);
        !order.is_some_and(|order| {
            self.threads.iter().flatten().any(|row| {
                row.id != thread
                    && row.port == port
                    && row.waiting.is_some_and(|later| later > order)
            })
        })
    }
    pub(crate) fn prepare_take(
        &self,
        port: CompletionPortId32,
        thread: u32,
        count: usize,
        runnable: impl Fn(u32) -> bool,
    ) -> Result<Option<PreparedCompletionTake32>, FileError32> {
        if count == 0 {
            return Err(FileError32::InvalidParameter);
        }
        if self.is_closed(port) {
            return Err(FileError32::InvalidHandle);
        }
        if !self.ready(port, thread, runnable) {
            return Ok(None);
        }
        self.next_revision()?;
        let port_value = self.port(port)?;
        let length = count.min(port_value.queued);
        let mut packets = Vec::new();
        packets
            .try_reserve_exact(length)
            .map_err(|_| FileError32::Allocation)?;
        let mut slot = port_value.head;
        for _ in 0..length {
            let entry = self.packets[slot.unwrap()].as_ref().unwrap();
            packets.push(entry.packet);
            slot = entry.next;
        }
        Ok(Some(PreparedCompletionTake32 {
            revision: self.revision,
            port,
            thread,
            row: self.thread_slot(thread)?,
            packets,
        }))
    }
    pub(crate) fn commit_take(&mut self, prepared: PreparedCompletionTake32) {
        assert_eq!(prepared.revision, self.revision);
        for _ in &prepared.packets {
            let slot = self.port(prepared.port).unwrap().head.unwrap();
            self.remove_head(prepared.port, slot);
        }
        let previous = self.threads[prepared.row].map(|row| row.port);
        self.threads[prepared.row] = Some(Thread32 {
            id: prepared.thread,
            port: prepared.port,
            active: true,
            waiting: None,
        });
        self.revision += 1;
        if let Some(previous) = previous.filter(|port| *port != prepared.port) {
            self.collect(previous);
        }
    }
    #[cfg(test)]
    pub(crate) fn cancel_wait(&mut self, thread: u32) {
        if let Some(port) = self
            .threads
            .iter()
            .flatten()
            .find(|row| row.id == thread)
            .map(|row| row.port)
        {
            self.cancel_wait_for(thread, port);
        }
    }
    pub(crate) fn cancel_wait_for(&mut self, thread: u32, port: CompletionPortId32) {
        if let Some(row) = self
            .threads
            .iter_mut()
            .flatten()
            .find(|row| row.id == thread && row.port == port && row.waiting.is_some())
        {
            row.waiting = None;
            row.active = false;
        }
        self.collect(port);
    }
}
