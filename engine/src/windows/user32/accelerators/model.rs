use super::Accelerator32;
use crate::windows::user32::User32Error32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AcceleratorHandle32(pub(super) u32);

impl AcceleratorHandle32 {
    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for AcceleratorHandle32 {
    type Error = E;
    fn try_from(value: u32) -> Result<Self, E> {
        (value != 0).then_some(Self(value)).ok_or(E::Win32(1403))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResourceKey32 {
    pub module: u64,
    pub entry: u32,
}

struct Table32 {
    handle: AcceleratorHandle32,
    entries: Vec<Accelerator32>,
    origin: Option<ResourceKey32>,
    references: u32,
}

pub(crate) struct AcceleratorStore32 {
    tables: Vec<Table32>,
    table_limit: usize,
    entry_limit: usize,
    entry_count: usize,
    next_handle: u32,
}

impl AcceleratorStore32 {
    pub(crate) fn new(table_limit: usize, entry_limit: usize) -> Result<Self, E> {
        if !(1..=256).contains(&table_limit) || !(1..=4096).contains(&entry_limit) {
            return Err(E::InvalidLimits);
        }
        let mut tables = Vec::new();
        tables
            .try_reserve_exact(table_limit)
            .map_err(|_| E::Allocation)?;
        Ok(Self {
            tables,
            table_limit,
            entry_limit,
            entry_count: 0,
            next_handle: 0x4100_0000,
        })
    }

    pub(crate) fn entries(&self, handle: AcceleratorHandle32) -> Result<&[Accelerator32], E> {
        Ok(&self.tables[self.index(handle)?].entries)
    }

    pub(crate) fn retain_resource(
        &mut self,
        key: ResourceKey32,
    ) -> Result<Option<AcceleratorHandle32>, E> {
        let Some(table) = self
            .tables
            .iter_mut()
            .find(|table| table.origin == Some(key))
        else {
            return Ok(None);
        };
        table.references = table.references.checked_add(1).ok_or(E::Capacity)?;
        Ok(Some(table.handle))
    }

    pub(crate) fn create(
        &mut self,
        entries: Vec<Accelerator32>,
        origin: Option<ResourceKey32>,
    ) -> Result<AcceleratorHandle32, E> {
        if entries.is_empty() || entries.iter().any(|entry| entry.flags & !0x7f != 0) {
            return Err(E::Win32(87));
        }
        if let Some(key) = origin
            && let Some(handle) = self.retain_resource(key)?
        {
            return Ok(handle);
        }
        if self.tables.len() == self.table_limit
            || entries.len() > self.entry_limit - self.entry_count
        {
            return Err(E::Capacity);
        }
        let handle = self
            .next_handle
            .checked_add(1)
            .filter(|&value| value < 0x4200_0000)
            .map(AcceleratorHandle32)
            .ok_or(E::Capacity)?;
        self.entry_count += entries.len();
        self.tables.push(Table32 {
            handle,
            entries,
            origin,
            references: 1,
        });
        self.next_handle = handle.0;
        Ok(handle)
    }

    pub(crate) fn destroy(&mut self, handle: AcceleratorHandle32) -> Result<bool, E> {
        let index = self.index(handle)?;
        if self.tables[index].references > 1 {
            self.tables[index].references -= 1;
            return Ok(false);
        }
        self.entry_count -= self.tables[index].entries.len();
        self.tables.remove(index);
        Ok(true)
    }

    fn index(&self, handle: AcceleratorHandle32) -> Result<usize, E> {
        self.tables
            .iter()
            .position(|table| table.handle == handle)
            .ok_or(E::Win32(1403))
    }
}
