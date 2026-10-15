use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessError32 {
    InvalidParameter,
    Capacity,
    Allocation,
    NoAccess,
    NotFound,
}
impl ProcessError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidParameter => 87,
            Self::Capacity | Self::Allocation => 8,
            Self::NoAccess => 998,
            Self::NotFound => 203,
        }
    }
}

/// A process-owned environment. Host variables are never imported implicitly.
#[derive(Clone, Debug)]
pub struct Environment32 {
    values: BTreeMap<Vec<u16>, (String, String)>,
    limit: usize,
}
impl Environment32 {
    pub fn new(limit: usize) -> Result<Self, ProcessError32> {
        if !(2..=16 * 1024 * 1024).contains(&limit) {
            return Err(ProcessError32::InvalidParameter);
        }
        Ok(Self {
            values: BTreeMap::new(),
            limit,
        })
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(&crate::windows::text::ordinal_key(
                &name.encode_utf16().collect::<Vec<_>>(),
            ))
            .map(|(_, value)| value.as_str())
    }
    pub fn set(&mut self, name: &str, value: Option<&str>) -> Result<(), ProcessError32> {
        if name.is_empty()
            || name.contains(['\0', '='])
            || value
                .is_some_and(|value| value.contains('\0') || value.encode_utf16().count() > 32767)
        {
            return Err(ProcessError32::InvalidParameter);
        }
        let key = crate::windows::text::ordinal_key(&name.encode_utf16().collect::<Vec<_>>());
        let old = self.values.get(&key).map_or(0, |(name, value)| {
            name.encode_utf16().count() + value.encode_utf16().count() + 2
        });
        let new = value.map_or(0, |value| {
            name.encode_utf16().count() + value.encode_utf16().count() + 2
        });
        let used = self
            .values
            .values()
            .map(|(name, value)| name.encode_utf16().count() + value.encode_utf16().count() + 2)
            .sum::<usize>();
        if (used - old + new + 1).max(2) > self.limit {
            return Err(ProcessError32::Capacity);
        }
        if let Some(value) = value {
            self.values.insert(key, (name.into(), value.into()));
        } else {
            self.values.remove(&key);
        }
        Ok(())
    }
    pub fn block(&self) -> Vec<u16> {
        let mut block = Vec::new();
        for (name, value) in self.values.values() {
            block.extend(name.encode_utf16());
            block.push(b'=' as u16);
            block.extend(value.encode_utf16());
            block.push(0);
        }
        if block.is_empty() {
            block.push(0);
        }
        block.push(0);
        block
    }
    pub fn expand(&self, source: &str) -> String {
        let mut output = String::new();
        let mut rest = source;
        while let Some(start) = rest.find('%') {
            output.push_str(&rest[..start]);
            rest = &rest[start..];
            let Some(end) = rest[1..].find('%').map(|end| end + 1) else {
                break;
            };
            let name = &rest[1..end];
            output.push_str(self.get(name).unwrap_or(&rest[..=end]));
            rest = &rest[end + 1..];
        }
        output.push_str(rest);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_preserves_empty_values_and_snapshot_order() {
        let mut env = Environment32::new(64).unwrap();
        env.set("z", Some("last")).unwrap();
        env.set("Path", Some("C:\\game")).unwrap();
        env.set("EMPTY", Some("")).unwrap();
        let snapshot = env.block();
        env.set("pATH", Some("new")).unwrap();
        assert_eq!(env.get("PATH"), Some("new"));
        assert_eq!(env.get("empty"), Some(""));
        assert_eq!(
            String::from_utf16(&snapshot).unwrap(),
            "EMPTY=\0Path=C:\\game\0z=last\0\0"
        );
        assert_eq!(
            env.expand("%path%/%unknown%/%EMPTY%/tail%"),
            "new/%unknown%//tail%"
        );
    }
    #[test]
    fn capacity_rejection_is_neutral_and_delete_releases_capacity() {
        let mut env = Environment32::new(8).unwrap();
        env.set("A", Some("1234")).unwrap();
        let before = env.block();
        assert_eq!(env.set("B", Some("x")), Err(ProcessError32::Capacity));
        assert_eq!(env.block(), before);
        env.set("a", None).unwrap();
        assert_eq!(env.block(), [0, 0]);
        env.set("B", Some("x")).unwrap();
    }
}
