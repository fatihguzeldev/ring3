use super::Selection32;
use crate::{
    memory::AddressSpace,
    process::windows::commdlg::CapturedFileDialog32,
    windows::{commdlg::ComdlgError32, user32::User32Error32 as E},
};

impl Selection32 {
    pub(super) fn publish(
        &self,
        request: &CapturedFileDialog32,
        ram: &mut AddressSpace,
        filter: u32,
    ) -> Result<u32, ComdlgError32> {
        if self.additional.is_empty() {
            return request.publish(ram, &self.path, filter, self.readonly);
        }
        let at = self
            .path
            .iter()
            .rposition(|unit| *unit == 92)
            .ok_or(ComdlgError32::InvalidRequest)?;
        let directory = &self.path[..if at == 2 { 3 } else { at }];
        let mut names = Vec::new();
        names
            .try_reserve_exact(self.additional.len() + 1)
            .map_err(|_| ComdlgError32::Allocation)?;
        for path in std::iter::once(&self.path).chain(self.additional.iter()) {
            let at = path
                .iter()
                .rposition(|unit| *unit == 92)
                .ok_or(ComdlgError32::InvalidRequest)?;
            if path[..if at == 2 { 3 } else { at }] != *directory {
                return Err(ComdlgError32::InvalidRequest);
            }
            names.push(path[at + 1..].to_vec());
        }
        request.publish_multiple(ram, directory, &names, filter, self.readonly)
    }
}

pub(super) fn quoted_names(text: &[u16]) -> Result<Vec<Vec<u16>>, E> {
    let mut names = Vec::new();
    let mut at = 0;
    while at < text.len() {
        while text.get(at) == Some(&32) {
            at += 1;
        }
        if at == text.len() {
            break;
        }
        if text[at] != 34 || names.len() == 4096 {
            return Err(E::Win32(87));
        }
        at += 1;
        let start = at;
        while text.get(at).is_some_and(|unit| *unit != 34) {
            at += 1;
        }
        if at == start || at == text.len() {
            return Err(E::Win32(87));
        }
        names.try_reserve(1).map_err(|_| E::Allocation)?;
        names.push(text[start..at].to_vec());
        at += 1;
        if text.get(at).is_some_and(|unit| *unit != 32) {
            return Err(E::Win32(87));
        }
    }
    Ok(names)
}
