use super::{
    RawInputRuntime32,
    types::{NO_LEGACY, RawRegistration32},
};
use crate::windows::user32::User32Error32 as E;
impl RawInputRuntime32 {
    pub(crate) fn prepare_registrations(
        &self,
        rows: &[RawRegistration32],
    ) -> Result<Vec<RawRegistration32>, E> {
        if rows.len() > self.limit {
            return Err(E::Capacity);
        }
        let mut next = Vec::new();
        next.try_reserve_exact(
            self.registrations
                .len()
                .checked_add(rows.len())
                .ok_or(E::Capacity)?,
        )
        .map_err(|_| E::Allocation)?;
        next.extend_from_slice(&self.registrations);
        for &row in rows {
            let keyboard = row.page == 1 && row.usage == 6;
            let mouse = row.page == 1 && row.usage == 2;
            let nolegacy = row.flags & NO_LEGACY == NO_LEGACY;
            if row.page == 0
                || row.flags & !0x3731 != 0
                || row.flags & 1 != 0 && (row.window != 0 || row.flags != 1)
                || row.flags & 0x100 != 0 && row.window == 0
                || row.flags & 0x1000 != 0 && row.window == 0
                || nolegacy && !keyboard && !mouse
                || !nolegacy && row.flags & 0x20 != 0 && row.usage != 0
                || row.usage == 0 && row.flags & 0x20 == 0 && row.flags & 1 == 0
                || row.flags & 0x200 != 0 && !keyboard && (!mouse || !nolegacy)
                || row.flags & 0x400 != 0 && (!keyboard || !nolegacy)
            {
                return Err(E::Win32(87));
            }
            if let Some(index) = next
                .iter()
                .position(|r| r.page == row.page && r.usage == row.usage)
            {
                next.remove(index);
            }
            if row.flags & 1 == 0 {
                next.push(row);
            }
        }
        if next.len() > self.limit {
            return Err(E::Capacity);
        }
        for row in &next {
            if row.flags & NO_LEGACY == 0x10
                && !next
                    .iter()
                    .any(|page| page.page == row.page && page.usage == 0 && page.flags & 0x20 != 0)
            {
                return Err(E::Win32(87));
            }
        }
        Ok(next)
    }
    pub(crate) fn commit_registrations(&mut self, rows: Vec<RawRegistration32>) {
        self.registrations = rows;
    }
    pub(super) fn registration(&self, usage: u16) -> Option<RawRegistration32> {
        let exact = self
            .registrations
            .iter()
            .find(|r| r.page == 1 && r.usage == usage);
        if exact.is_some_and(|r| r.flags & NO_LEGACY == 0x10) {
            return None;
        }
        exact
            .or_else(|| {
                self.registrations
                    .iter()
                    .find(|r| r.page == 1 && r.usage == 0 && r.flags & 0x20 != 0)
            })
            .copied()
    }
    pub(crate) fn no_legacy(&self, keyboard: bool) -> bool {
        self.registration(if keyboard { 6 } else { 2 })
            .is_some_and(|r| r.flags & NO_LEGACY == NO_LEGACY)
    }
}
