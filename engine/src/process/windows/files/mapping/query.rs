use super::context::*;
use crate::{
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions},
    windows::memory::MemoryInfo32,
};

pub(super) fn raw_protection(permissions: Permissions, copy: bool) -> u32 {
    let guard = if permissions.is_guarded() { 0x100 } else { 0 };
    let permissions = permissions.without_guard();
    let bits = u8::from(permissions.allows(crate::memory::Access::Read))
        | u8::from(permissions.allows(crate::memory::Access::Write)) * 2
        | u8::from(permissions.allows(crate::memory::Access::Execute)) * 4;
    guard
        | match (bits, copy) {
            (0, _) => 1,
            (1, _) => 2,
            (3, false) => 4,
            (3, true) => 8,
            (4, _) => 0x10,
            (5, _) => 0x20,
            (7, false) => 0x40,
            (7, true) => 0x80,
            _ => 1,
        }
}
impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn memory_info(
        &self,
        memory: &AddressSpace,
        address: u32,
    ) -> Option<MemoryInfo32> {
        let page = address / PAGE_SIZE;
        let view = self.views.iter().find(|view| {
            page as usize >= view.range.first
                && (page as usize) < view.range.first + view.range.count
        })?;
        let object = &self.objects[self.index(view.object).ok()?];
        let first = page as usize - view.range.first;
        let describe = |index: usize| {
            let offset = view.offset + index as u64 * u64::from(PAGE_SIZE);
            let committed = !object.protection.reserve
                || self
                    .pages
                    .iter()
                    .filter(|pages| pages.key == object.key)
                    .find_map(|pages| {
                        pages
                            .offsets
                            .binary_search(&offset)
                            .ok()
                            .map(|at| pages.committed[at])
                    })
                    .unwrap_or(false);
            let permissions = memory
                .permissions_at(GuestAddress(view.address + index as u32 * PAGE_SIZE))
                .ok()?;
            let expected = match view.protections[index] & 0xff {
                1 => Permissions::NONE,
                2 => Permissions::READ,
                4 | 8 => Permissions::READ_WRITE,
                0x10 => Permissions::EXECUTE,
                0x20 => Permissions::READ_EXECUTE,
                0x40 | 0x80 => Permissions::ALL,
                _ => Permissions::NONE,
            };
            let expected = if view.protections[index] & 0x100 != 0 {
                expected.with_guard()
            } else {
                expected
            };
            let mut raw = if expected == permissions {
                view.protections[index]
            } else {
                raw_protection(permissions, view.copy)
            };
            if matches!(raw & !0x100, 8 | 0x80)
                && memory
                    .content_version_at(GuestAddress(view.address + index as u32 * PAGE_SIZE))
                    .ok()?
                    != view.protection_versions[index]
                && self
                    .pages
                    .iter()
                    .filter(|pages| pages.key == object.key)
                    .find_map(|pages| {
                        pages.offsets.binary_search(&offset).ok().map(|at| {
                            !memory.is_shared_alias(
                                GuestAddress(view.address + index as u32 * PAGE_SIZE),
                                &pages.backing,
                                at,
                            )
                        })
                    })
                    .unwrap_or(false)
            {
                raw = (raw & 0x100) | if raw & 0xff == 8 { 4 } else { 0x40 };
            }
            Some((
                if committed { 0x1000 } else { 0x2000 },
                if committed { raw } else { 0 },
            ))
        };
        let (state, protection) = describe(first)?;
        let mut count = 1usize;
        while first + count < view.range.count
            && describe(first + count) == Some((state, protection))
        {
            count += 1;
        }
        Some(MemoryInfo32 {
            base: page * PAGE_SIZE,
            allocation_base: view.address,
            allocation_protection: raw_protection(view.permissions, view.copy),
            size: count as u32 * PAGE_SIZE,
            state,
            protection,
            kind: 0x40000,
        })
    }
    pub(in crate::process::windows::files) fn record_protection(
        &mut self,
        memory: &AddressSpace,
        range: PageRange,
        raw: u32,
    ) {
        for view in &mut self.views {
            let start = range.first.max(view.range.first);
            let end = (range.first + range.count).min(view.range.first + view.range.count);
            if start < end {
                for page in start..end {
                    let index = page - view.range.first;
                    view.protections[index] = raw;
                    view.protection_versions[index] = memory
                        .content_version_at(GuestAddress(page as u32 * PAGE_SIZE))
                        .expect("preflighted mapped protection");
                }
            }
        }
    }
}
