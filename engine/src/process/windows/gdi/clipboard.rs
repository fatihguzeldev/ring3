use super::{GdiRuntime32, storage::SurfaceKey32};
use crate::{
    memory::{AddressSpace, PageRange},
    process::{EngineInstance, HostError},
    windows::gdi::{GdiClipboardObject32, GdiError32},
};
struct BitmapWitness32 {
    handle: u32,
    id: u64,
    revision: u64,
}
pub(in crate::process) struct GdiClipboardRetirement32 {
    identity: u64,
    version: u64,
    retire: Vec<GdiClipboardObject32>,
    deferred: Vec<GdiClipboardObject32>,
    witnesses: Vec<BitmapWitness32>,
    pub(in crate::process) ranges: Vec<PageRange>,
    pub(in crate::process) sections: Vec<crate::process::windows::files::DibSection32>,
}
impl GdiRuntime32 {
    pub(in crate::process) fn capture_clipboard_object(
        &self,
        ram: &AddressSpace,
        format: u32,
        handle: u32,
    ) -> Result<GdiClipboardObject32, GdiError32> {
        let object = self.model.capture_clipboard_object(format, handle)?;
        self.check_clipboard_storage(ram, object)?;
        Ok(object)
    }
    pub(in crate::process) fn check_clipboard_object(
        &self,
        ram: &AddressSpace,
        object: GdiClipboardObject32,
    ) -> Result<(), GdiError32> {
        self.model.check_clipboard_object(object)?;
        self.check_clipboard_storage(ram, object)
    }
    fn check_clipboard_storage(
        &self,
        ram: &AddressSpace,
        object: GdiClipboardObject32,
    ) -> Result<(), GdiError32> {
        if let GdiClipboardObject32::Bitmap(handle) = object {
            let surface = &self.surfaces[self.surface_index(SurfaceKey32::Bitmap(handle))?];
            surface.storage.check(ram)?;
        }
        Ok(())
    }
    pub(in crate::process) fn prepare_clipboard_retirement(
        &self,
        ram: &AddressSpace,
        objects: &[GdiClipboardObject32],
    ) -> Result<GdiClipboardRetirement32, GdiError32> {
        let mut plan = GdiClipboardRetirement32 {
            identity: ram.identity(),
            version: ram.transaction_version(),
            retire: Vec::new(),
            deferred: Vec::new(),
            witnesses: Vec::new(),
            ranges: Vec::new(),
            sections: Vec::new(),
        };
        let count = objects.len() + self.model.retired_clipboard_objects().count();
        plan.retire
            .try_reserve_exact(count)
            .map_err(|_| GdiError32::Allocation)?;
        plan.deferred
            .try_reserve_exact(objects.len())
            .map_err(|_| GdiError32::Allocation)?;
        plan.witnesses
            .try_reserve_exact(count)
            .map_err(|_| GdiError32::Allocation)?;
        plan.ranges
            .try_reserve_exact(count)
            .map_err(|_| GdiError32::Allocation)?;
        plan.sections
            .try_reserve_exact(count)
            .map_err(|_| GdiError32::Allocation)?;
        for object in objects {
            self.check_clipboard_object(ram, *object)?;
        }
        for object in objects
            .iter()
            .copied()
            .chain(self.model.retired_clipboard_objects())
        {
            if plan.retire.contains(&object) || plan.deferred.contains(&object) {
                continue;
            }
            if self.model.object_selected(object.handle()) {
                if objects.contains(&object) {
                    plan.deferred.push(object);
                }
                continue;
            }
            self.check_clipboard_storage(ram, object)?;
            if let GdiClipboardObject32::Bitmap(handle) = object {
                let surface = &self.surfaces[self.surface_index(SurfaceKey32::Bitmap(handle))?];
                plan.witnesses.push(BitmapWitness32 {
                    handle,
                    id: surface.id,
                    revision: surface.revision,
                });
                if let Some(range) = surface.storage.range() {
                    plan.ranges.push(range);
                }
                if let Some(section) = surface.storage.section() {
                    plan.sections.push(section);
                }
            }
            plan.retire.push(object);
        }
        Ok(plan)
    }
    pub(in crate::process) fn check_clipboard_retirement(
        &self,
        ram: &AddressSpace,
        plan: &GdiClipboardRetirement32,
    ) -> Result<(), GdiError32> {
        if ram.identity() != plan.identity || ram.transaction_version() != plan.version {
            return Err(GdiError32::InvalidRequest);
        }
        for object in &plan.retire {
            if self.model.object_selected(object.handle()) {
                return Err(GdiError32::InvalidRequest);
            }
            self.check_clipboard_storage(ram, *object)?;
        }
        for witness in &plan.witnesses {
            let surface =
                &self.surfaces[self.surface_index(SurfaceKey32::Bitmap(witness.handle))?];
            if (surface.id, surface.revision) != (witness.id, witness.revision) {
                return Err(GdiError32::InvalidRequest);
            }
        }
        for object in &plan.deferred {
            self.model.check_clipboard_object(*object)?;
        }
        Ok(())
    }
    pub(in crate::process) fn commit_clipboard_retirement(
        &mut self,
        plan: GdiClipboardRetirement32,
    ) {
        for object in plan.retire {
            if let GdiClipboardObject32::Bitmap(handle) = object {
                let index = self
                    .surface_index(SurfaceKey32::Bitmap(handle))
                    .expect("preflighted retired bitmap");
                let surface = self.surfaces.remove(index);
                self.bytes -= surface.layout.byte_len().unwrap();
            }
            self.model.commit_clipboard_retirement(object);
        }
        for object in plan.deferred {
            self.model.defer_clipboard_retirement(object);
        }
    }
    pub(in crate::process) fn commit_clipboard_transfer(&mut self, object: GdiClipboardObject32) {
        self.model.commit_clipboard_transfer(object);
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_gdi_clipboard_retirement(
        &self,
        objects: &[GdiClipboardObject32],
    ) -> Result<Option<GdiClipboardRetirement32>, HostError> {
        self.gdi
            .as_ref()
            .map(|gdi| {
                gdi.prepare_clipboard_retirement(
                    self.memory.as_ref().ok_or(HostError::Closed)?,
                    objects,
                )
                .map_err(HostError::Gdi)
            })
            .transpose()
    }
}

impl EngineInstance {
    pub(in crate::process) fn retire_clipboard_graphics(
        &mut self,
        objects: &[GdiClipboardObject32],
    ) -> Result<Option<crate::files::FileReadRequest32>, HostError> {
        let Some(plan) = self.prepare_gdi_clipboard_retirement(objects)? else {
            if objects.is_empty() {
                return Ok(None);
            }
            return Err(HostError::Gdi(GdiError32::InvalidConfig));
        };
        if plan.retire.is_empty() && plan.deferred.is_empty() {
            return Ok(None);
        }
        self.gdi
            .as_ref()
            .unwrap()
            .check_clipboard_retirement(self.memory.as_ref().unwrap(), &plan)
            .map_err(HostError::Gdi)?;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(plan.ranges.len() + plan.sections.len())
            .map_err(|_| HostError::Gdi(GdiError32::Allocation))?;
        ranges.extend_from_slice(&plan.ranges);
        ranges.extend(plan.sections.iter().map(|lease| lease.range));
        ranges.sort_unstable_by_key(|range| range.first);
        for range in &ranges {
            for page in range.indices() {
                self.memory
                    .as_ref()
                    .unwrap()
                    .permissions_at(crate::memory::GuestAddress(
                        page as u32 * crate::memory::PAGE_SIZE,
                    ))
                    .map_err(HostError::Memory)?;
            }
        }
        self.memory
            .as_ref()
            .unwrap()
            .ensure_transaction_versions(u64::from(!ranges.is_empty()))
            .map_err(HostError::Memory)?;
        let thread = self.current_windows_thread_id()?;
        let files = if let Some(files) = self.files.as_mut() {
            match files
                .prepare_dib_retirement(
                    self.memory.as_ref().unwrap(),
                    &plan.sections,
                    &plan.ranges,
                    thread,
                )
                .map_err(HostError::File)?
            {
                crate::files::FileIo32::NeedBytes(request) => return Ok(Some(request)),
                crate::files::FileIo32::Ready(receipt) => Some(receipt),
            }
        } else {
            if !plan.sections.is_empty() {
                return Err(HostError::Gdi(GdiError32::InvalidConfig));
            }
            None
        };
        self.gdi
            .as_ref()
            .unwrap()
            .check_clipboard_retirement(self.memory.as_ref().unwrap(), &plan)
            .map_err(HostError::Gdi)?;
        if let Some(receipt) = files {
            self.files
                .as_mut()
                .unwrap()
                .commit_dib_retirement(self.memory.as_mut().unwrap(), receipt);
        } else if !plan.ranges.is_empty() {
            self.memory
                .as_mut()
                .unwrap()
                .unmap_ranges_with_word_writes(&plan.ranges, &[])
                .map_err(HostError::Memory)?;
        }
        self.gdi.as_mut().unwrap().commit_clipboard_retirement(plan);
        Ok(None)
    }
    pub(in crate::process::windows::gdi) fn collect_retired_clipboard_graphics(
        &mut self,
    ) -> Result<Option<crate::files::FileReadRequest32>, HostError> {
        if self
            .gdi
            .as_ref()
            .is_none_or(|gdi| gdi.model.retired_clipboard_objects().next().is_none())
        {
            return Ok(None);
        }
        self.retire_clipboard_graphics(&[])
    }
}
