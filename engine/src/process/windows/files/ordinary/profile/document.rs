pub(super) struct ProfileDocument32 {
    lines: Vec<String>,
}
fn same(left: &str, right: &str) -> bool {
    crate::windows::text::ordinal_key(&left.encode_utf16().collect::<Vec<_>>())
        == crate::windows::text::ordinal_key(&right.encode_utf16().collect::<Vec<_>>())
}
fn section(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix('[')?
        .split_once(']')
        .map(|(name, _)| name.trim())
}
fn entry(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.starts_with(';') {
        return None;
    }
    line.split_once('=')
        .map(|(key, value)| (key.trim(), value.trim()))
}
impl ProfileDocument32 {
    pub(super) fn new(text: &str) -> Self {
        Self {
            lines: text.lines().map(str::to_owned).collect(),
        }
    }
    pub(super) fn names(&self, wanted: Option<&str>) -> Vec<String> {
        let mut current = false;
        let mut result = Vec::new();
        for line in &self.lines {
            if let Some(name) = section(line) {
                current = wanted.is_some_and(|wanted| same(wanted, name));
                if wanted.is_none() {
                    result.push(name.to_owned());
                }
            } else if current && let Some((key, _)) = entry(line) {
                result.push(key.to_owned());
            }
        }
        result
    }
    pub(super) fn value(&self, wanted: &str, key: &str) -> Option<String> {
        let mut current = false;
        for line in &self.lines {
            if let Some(name) = section(line) {
                current = same(wanted, name);
            } else if current
                && let Some((name, value)) = entry(line)
                && same(name, key)
            {
                let value = if value.len() >= 2
                    && ((value.starts_with('"') && value.ends_with('"'))
                        || (value.starts_with('\'') && value.ends_with('\'')))
                {
                    &value[1..value.len() - 1]
                } else {
                    value
                };
                return Some(value.to_owned());
            }
        }
        None
    }
    pub(super) fn set(&mut self, name: &str, key: Option<&str>, value: Option<&str>) {
        let start = self
            .lines
            .iter()
            .position(|line| section(line).is_some_and(|s| same(s, name)));
        let Some(start) = start else {
            if let (Some(key), Some(value)) = (key, value) {
                self.lines.push(format!("[{name}]"));
                self.lines.push(format!("{key}={value}"));
            }
            return;
        };
        let end = self.lines[start + 1..]
            .iter()
            .position(|line| section(line).is_some())
            .map_or(self.lines.len(), |i| start + 1 + i);
        let Some(key) = key else {
            self.lines.drain(start..end);
            return;
        };
        let existing =
            (start + 1..end).find(|&i| entry(&self.lines[i]).is_some_and(|(k, _)| same(k, key)));
        match (existing, value) {
            (Some(index), Some(value)) => self.lines[index] = format!("{key}={value}"),
            (Some(index), None) => {
                self.lines.remove(index);
            }
            (None, Some(value)) => self.lines.insert(end, format!("{key}={value}")),
            (None, None) => {}
        }
    }
    pub(super) fn text(&self) -> String {
        if self.lines.is_empty() {
            String::new()
        } else {
            self.lines.join("\r\n") + "\r\n"
        }
    }
}
pub(super) fn integer(value: &str) -> u32 {
    let value = value.trim_start();
    let (negative, value) = if let Some(rest) = value.strip_prefix('-') {
        (true, rest)
    } else {
        (false, value.strip_prefix('+').unwrap_or(value))
    };
    let (radix, value) = if let Some(rest) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (16, rest)
    } else {
        (10, value)
    };
    let mut result = 0u32;
    for digit in value.chars().map_while(|c| c.to_digit(radix)) {
        result = result.wrapping_mul(radix).wrapping_add(digit);
    }
    if negative {
        result.wrapping_neg()
    } else {
        result
    }
}
