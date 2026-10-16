use super::VersionError32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VersionValue32 {
    pub offset: usize,
    pub length: usize,
    pub text: bool,
}
struct Node32<'a> {
    bytes: &'a [u8],
    end: usize,
    key: Vec<u16>,
    value: VersionValue32,
    children: usize,
}
pub(crate) struct VersionBlock32<'a> {
    bytes: &'a [u8],
}
fn word(bytes: &[u8], at: usize) -> Result<u16, E> {
    bytes
        .get(at..at.checked_add(2).ok_or(E::InvalidData)?)
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .ok_or(E::InvalidData)
}
fn align(at: usize) -> Result<usize, E> {
    at.checked_add(3).map(|n| n & !3).ok_or(E::InvalidData)
}
fn key_eq(a: &[u16], b: &[u16]) -> bool {
    crate::windows::text::ordinal_key(a) == crate::windows::text::ordinal_key(b)
}
impl<'a> Node32<'a> {
    fn parse(bytes: &'a [u8], at: usize, parent_end: usize) -> Result<Self, E> {
        let length = word(bytes, at)? as usize;
        let end = at
            .checked_add(length)
            .filter(|&n| n <= parent_end && n <= bytes.len())
            .ok_or(E::InvalidData)?;
        if length < 8 {
            return Err(E::InvalidData);
        }
        let value_length = word(bytes, at + 2)? as usize;
        let kind = word(bytes, at + 4)?;
        if kind > 1 {
            return Err(E::InvalidData);
        }
        let mut cursor = at + 6;
        let mut key = Vec::new();
        key.try_reserve(32).map_err(|_| E::Allocation)?;
        loop {
            if cursor + 2 > end {
                return Err(E::InvalidData);
            }
            let unit = word(bytes, cursor)?;
            cursor += 2;
            if unit == 0 {
                break;
            }
            if key.len() == 1024 {
                return Err(E::InvalidData);
            }
            key.try_reserve(1).map_err(|_| E::Allocation)?;
            key.push(unit);
        }
        let value_at = align(cursor)?;
        let value_bytes = value_length
            .checked_mul(if kind == 1 { 2 } else { 1 })
            .ok_or(E::InvalidData)?;
        if value_at.checked_add(value_bytes).is_none_or(|n| n > end) {
            return Err(E::InvalidData);
        }
        Ok(Self {
            bytes,
            end,
            key,
            value: VersionValue32 {
                offset: value_at,
                length: value_length,
                text: kind == 1,
            },
            children: align(value_at + value_bytes)?,
        })
    }
    fn find(&self, name: &[u16], budget: &mut usize) -> Result<Self, E> {
        let mut at = self.children;
        while at < self.end {
            if self.end - at < 8 && self.bytes[at..self.end].iter().all(|&v| v == 0) {
                break;
            }
            *budget = budget.checked_sub(1).ok_or(E::InvalidData)?;
            let node = Self::parse(self.bytes, at, self.end)?;
            if key_eq(&node.key, name) {
                return Ok(node);
            }
            at = align(node.end)?;
        }
        Err(E::NotFound)
    }
}
impl<'a> VersionBlock32<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, E> {
        let root = Node32::parse(bytes, 0, bytes.len())?;
        if root.key != "VS_VERSION_INFO".encode_utf16().collect::<Vec<_>>()
            || root.value.text
            || root.value.length < 52
        {
            return Err(E::InvalidData);
        }
        let signature = bytes
            .get(root.value.offset..root.value.offset + 4)
            .ok_or(E::InvalidData)?;
        if signature != 0xfeef_04bdu32.to_le_bytes() {
            return Err(E::InvalidData);
        }
        Ok(Self {
            bytes: &bytes[..root.end],
        })
    }
    pub fn encoded(&self, cp: crate::windows::text::CodePage32) -> Result<Vec<u8>, E> {
        let root = Node32::parse(self.bytes, 0, self.bytes.len())?;
        let mut pending = vec![(root.children, root.end, 0usize)];
        let mut values = Vec::new();
        let mut budget = 4096usize;
        while let Some((mut at, end, depth)) = pending.pop() {
            if depth >= 16 {
                return Err(E::InvalidData);
            }
            while at < end {
                if end - at < 8 && self.bytes[at..end].iter().all(|&b| b == 0) {
                    break;
                }
                budget = budget.checked_sub(1).ok_or(E::InvalidData)?;
                let node = Node32::parse(self.bytes, at, end)?;
                if node.value.text && node.value.length > 0 {
                    let bytes =
                        &self.bytes[node.value.offset..node.value.offset + node.value.length * 2];
                    let units = bytes
                        .chunks_exact(2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect::<Vec<_>>();
                    if units.last() != Some(&0) {
                        return Err(E::InvalidData);
                    }
                    let encoded = cp
                        .encode_windows(&units, 0, b'?')
                        .map_err(|_| E::Allocation)?
                        .0;
                    values.try_reserve(1).map_err(|_| E::Allocation)?;
                    values.push((node.value.offset, encoded));
                }
                if node.children < node.end {
                    pending.try_reserve(1).map_err(|_| E::Allocation)?;
                    pending.push((node.children, node.end, depth + 1));
                }
                at = align(node.end)?;
            }
        }
        let start = align(self.bytes.len())?;
        let table = values
            .len()
            .checked_mul(12)
            .and_then(|n| n.checked_add(16))
            .ok_or(E::Allocation)?;
        let size = values
            .iter()
            .try_fold(start + table, |n, (_, v)| n.checked_add(v.len()))
            .ok_or(E::Allocation)?;
        let mut out = Vec::new();
        out.try_reserve_exact(size).map_err(|_| E::Allocation)?;
        out.extend_from_slice(self.bytes);
        out.resize(start + table, 0);
        out[start..start + 4].copy_from_slice(b"R3VA");
        out[start + 4..start + 8].copy_from_slice(&(size as u32).to_le_bytes());
        out[start + 8..start + 12].copy_from_slice(&(values.len() as u32).to_le_bytes());
        out[start + 12..start + 16].copy_from_slice(&cp.id().to_le_bytes());
        for (index, (offset, bytes)) in values.into_iter().enumerate() {
            let entry = start + 16 + index * 12;
            let value_offset = out.len() as u32;
            out[entry..entry + 4].copy_from_slice(&(offset as u32).to_le_bytes());
            out[entry + 4..entry + 8].copy_from_slice(&value_offset.to_le_bytes());
            out[entry + 8..entry + 12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&bytes);
        }
        Ok(out)
    }
    pub fn query(&self, path: &[u16]) -> Result<VersionValue32, E> {
        let mut node = Node32::parse(self.bytes, 0, self.bytes.len())?;
        let mut budget = 4096;
        let components = path.split(|&c| c == b'\\' as u16).filter(|c| !c.is_empty());
        for (depth, component) in components.enumerate() {
            if depth >= 16 {
                return Err(E::InvalidData);
            }
            node = node.find(component, &mut budget)?;
        }
        if node.value.text
            && node.value.length != 0
            && word(self.bytes, node.value.offset + (node.value.length - 1) * 2)? != 0
        {
            return Err(E::InvalidData);
        }
        Ok(node.value)
    }
}
