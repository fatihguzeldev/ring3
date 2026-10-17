use super::DibSection32;
use crate::{
    files::{FileError32, FileIo32, OpenFileId},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange},
    process::windows::files::{
        FileRuntime32,
        mapping::{context::MappingBacking32, views::memory_error},
    },
};
struct Object32 {
    id: u64,
    references: u32,
    removed: u32,
    handles: u32,
}
pub(in crate::process) struct PreparedDibRetirement32 {
    identity: u64,
    version: u64,
    vfs: u64,
    leases: Vec<DibSection32>,
    ranges: Vec<PageRange>,
    objects: Vec<Object32>,
    keys: Vec<MappingBacking32>,
    closes: Vec<OpenFileId>,
}
impl FileRuntime32 {
    pub(in crate::process) fn prepare_dib_retirement(
        &mut self,
        ram: &AddressSpace,
        leases: &[DibSection32],
        extra: &[PageRange],
        thread: u32,
    ) -> Result<FileIo32<PreparedDibRetirement32>, FileError32> {
        self.vfs.set_io_thread(thread);
        let mut plan = PreparedDibRetirement32 {
            identity: ram.identity(),
            version: ram.transaction_version(),
            vfs: 0,
            leases: Vec::new(),
            ranges: Vec::new(),
            objects: Vec::new(),
            keys: Vec::new(),
            closes: Vec::new(),
        };
        plan.leases
            .try_reserve_exact(leases.len())
            .map_err(|_| FileError32::Allocation)?;
        plan.leases.extend_from_slice(leases);
        plan.ranges
            .try_reserve_exact(leases.len() + extra.len())
            .map_err(|_| FileError32::Allocation)?;
        plan.ranges.extend_from_slice(extra);
        plan.objects
            .try_reserve_exact(leases.len())
            .map_err(|_| FileError32::Allocation)?;
        plan.keys
            .try_reserve_exact(leases.len())
            .map_err(|_| FileError32::Allocation)?;
        plan.closes
            .try_reserve_exact(leases.len())
            .map_err(|_| FileError32::Allocation)?;
        for lease in leases {
            self.check_dib_retirement_lease(ram, *lease)?;
            plan.ranges.push(lease.range);
            if let Some(object) = plan
                .objects
                .iter_mut()
                .find(|object| object.id == lease.object)
            {
                object.removed += 1;
            } else {
                let object = &self.mappings.objects[self.mappings.index(lease.object)?];
                plan.objects.push(Object32 {
                    id: object.id,
                    references: object.references,
                    removed: 1,
                    handles: object.handles,
                });
            }
        }
        plan.ranges.sort_unstable_by_key(|range| range.first);
        let mut end = 0;
        for range in &plan.ranges {
            if range.count == 0 || range.first < end {
                return Err(FileError32::InvalidParameter);
            }
            end = range
                .first
                .checked_add(range.count)
                .ok_or(FileError32::InvalidParameter)?;
            for page in range.indices() {
                ram.permissions_at(GuestAddress(page as u32 * PAGE_SIZE))
                    .map_err(memory_error)?;
            }
        }
        ram.ensure_transaction_versions(u64::from(!plan.ranges.is_empty()))
            .map_err(memory_error)?;
        for object in &plan.objects {
            if object.removed > object.references {
                return Err(FileError32::Stale);
            }
            if object.removed == object.references {
                let entry = &self.mappings.objects[self.mappings.index(object.id)?];
                if let Some(open) = entry.source {
                    plan.closes.push(open);
                }
                let key = entry.key;
                let final_key = self
                    .mappings
                    .objects
                    .iter()
                    .filter(|other| other.key == key)
                    .all(|other| {
                        plan.objects.iter().any(|retire| {
                            retire.id == other.id && retire.removed == retire.references
                        })
                    });
                if final_key && !plan.keys.contains(&key) {
                    plan.keys.push(key);
                }
            }
        }
        for key in &plan.keys {
            if let Some(request) = self.flush_mapping_pages(ram, *key, None)? {
                return Ok(FileIo32::NeedBytes(request));
            }
        }
        let mut closes = Vec::new();
        closes
            .try_reserve_exact(plan.closes.len())
            .map_err(|_| FileError32::Allocation)?;
        closes.extend(plan.closes.iter().map(|open| (*open, 1)));
        self.vfs.check_close_open_group(&closes, 0, &[])?;
        for pages in self
            .mappings
            .pages
            .iter()
            .filter(|pages| plan.keys.contains(&pages.key))
        {
            if !pages.offsets.is_empty() {
                ram.shared_content_version(&pages.backing, 0)
                    .map_err(memory_error)?;
            }
        }
        plan.vfs = self.vfs.revision();
        Ok(FileIo32::Ready(plan))
    }
    fn check_dib_retirement_lease(
        &self,
        ram: &AddressSpace,
        lease: DibSection32,
    ) -> Result<(), FileError32> {
        if ram.identity() != lease.identity
            || !self
                .mappings
                .views
                .iter()
                .any(|view| view.object == lease.object && view.range == lease.range)
        {
            return Err(FileError32::Stale);
        }
        for page in lease.range.indices() {
            if ram
                .mapping_incarnation_at(GuestAddress(page as u32 * PAGE_SIZE))
                .map_err(memory_error)?
                != lease.incarnation
            {
                return Err(FileError32::Stale);
            }
        }
        Ok(())
    }
    pub(in crate::process) fn check_dib_retirement(
        &self,
        ram: &AddressSpace,
        plan: &PreparedDibRetirement32,
    ) -> Result<(), FileError32> {
        if (
            ram.identity(),
            ram.transaction_version(),
            self.vfs.revision(),
        ) != (plan.identity, plan.version, plan.vfs)
        {
            return Err(FileError32::Stale);
        }
        for lease in &plan.leases {
            self.check_dib_retirement_lease(ram, *lease)?;
        }
        for object in &plan.objects {
            let entry = &self.mappings.objects[self.mappings.index(object.id)?];
            if (entry.references, entry.handles) != (object.references, object.handles) {
                return Err(FileError32::Stale);
            }
        }
        Ok(())
    }
    pub(in crate::process) fn commit_dib_retirement(
        &mut self,
        ram: &mut AddressSpace,
        plan: PreparedDibRetirement32,
    ) {
        self.check_dib_retirement(ram, &plan)
            .expect("exact checked DIB retirement authority");
        if !plan.ranges.is_empty() {
            ram.unmap_ranges_with_word_writes(&plan.ranges, &[])
                .expect("checked aggregate DIB ranges");
        }
        self.mappings.views.retain(|view| {
            !plan
                .leases
                .iter()
                .any(|lease| lease.range == view.range && lease.object == view.object)
        });
        for object in &plan.objects {
            let index = self.mappings.index(object.id).unwrap();
            if object.removed == object.references {
                self.mappings.objects.remove(index);
            } else {
                self.mappings.objects[index].references -= object.removed;
            }
        }
        let mut index = 0;
        while index < self.mappings.pages.len() {
            if plan.keys.contains(&self.mappings.pages[index].key) {
                let pages = self.mappings.pages.remove(index);
                ram.release_shared_pages(pages.backing)
                    .expect("checked final backing retained until aggregate unmap");
            } else {
                index += 1;
            }
        }
        for open in plan.closes {
            self.vfs.close_open(open).expect("checked VFS close group");
            self.retire_completion_file(open);
        }
    }
}
