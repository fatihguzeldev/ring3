use super::*;
use crate::files::FileError32;
pub(crate) struct NullDevices32 {
    descriptions: Vec<Option<NullDescription32>>,
    next: u64,
}
impl NullDevices32 {
    pub(crate) fn new(limit: usize) -> Result<Self, FileError32> {
        if limit == 0 || limit > 65536 {
            return Err(FileError32::InvalidLimits);
        }
        let mut descriptions = Vec::new();
        descriptions
            .try_reserve_exact(limit)
            .map_err(|_| FileError32::Allocation)?;
        descriptions.resize_with(limit, || None);
        Ok(Self {
            descriptions,
            next: 1,
        })
    }
    pub(crate) fn prepare_open(&self, overlapped: bool) -> Result<PreparedNullOpen32, FileError32> {
        let slot = self
            .descriptions
            .iter()
            .position(Option::is_none)
            .ok_or(FileError32::Capacity)?;
        self.next.checked_add(1).ok_or(FileError32::Exhausted)?;
        Ok(PreparedNullOpen32 {
            slot,
            id: DeviceDescriptionId32(self.next),
            overlapped,
        })
    }
    pub(crate) fn commit_open(&mut self, prepared: PreparedNullOpen32) -> DeviceDescriptionId32 {
        assert_eq!(prepared.id.0, self.next);
        assert!(self.descriptions[prepared.slot].is_none());
        self.next += 1;
        self.descriptions[prepared.slot] = Some(NullDescription32 {
            id: prepared.id,
            references: 1,
            overlapped: prepared.overlapped,
        });
        prepared.id
    }
    fn slot(&self, id: DeviceDescriptionId32) -> Result<usize, FileError32> {
        self.descriptions
            .iter()
            .position(|value| value.as_ref().is_some_and(|value| value.id == id))
            .ok_or(FileError32::InvalidHandle)
    }
    pub(crate) fn overlapped(&self, id: DeviceDescriptionId32) -> Result<bool, FileError32> {
        Ok(self.descriptions[self.slot(id)?]
            .as_ref()
            .unwrap()
            .overlapped)
    }
    pub(crate) fn check_retain(&self, id: DeviceDescriptionId32) -> Result<(), FileError32> {
        self.descriptions[self.slot(id)?]
            .as_ref()
            .unwrap()
            .references
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        Ok(())
    }
    pub(crate) fn retain(&mut self, id: DeviceDescriptionId32) -> Result<(), FileError32> {
        self.check_retain(id)?;
        let slot = self.slot(id)?;
        self.descriptions[slot].as_mut().unwrap().references += 1;
        Ok(())
    }
    pub(crate) fn release(&mut self, id: DeviceDescriptionId32) -> Result<(), FileError32> {
        let slot = self.slot(id)?;
        let value = self.descriptions[slot].as_mut().unwrap();
        value.references -= 1;
        if value.references == 0 {
            self.descriptions[slot] = None;
        }
        Ok(())
    }
    pub(crate) fn clear(&mut self) {
        self.descriptions.iter_mut().for_each(|value| *value = None);
    }
}
