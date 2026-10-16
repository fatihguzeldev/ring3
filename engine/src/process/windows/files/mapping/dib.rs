use super::{context::MappingViewPurpose32, views::MapViewRequest32};
use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange},
    process::windows::files::{
        FileRuntime32, handles::KernelFileObject32, ordinary::FileCallOutcome32,
    },
};

#[derive(Clone, Copy)]
pub(in crate::process) struct DibSection32 {
    pub range: PageRange,
    pub bits: u32,
    pub section: u32,
    pub offset: u32,
    pub identity: u64,
    pub incarnation: u64,
    object: u64,
}
impl FileRuntime32 {
    pub(in crate::process) fn map_dib_section(
        &mut self,
        memory: &mut AddressSpace,
        reserved: &[PageRange],
        args: &[u32],
        length: usize,
    ) -> Result<FileCallOutcome32, FileError32> {
        let KernelFileObject32::Mapping(object) =
            self.handles.object(args[4]).map_err(|error| {
                if error == FileError32::InvalidHandle {
                    FileError32::InvalidParameter
                } else {
                    error
                }
            })?
        else {
            return Err(FileError32::InvalidParameter);
        };
        let entry = &self.mappings.objects[self.mappings.index(object)?];
        if entry.protection.execute
            || (!entry.protection.write && !entry.protection.copy)
            || entry.protection.reserve
        {
            return Err(FileError32::InvalidParameter);
        }
        let access = if entry.protection.copy { 1 } else { 2 };
        if self.handles.access(args[4])? & access != access {
            return Err(FileError32::AccessDenied);
        }
        let offset = args[5];
        if !offset.is_multiple_of(4) {
            return Ok(FileCallOutcome32::Return {
                value: 0,
                last_error: None,
            });
        }
        if u64::from(offset)
            .checked_add(length as u64)
            .is_none_or(|end| end > entry.size)
        {
            return Err(FileError32::InvalidParameter);
        }
        let within = offset % PAGE_SIZE;
        self.map_file_view(
            memory,
            reserved,
            MapViewRequest32 {
                object,
                access,
                offset: u64::from(offset - within),
                length: u32::try_from(length)
                    .ok()
                    .and_then(|length| length.checked_add(within))
                    .ok_or(FileError32::Capacity)?,
                base: 0,
                purpose: MappingViewPurpose32::Dib {
                    output: args[3],
                    within,
                },
            },
        )
    }
    pub(in crate::process) fn dib_section_lease(
        &self,
        memory: &AddressSpace,
        section: u32,
        bits: u32,
        offset: u32,
    ) -> DibSection32 {
        let KernelFileObject32::Mapping(object) = self
            .handles
            .object(section)
            .expect("accepted section handle")
        else {
            unreachable!()
        };
        let base = bits - offset % PAGE_SIZE;
        let view = self
            .mappings
            .views
            .iter()
            .find(|view| view.object == object && view.address == base)
            .expect("accepted DIB view");
        DibSection32 {
            range: view.range,
            bits,
            section,
            offset,
            object,
            identity: memory.identity(),
            incarnation: memory
                .mapping_incarnation_at(GuestAddress(base))
                .expect("observed accepted DIB view"),
        }
    }
    pub(in crate::process) fn unmap_dib_section(
        &mut self,
        memory: &mut AddressSpace,
        lease: DibSection32,
    ) -> Result<FileCallOutcome32, FileError32> {
        if lease.identity != memory.identity()
            || !self
                .mappings
                .views
                .iter()
                .any(|view| view.object == lease.object && view.range == lease.range)
        {
            return Err(FileError32::Stale);
        }
        for page in lease.range.indices() {
            if memory
                .mapping_incarnation_at(GuestAddress(page as u32 * PAGE_SIZE))
                .map_err(super::views::memory_error)?
                != lease.incarnation
            {
                return Err(FileError32::Stale);
            }
        }
        self.unmap_file_view(memory, lease.range.first as u32 * PAGE_SIZE)
    }
}
