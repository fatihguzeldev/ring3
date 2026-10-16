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

pub(crate) fn module_source_path(name: &str) -> Result<Option<String>, ModuleError32> {
    if !name.contains(['/', '\\', ':']) {
        return Ok(None);
    }
    if name.ends_with(['/', '\\']) {
        return Err(ModuleError32::InvalidName);
    }
    if !(name.as_bytes().get(1) == Some(&b':')
        && name
            .as_bytes()
            .get(2)
            .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
        || name.starts_with("\\\\"))
    {
        return Err(ModuleError32::InvalidName);
    }
    let mut path = crate::files::path::normalize(
        &name.encode_utf16().collect::<Vec<_>>(),
        &[67, 58, 92],
        false,
    )
    .map_err(|_| ModuleError32::InvalidName)?;
    if !crate::files::path::name(&path).contains(&46) {
        path.extend(".dll".encode_utf16());
    }
    String::from_utf16(&path)
        .map(Some)
        .map_err(|_| ModuleError32::InvalidName)
}

pub fn normalize_module_identity(name: &str) -> Result<String, ModuleError32> {
    let Some(path) = module_source_path(name)? else {
        return normalize_module_name(name);
    };
    String::from_utf16(&crate::files::path::fold(
        &path.encode_utf16().collect::<Vec<_>>(),
    ))
    .map_err(|_| ModuleError32::InvalidName)
}
