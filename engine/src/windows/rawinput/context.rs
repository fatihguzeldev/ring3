use super::types::{RawPacket32, RawRegistration32};
use crate::windows::user32::User32Error32 as E;

pub(crate) struct RawInputRuntime32 {
    pub(super) registrations: Vec<RawRegistration32>,
    pub(super) packets: Vec<RawPacket32>,
    pub(super) limit: usize,
    pub(super) next: u32,
}
impl RawInputRuntime32 {
    pub(crate) fn new(limit: usize) -> Result<Self, E> {
        if !(1..=65536).contains(&limit) {
            return Err(E::InvalidLimits);
        }
        let mut packets = Vec::new();
        packets
            .try_reserve_exact(limit)
            .map_err(|_| E::Allocation)?;
        Ok(Self {
            registrations: Vec::new(),
            packets,
            limit,
            next: 0x5800_0000,
        })
    }
    pub(crate) fn registrations(&self) -> &[RawRegistration32] {
        &self.registrations
    }
    pub(crate) fn registration_limit(&self) -> usize {
        self.limit
    }
    pub(crate) fn packet(&self, id: u32, thread: u32) -> Result<&RawPacket32, E> {
        self.packets
            .iter()
            .find(|p| p.id == id && p.thread == thread)
            .ok_or(E::Win32(6))
    }
    pub(crate) fn queued(&self, thread: u32) -> impl Iterator<Item = &RawPacket32> {
        self.packets
            .iter()
            .filter(move |p| p.thread == thread && p.queued)
    }
    pub(crate) fn mark_delivered(&mut self, thread: u32, id: u32) {
        self.packets
            .retain(|p| p.thread != thread || p.queued || p.bytes[12] == 0);
        if let Some(packet) = self.packets.iter_mut().find(|p| p.id == id) {
            packet.queued = false;
        }
    }
    pub(crate) fn retire(&mut self, ids: &[u32]) {
        debug_assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        self.packets.retain(|p| ids.binary_search(&p.id).is_err());
    }
    pub(crate) fn retire_window(&mut self, window: u32) {
        self.packets.retain(|p| p.window != window);
        self.registrations.retain(|r| r.window != window);
    }
    pub(crate) fn retire_thread(&mut self, thread: u32) {
        self.packets.retain(|p| p.thread != thread);
    }
    pub(crate) fn retire_registrations(&mut self, mut matches: impl FnMut(u32) -> bool) {
        self.registrations.retain(|row| !matches(row.window));
    }
    pub(crate) fn terminate(&mut self) {
        self.packets.clear();
        self.registrations.clear();
    }
    pub(crate) fn reset_packets(&mut self) {
        self.packets.clear();
    }
}
