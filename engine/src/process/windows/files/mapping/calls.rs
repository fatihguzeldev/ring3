use super::{context::*, protection::MappingProtection32};
use crate::{
    files::FileError32,
    memory::{AddressSpace, PageRange},
    process::windows::files::{
        FileRuntime32,
        handles::KernelFileObject32,
        ordinary::{FileCallOutcome32, memory as guest},
    },
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn execute_mapping(
        &mut self,
        memory: &mut AddressSpace,
        reserved: &[PageRange],
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        match api {
            CreateFileMappingA | CreateFileMappingW => {
                self.create_mapping(memory, args, api == CreateFileMappingW)
            }
            OpenFileMappingA | OpenFileMappingW => {
                self.open_mapping(memory, args, api == OpenFileMappingW)
            }
            MapViewOfFile | MapViewOfFileEx => {
                let KernelFileObject32::Mapping(id) = self.handles.object(args[0])? else {
                    return Err(FileError32::InvalidHandle);
                };
                let access = args[1];
                let needed =
                    if access & 2 != 0 { 2 } else { 4 } | if access & 0x20 != 0 { 0x20 } else { 0 };
                if self.handles.access(args[0])? & needed != needed {
                    return Err(FileError32::AccessDenied);
                }
                self.map_file_view(
                    memory,
                    reserved,
                    super::views::MapViewRequest32 {
                        object: id,
                        access,
                        offset: u64::from(args[2]) << 32 | u64::from(args[3]),
                        length: args[4],
                        base: if api == MapViewOfFileEx { args[5] } else { 0 },
                        purpose: MappingViewPurpose32::Ordinary,
                    },
                )
            }
            UnmapViewOfFile => self.unmap_file_view(memory, args[0]),
            FlushViewOfFile => self.flush_file_view(memory, args[0], args[1]),
            _ => Err(FileError32::InvalidParameter),
        }
    }
    fn create_mapping(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
        wide: bool,
    ) -> Result<FileCallOutcome32, FileError32> {
        let inherit = guest::inherit(memory, args[1])?;
        let name = if args[5] == 0 {
            None
        } else {
            Some(mapping_name(guest::path(
                memory,
                args[5],
                wide,
                self.ordinary.ansi,
            )?)?)
        };
        let protection = MappingProtection32::parse(args[2], args[0] == u32::MAX)?;
        if let Some(index) = name.as_ref().and_then(|name| {
            self.mappings
                .objects
                .iter()
                .position(|object| object.handles != 0 && object.name.as_ref() == Some(name))
        }) {
            let id = self.mappings.objects[index].id;
            if self.mappings.objects[index].references == u32::MAX {
                return Err(FileError32::Exhausted);
            }
            let handle = self.handles.prepare(0x000f_003f, inherit)?;
            self.mappings.retain(id)?;
            let value = self.handles.commit(handle, KernelFileObject32::Mapping(id));
            return Ok(FileCallOutcome32::Return {
                value,
                last_error: Some(183),
            });
        }
        if self.mappings.objects.len() >= self.mappings.limits.objects {
            return Err(FileError32::Capacity);
        }
        let id = self.mappings.next;
        let next = id.checked_add(1).ok_or(FileError32::Exhausted)?;
        let maximum = u64::from(args[3]) << 32 | u64::from(args[4]);
        let (source, key, size, resize) = if args[0] == u32::MAX {
            if maximum == 0 {
                return Err(FileError32::InvalidParameter);
            }
            (None, MappingBacking32::PageFile(id), maximum, None)
        } else {
            let open = self.handles.file(args[0])?;
            let file = self.vfs.file_for_open(open)?;
            if !self
                .vfs
                .open_access(open)?
                .contains(protection.file_access())
                || !crate::files::FileAccess32(self.handles.access(args[0])?)
                    .contains(protection.file_access())
            {
                return Err(FileError32::AccessDenied);
            }
            let old_size = self.vfs.info(file)?.size;
            let size = if maximum == 0 { old_size } else { maximum };
            if size == 0 {
                return Err(FileError32::FileInvalid);
            }
            if size > old_size && !protection.write {
                return Err(FileError32::AccessDenied);
            }
            self.vfs.check_retain_open(open)?;
            let resize = if size > old_size {
                Some(self.vfs.prepare_resize(file, size)?)
            } else {
                None
            };
            (Some(open), MappingBacking32::File(file), size, resize)
        };
        let prepared = self.handles.prepare(0x000f_003f, inherit)?;
        let mut initial_offsets = Vec::new();
        let mut initial_flushed = Vec::new();
        let mut committed = Vec::new();
        let memory_prepared = if source.is_none() {
            let count = u32::try_from(size.div_ceil(u64::from(crate::memory::PAGE_SIZE)))
                .map_err(|_| FileError32::Capacity)?;
            let prepared = memory
                .prepare_shared_pages(count, &[])
                .map_err(super::views::memory_error)?;
            committed
                .try_reserve_exact(count as usize)
                .map_err(|_| FileError32::Allocation)?;
            committed.resize(count as usize, !protection.reserve);
            initial_offsets
                .try_reserve_exact(count as usize)
                .map_err(|_| FileError32::Allocation)?;
            initial_flushed
                .try_reserve_exact(count as usize)
                .map_err(|_| FileError32::Allocation)?;
            for index in 0..count {
                initial_offsets.push(u64::from(index) * u64::from(crate::memory::PAGE_SIZE));
            }
            self.mappings
                .pages
                .try_reserve(1)
                .map_err(|_| FileError32::Allocation)?;
            Some(prepared)
        } else {
            None
        };
        if let Some(resize) = &resize {
            resize.validate(&self.vfs)?;
        }
        if let Some(open) = source {
            self.vfs
                .retain_open(open)
                .expect("open retention checked before mapping publication");
        }
        if let Some(resize) = resize {
            self.vfs.commit_resize(resize);
        }
        if let Some(prepared) = memory_prepared {
            let backing = memory.commit_shared_pages(prepared);
            for index in 0..initial_offsets.len() {
                initial_flushed.push(memory.shared_content_version(&backing, index).unwrap());
            }
            self.mappings.pages.push(MappingPage32 {
                key,
                offsets: initial_offsets,
                backing,
                flushed: initial_flushed,
                committed,
            });
        }
        self.mappings.next = next;
        self.mappings.objects.push(MappingObject32 {
            id,
            source,
            key,
            size,
            protection,
            name,
            references: 1,
            handles: 1,
        });
        let value = self
            .handles
            .commit(prepared, KernelFileObject32::Mapping(id));
        Ok(FileCallOutcome32::Return {
            value,
            last_error: Some(0),
        })
    }
    fn open_mapping(
        &mut self,
        memory: &AddressSpace,
        args: &[u32],
        wide: bool,
    ) -> Result<FileCallOutcome32, FileError32> {
        let name = mapping_name(guest::path(memory, args[2], wide, self.ordinary.ansi)?)?;
        let index = self
            .mappings
            .objects
            .iter()
            .position(|object| object.handles != 0 && object.name.as_ref() == Some(&name))
            .ok_or(FileError32::NotFound)?;
        self.mappings.objects[index].protection.view(args[0])?;
        let id = self.mappings.objects[index].id;
        if self.mappings.objects[index].references == u32::MAX {
            return Err(FileError32::Exhausted);
        }
        let prepared = self.handles.prepare(
            if args[0] & 1 != 0 {
                args[0] | 4
            } else {
                args[0]
            },
            args[1] != 0,
        )?;
        self.mappings.retain(id)?;
        Ok(FileCallOutcome32::success(
            self.handles
                .commit(prepared, KernelFileObject32::Mapping(id)),
        ))
    }
}
fn mapping_name(mut name: Vec<u16>) -> Result<Vec<u16>, FileError32> {
    if name.is_empty() || name.len() > 260 {
        return Err(FileError32::InvalidName);
    }
    let local: [u16; 6] = [76, 111, 99, 97, 108, 92];
    let global: [u16; 7] = [71, 108, 111, 98, 97, 108, 92];
    if name.starts_with(&global) {
        return Err(FileError32::Unsupported);
    }
    if name.starts_with(&local) {
        name.drain(..local.len());
    }
    let rest = if name.starts_with(&global) {
        &name[global.len()..]
    } else {
        &name
    };
    if rest.is_empty() || rest.contains(&92) {
        return Err(FileError32::InvalidName);
    }
    Ok(name)
}

impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn has_named_mapping(
        &self,
        name: &[u16],
    ) -> Result<bool, FileError32> {
        if name.starts_with(&[71, 108, 111, 98, 97, 108, 92]) {
            return Err(FileError32::Unsupported);
        }
        let name = name
            .strip_prefix(&[76, 111, 99, 97, 108, 92])
            .unwrap_or(name);
        if name.is_empty() || name.len() > 260 || name.contains(&0) || name.contains(&92) {
            return Err(FileError32::InvalidName);
        }
        Ok(self
            .objects
            .iter()
            .any(|object| object.handles != 0 && object.name.as_deref() == Some(name)))
    }
}
