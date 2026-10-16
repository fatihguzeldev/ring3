use super::*;
use crate::{
    loader::{LoadPlan32, ModuleError32, ModuleId32},
    memory::{GuestAddress, PAGE_SIZE, Permissions, WordWrite32},
    process::{EngineInstance, HostError, loading::modules::TlsAllocation},
};

#[derive(Clone, Copy)]
pub(super) struct ThreadStaticTls32 {
    pub module: ModuleId32,
    pub slot: u32,
    pub range: PageRange,
    pub external: bool,
    pub mapping_version: u64,
}
pub(in crate::process) struct PreparedSecondaryTls32 {
    rows: Vec<(u32, Vec<ThreadStaticTls32>)>,
    words: Vec<WordWrite32>,
    ranges: Vec<PageRange>,
}
impl PreparedSecondaryTls32 {
    pub(in crate::process) fn words(&self) -> &[WordWrite32] {
        &self.words
    }
    pub(in crate::process) fn ranges(&self) -> &[PageRange] {
        &self.ranges
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_secondary_static_tls(
        &self,
        plan: &mut LoadPlan32,
        allocations: &[TlsAllocation],
        bounds: PageRange,
    ) -> Result<PreparedSecondaryTls32, HostError> {
        let children = self
            .threads
            .threads
            .iter()
            .filter(|thread| thread.id != 1 && thread.state != ThreadState32::Terminated)
            .count();
        let mut receipt = PreparedSecondaryTls32 {
            rows: Vec::new(),
            words: Vec::new(),
            ranges: Vec::new(),
        };
        receipt
            .rows
            .try_reserve_exact(children)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        receipt
            .words
            .try_reserve_exact(
                children
                    .checked_mul(allocations.len())
                    .ok_or(HostError::Module(ModuleError32::Capacity))?,
            )
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        for thread in self
            .threads
            .threads
            .iter()
            .filter(|thread| thread.id != 1 && thread.state != ThreadState32::Terminated)
        {
            let array = thread
                .tls_array
                .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
            let mut pointer = [0; 4];
            self.memory()?
                .read(GuestAddress(thread.teb.unwrap() + 0x2c), &mut pointer)
                .map_err(HostError::Memory)?;
            if u32::from_le_bytes(pointer) != array.first as u32 * PAGE_SIZE {
                return Err(HostError::Module(ModuleError32::InvalidRequest));
            }
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(thread.static_tls.len() + allocations.len())
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            entries.extend_from_slice(&thread.static_tls);
            for allocation in allocations {
                if allocation.slot as u64 * 4 + 4 > array.count as u64 * u64::from(PAGE_SIZE)
                    || entries.iter().any(|entry| entry.slot == allocation.slot)
                {
                    return Err(HostError::Module(ModuleError32::Capacity));
                }
                let module = plan
                    .additions()
                    .iter()
                    .find(|module| module.id() == allocation.module)
                    .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
                let tls = module
                    .tls()
                    .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
                let template = tls.template.clone();
                let size = (template.len() as u64)
                    .checked_add(u64::from(tls.zero_fill))
                    .ok_or(HostError::Module(ModuleError32::Capacity))?;
                let pages = u32::try_from(size.max(1).div_ceil(u64::from(PAGE_SIZE)))
                    .map_err(|_| HostError::Module(ModuleError32::Capacity))?;
                let alignment = PAGE_SIZE.max(tls.alignment);
                let mut lower = bounds.first;
                let range = loop {
                    if lower >= bounds.first + bounds.count {
                        return Err(HostError::Module(ModuleError32::Capacity));
                    }
                    let candidate = self
                        .memory()?
                        .find_free_range(
                            PageRange {
                                first: lower,
                                count: bounds.first + bounds.count - lower,
                            },
                            pages,
                            alignment,
                        )
                        .map_err(HostError::Memory)?;
                    if plan.intersects_staged_range(candidate) {
                        lower = candidate.first + 1;
                        continue;
                    }
                    break candidate;
                };
                let block = range.first as u32 * PAGE_SIZE;
                for page in 0..pages {
                    let start = page as usize * PAGE_SIZE as usize;
                    let bytes = template
                        .get(start..template.len().min(start + PAGE_SIZE as usize))
                        .unwrap_or(&[]);
                    plan.add_page(
                        GuestAddress(block + page * PAGE_SIZE),
                        Permissions::READ_WRITE,
                        bytes,
                    )
                    .map_err(HostError::Module)?;
                }
                receipt.words.push(WordWrite32 {
                    address: GuestAddress(array.first as u32 * PAGE_SIZE + allocation.slot * 4),
                    value: block,
                });
                entries.push(ThreadStaticTls32 {
                    module: allocation.module,
                    slot: allocation.slot,
                    range,
                    external: true,
                    mapping_version: 0,
                });
            }
            receipt.rows.push((thread.id, entries));
        }
        Ok(receipt)
    }
    pub(in crate::process) fn prepare_secondary_tls_unload(
        &self,
        ids: &[ModuleId32],
    ) -> Result<PreparedSecondaryTls32, HostError> {
        let mut receipt = PreparedSecondaryTls32 {
            rows: Vec::new(),
            words: Vec::new(),
            ranges: Vec::new(),
        };
        receipt
            .rows
            .try_reserve_exact(self.threads.threads.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        for thread in self
            .threads
            .threads
            .iter()
            .filter(|thread| thread.id != 1 && thread.state != ThreadState32::Terminated)
        {
            let Some(array) = thread.tls_array else {
                continue;
            };
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(thread.static_tls.len())
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            for &entry in &thread.static_tls {
                if ids.contains(&entry.module) {
                    receipt
                        .words
                        .try_reserve(1)
                        .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
                    receipt.words.push(WordWrite32 {
                        address: GuestAddress(array.first as u32 * PAGE_SIZE + entry.slot * 4),
                        value: 0,
                    });
                    if entry.external {
                        self.check_thread_tls_mapping(entry)?;
                        receipt
                            .ranges
                            .try_reserve(1)
                            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
                        receipt.ranges.push(entry.range);
                    }
                } else {
                    entries.push(entry);
                }
            }
            receipt.rows.push((thread.id, entries));
        }
        Ok(receipt)
    }
    pub(in crate::process) fn commit_secondary_static_tls(
        &mut self,
        receipt: PreparedSecondaryTls32,
    ) {
        for (id, mut entries) in receipt.rows {
            for entry in &mut entries {
                if entry.mapping_version == 0 {
                    entry.mapping_version = self
                        .memory
                        .as_ref()
                        .unwrap()
                        .mapping_version_at(GuestAddress(entry.range.first as u32 * PAGE_SIZE))
                        .expect("loader installed prepared child TLS mapping");
                }
            }
            let thread = self
                .threads
                .threads
                .iter_mut()
                .find(|thread| thread.id == id)
                .expect("thread TLS ownership unchanged during loader transaction");
            thread.static_tls = entries;
        }
    }
    pub(super) fn check_thread_tls_mapping(
        &self,
        entry: ThreadStaticTls32,
    ) -> Result<(), HostError> {
        for page in entry.range.indices() {
            if self
                .memory()?
                .mapping_version_at(GuestAddress(page as u32 * PAGE_SIZE))
                != Ok(entry.mapping_version)
            {
                return Err(HostError::Thread(ThreadError32::Stale));
            }
        }
        Ok(())
    }
}
