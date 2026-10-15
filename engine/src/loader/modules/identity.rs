use super::ModuleError32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId32(pub(super) u64);

impl ModuleId32 {
    pub(super) fn from_sequence(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

pub fn normalize_module_name(name: &str) -> Result<String, ModuleError32> {
    if name.is_empty()
        || name.len() > 255
        || name.ends_with(['.', ' '])
        || name
            .bytes()
            .any(|byte| !(0x20..=0x7e).contains(&byte) || b"/\\:<>\"|?*".contains(&byte))
    {
        return Err(ModuleError32::InvalidName);
    }
    let mut normalized = name.to_ascii_lowercase();
    if !normalized.contains('.') {
        normalized.push_str(".dll");
    }
    if normalized.len() > 255 {
        return Err(ModuleError32::InvalidName);
    }
    Ok(normalized)
}
