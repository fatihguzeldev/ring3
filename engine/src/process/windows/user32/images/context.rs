use crate::{
    files::FileReadRequest32,
    loader::{ModuleId32, ResourceIdentifier32},
    process::calls::call::PendingOwner,
    windows::{
        CallFrame32,
        user32::{
            User32Api32, User32Error32 as E,
            images::{ImageKind32, LoadImage32},
        },
    },
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ResourceImage32 {
    pub module: ModuleId32,
    pub group: u32,
    pub kind: ImageKind32,
    pub name: ResourceIdentifier32,
    pub language: u16,
}

pub(super) struct ImageRecord32 {
    pub handle: u32,
    pub kind: ImageKind32,
    pub shared: bool,
    pub retiring: bool,
    pub origin: Option<ImageOrigin32>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ImageOrigin32 {
    Resource(ResourceImage32),
    System { kind: ImageKind32, id: u16 },
}

#[derive(Default)]
pub(in crate::process::windows::user32) struct ImageRuntime32 {
    records: Vec<ImageRecord32>,
    retirements: usize,
    pub(super) reply: Option<ImageReply32>,
}

pub(in crate::process::windows::user32) struct ImageReply32 {
    pub(super) owner: PendingOwner,
    pub(super) token: u32,
    pub(super) frame: CallFrame32,
    pub(super) api: User32Api32,
    pub(super) thread: u32,
    pub(super) request: LoadImage32,
    pub(super) input: super::input::ImageInput32,
    pub(super) pending: Option<FileReadRequest32>,
    pub(super) error: Option<u32>,
}

impl ImageRuntime32 {
    pub(in crate::process::windows::user32) fn take_reply(&mut self) -> Option<ImageReply32> {
        self.reply.take()
    }
    pub(in crate::process::windows::user32) fn restore_reply(
        &mut self,
        reply: Option<ImageReply32>,
    ) {
        assert!(self.reply.is_none());
        self.reply = reply;
    }
    pub(super) fn reserve(&mut self, capacity: usize, replace: Option<u32>) -> Result<(), E> {
        if replace.is_some_and(|handle| self.records.iter().any(|record| record.handle == handle)) {
            return Ok(());
        }
        if self.records.len() >= capacity {
            return Err(E::Capacity);
        }
        self.records.try_reserve(1).map_err(|_| E::Allocation)
    }
    pub(super) fn shared(&self, source: &ImageOrigin32) -> Option<u32> {
        self.records
            .iter()
            .find(|record| {
                record.shared
                    && record
                        .origin
                        .as_ref()
                        .is_some_and(|origin| match (origin, source) {
                            (
                                ImageOrigin32::Resource(resource),
                                ImageOrigin32::Resource(source),
                            ) => {
                                resource.module == source.module
                                    && resource.kind == source.kind
                                    && resource.name == source.name
                            }
                            (left, right) => left == right,
                        })
            })
            .map(|record| record.handle)
    }
    pub(super) fn shared_resource(
        &self,
        module: ModuleId32,
        kind: ImageKind32,
        name: &ResourceIdentifier32,
    ) -> Option<u32> {
        self.records.iter().find_map(|record| {
            if !record.shared {
                return None;
            }
            let Some(ImageOrigin32::Resource(source)) = record.origin.as_ref() else {
                return None;
            };
            (source.module == module && source.kind == kind && &source.name == name)
                .then_some(record.handle)
        })
    }
    pub(super) fn record(&self, handle: u32, kind: ImageKind32) -> Result<&ImageRecord32, E> {
        self.records
            .iter()
            .find(|record| record.handle == handle && record.kind == kind)
            .ok_or(E::Win32(6))
    }
    pub(super) fn defer_retirement(&mut self, handle: u32, kind: ImageKind32) {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.handle == handle && record.kind == kind)
            .expect("owned image retirement retains its canonical registry record");
        if !record.retiring {
            record.retiring = true;
            self.retirements += 1;
        }
    }
    pub(super) fn pending_retirement_count(&self) -> usize {
        self.retirements
    }
    pub(super) fn pending_retirement(
        &self,
    ) -> impl Iterator<Item = crate::windows::controls::ControlImage32> + '_ {
        self.records
            .iter()
            .filter(|record| record.retiring)
            .map(|record| crate::windows::controls::ControlImage32 {
                kind: record.kind,
                handle: record.handle,
            })
    }
    pub(super) fn publish(&mut self, record: ImageRecord32) {
        assert!(self.records.len() < self.records.capacity());
        assert!(self.records.iter().all(|old| old.handle != record.handle));
        self.records.push(record);
    }
    pub(super) fn retire(&mut self, handle: u32) {
        if let Some(index) = self
            .records
            .iter()
            .position(|record| record.handle == handle)
        {
            self.retirements -= usize::from(self.records[index].retiring);
            self.records.remove(index);
        }
    }
    pub(super) fn prune(&mut self, mut current: impl FnMut(&ImageRecord32) -> bool) {
        self.records.retain(|record| current(record));
        self.retirements = self.records.iter().filter(|record| record.retiring).count();
    }
    pub(in crate::process::windows::user32) fn retire_thread(&mut self, thread: u32) {
        if self
            .reply
            .as_ref()
            .is_some_and(|reply| reply.thread == thread)
        {
            self.reply = None;
        }
    }
    pub(in crate::process::windows::user32) fn clear(&mut self) {
        self.records.clear();
        self.retirements = 0;
        self.reply = None;
    }
}
