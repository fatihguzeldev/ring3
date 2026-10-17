use super::ComdlgError32 as E;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FileFilter32 {
    pub label: Vec<u16>,
    pub pattern: Vec<u16>,
}

#[derive(Default)]
pub(crate) struct FileFilters32 {
    pub entries: Vec<FileFilter32>,
    pub custom: Option<FileFilter32>,
}

impl FileFilters32 {
    pub(crate) fn parse(units: &[u16], limit: usize) -> Result<Vec<FileFilter32>, E> {
        let mut entries = Vec::new();
        let mut at = 0;
        loop {
            let (label, next) = field(units, at)?;
            at = next;
            if label.is_empty() {
                if at != units.len() {
                    return Err(E::InvalidRequest);
                }
                return Ok(entries);
            }
            let (pattern, next) = field(units, at)?;
            at = next;
            if entries.len() == limit {
                return Err(E::Capacity);
            }
            entries.try_reserve(1).map_err(|_| E::Allocation)?;
            entries.push(FileFilter32 {
                label: copy(label)?,
                pattern: copy(pattern)?,
            });
        }
    }

    pub(crate) fn selected(&self, index: u32) -> Option<&FileFilter32> {
        if index == 0 {
            self.custom.as_ref().or_else(|| self.entries.first())
        } else {
            self.entries
                .get(index as usize - 1)
                .or_else(|| self.entries.first())
        }
    }
}

impl FileFilter32 {
    pub(crate) fn matches(&self, name: &[u16]) -> bool {
        self.pattern
            .split(|&unit| unit == 59)
            .filter(|pattern| !pattern.is_empty())
            .any(|pattern| crate::files::path::matches(pattern, name))
    }
}

fn field(units: &[u16], at: usize) -> Result<(&[u16], usize), E> {
    let remaining = units.get(at..).ok_or(E::InvalidRequest)?;
    let length = remaining
        .iter()
        .position(|&unit| unit == 0)
        .ok_or(E::InvalidRequest)?;
    Ok((&remaining[..length], at + length + 1))
}

fn copy(units: &[u16]) -> Result<Vec<u16>, E> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(units.len())
        .map_err(|_| E::Allocation)?;
    copy.extend_from_slice(units);
    Ok(copy)
}
