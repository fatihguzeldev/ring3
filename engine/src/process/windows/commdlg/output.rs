use crate::{
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE, PageRange},
    windows::commdlg::{ComdlgError32 as E, OpenFileName32},
};

pub(super) struct OutputPages32 {
    identity: u64,
    pages: Vec<(GuestAddress, u64)>,
}

impl OutputPages32 {
    pub(super) fn capture(
        memory: &mut AddressSpace,
        structure: u32,
        input: OpenFileName32,
        wide: bool,
    ) -> Result<Self, E> {
        let width = if wide { 2 } else { 1 };
        let spans = [
            (structure, input.size as usize),
            (input.file, length(input.file_capacity, width)?),
            (input.file_title, length(input.title_capacity, width)?),
            (input.custom_filter, length(input.custom_capacity, width)?),
        ];
        let mut pages = Vec::new();
        for (address, length) in spans {
            if address == 0 || length == 0 {
                continue;
            }
            memory.check_span(GuestAddress(address), length, Access::Write)?;
            let last = address
                .checked_add(u32::try_from(length - 1).map_err(|_| E::Capacity)?)
                .ok_or(E::InvalidRequest)?;
            let first = address / PAGE_SIZE;
            let count = last / PAGE_SIZE - first + 1;
            memory.observe_mapping_incarnations(PageRange::new(
                GuestAddress(first * PAGE_SIZE),
                count,
            )?)?;
            for page in first..=last / PAGE_SIZE {
                let address = GuestAddress(page * PAGE_SIZE);
                if pages.iter().any(|(old, _)| *old == address) {
                    continue;
                }
                pages.try_reserve(1).map_err(|_| E::Allocation)?;
                pages.push((address, memory.mapping_incarnation_at(address)?));
            }
        }
        Ok(Self {
            identity: memory.identity(),
            pages,
        })
    }

    pub(super) fn check(&self, memory: &AddressSpace) -> Result<(), E> {
        if memory.identity() != self.identity {
            return Err(E::StateChanged);
        }
        for &(address, incarnation) in &self.pages {
            match memory.mapping_incarnation_at(address) {
                Ok(current) if current == incarnation => {}
                Ok(_) | Err(crate::memory::MemoryError::NotMapped { .. }) => {
                    return Err(E::StateChanged);
                }
                Err(error) => return Err(E::Memory(error)),
            }
        }
        Ok(())
    }
}

fn length(units: u32, width: usize) -> Result<usize, E> {
    (units as usize).checked_mul(width).ok_or(E::Capacity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Permissions;

    #[test]
    fn repaired_rights_preserve_output_ownership_but_same_address_replacement_does_not() {
        let mut memory = AddressSpace::new(4).unwrap();
        let range = PageRange::new(GuestAddress(0x10000), 2).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        let mut raw = [0; 88];
        raw[..4].copy_from_slice(&88u32.to_le_bytes());
        raw[28..32].copy_from_slice(&0x11000u32.to_le_bytes());
        raw[32..36].copy_from_slice(&128u32.to_le_bytes());
        let input = OpenFileName32::parse(&raw).unwrap();
        let pages = OutputPages32::capture(&mut memory, 0x10000, input, true).unwrap();
        memory.protect(range, Permissions::READ).unwrap();
        assert_eq!(pages.check(&memory), Ok(()));
        memory.protect(range, Permissions::READ_WRITE).unwrap();
        assert_eq!(pages.check(&memory), Ok(()));
        memory.unmap(range).unwrap();
        memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
        assert_eq!(pages.check(&memory), Err(E::StateChanged));
    }
}
