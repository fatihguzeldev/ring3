use super::{AutomationError32 as E, AutomationLimits32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrayBound32 {
    pub count: u32,
    pub lower: i32,
}
#[derive(Clone, Debug)]
pub(crate) struct ArrayLayout32 {
    pub vartype: u16,
    pub element_bytes: usize,
    pub features: u16,
    pub bounds: Vec<ArrayBound32>,
    pub data_bytes: usize,
}
impl ArrayLayout32 {
    pub(crate) fn new(
        vartype: u16,
        bounds: &[ArrayBound32],
        vector: bool,
        limits: AutomationLimits32,
    ) -> Result<Self, E> {
        if bounds.is_empty() || bounds.len() > limits.dimensions {
            return Err(E::InvalidParameter);
        }
        let element_bytes = match vartype {
            16 | 17 => 1,
            2 | 11 | 18 => 2,
            3 | 4 | 8 | 9 | 10 | 13 | 19 | 22 | 23 => 4,
            5 | 6 | 7 | 20 | 21 => 8,
            12 | 14 => 16,
            _ => return Err(E::BadVariantType),
        };
        let elements = bounds.iter().try_fold(1usize, |n, b| {
            n.checked_mul(b.count as usize).ok_or(E::Capacity)
        })?;
        let data_bytes = elements
            .checked_mul(element_bytes)
            .filter(|&n| n <= limits.bytes)
            .ok_or(E::Capacity)?;
        let features = 0x80
            | if vector { 0x2000 } else { 0 }
            | match vartype {
                8 => 0x100,
                9 => 0x400,
                13 => 0x200,
                12 => 0x800,
                _ => 0,
            };
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bounds.len())
            .map_err(|_| E::Allocation)?;
        owned.extend_from_slice(bounds);
        Ok(Self {
            vartype,
            element_bytes,
            features,
            bounds: owned,
            data_bytes,
        })
    }
    pub(crate) fn descriptor_bytes(&self) -> usize {
        16 + self.bounds.len() * 8
    }
    pub(crate) fn allocation_bytes(&self) -> Result<usize, E> {
        self.data_offset()
            .checked_add(self.data_bytes)
            .ok_or(E::Capacity)
    }
    pub(crate) fn data_offset(&self) -> usize {
        (4 + self.descriptor_bytes() + 7) & !7
    }
    pub(crate) fn write_descriptor(
        &self,
        bytes: &mut [u8],
        data: u32,
        locks: u32,
    ) -> Result<(), E> {
        let bytes = bytes
            .get_mut(..self.descriptor_bytes())
            .ok_or(E::InvalidParameter)?;
        bytes.fill(0);
        bytes[..2].copy_from_slice(&(self.bounds.len() as u16).to_le_bytes());
        bytes[2..4].copy_from_slice(&self.features.to_le_bytes());
        bytes[4..8].copy_from_slice(&(self.element_bytes as u32).to_le_bytes());
        bytes[8..12].copy_from_slice(&locks.to_le_bytes());
        bytes[12..16].copy_from_slice(&data.to_le_bytes());
        for (index, bound) in self.bounds.iter().rev().enumerate() {
            bytes[16 + index * 8..20 + index * 8].copy_from_slice(&bound.count.to_le_bytes());
            bytes[20 + index * 8..24 + index * 8].copy_from_slice(&bound.lower.to_le_bytes());
        }
        Ok(())
    }
    pub(crate) fn bound(&self, dimension: u32) -> Result<ArrayBound32, E> {
        self.bounds
            .get(dimension.checked_sub(1).ok_or(E::BadIndex)? as usize)
            .copied()
            .ok_or(E::BadIndex)
    }
    pub(crate) fn offset(&self, indices: &[i32]) -> Result<usize, E> {
        if indices.len() != self.bounds.len() {
            return Err(E::BadIndex);
        }
        let mut stride = 1usize;
        let mut index = 0usize;
        for (value, bound) in indices.iter().zip(&self.bounds) {
            let relative = i64::from(*value) - i64::from(bound.lower);
            if relative < 0 || relative >= i64::from(bound.count) {
                return Err(E::BadIndex);
            }
            index = index
                .checked_add((relative as usize).checked_mul(stride).ok_or(E::Overflow)?)
                .ok_or(E::Overflow)?;
            stride = stride
                .checked_mul(bound.count as usize)
                .ok_or(E::Overflow)?;
        }
        index.checked_mul(self.element_bytes).ok_or(E::Overflow)
    }
    pub(crate) fn owns_elements(&self) -> bool {
        matches!(self.vartype, 8 | 9 | 12 | 13)
    }
}
