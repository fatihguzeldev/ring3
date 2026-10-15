use super::{
    GuestComError32, GuestComObjects32, InterfaceLayout32, ObjectAllocation32, Vtable32,
    allocation::validate_layouts,
};
use crate::{
    memory::{
        AddressSpace, GuestAddress, MAX_WORD_WRITES32, MemoryError, PAGE_SIZE, PageInitialization,
        PageRange, Permissions, WordWrite32,
    },
    windows::com::{
        ComError32, ComIdentity32, IID_IUNKNOWN, InterfaceBinding32, InterfaceId, ReleaseOutcome32,
    },
};

struct PageBytes {
    address: GuestAddress,
    bytes: [u8; PAGE_SIZE as usize],
}

impl GuestComObjects32 {
    pub(crate) fn create(
        &mut self,
        memory: &mut AddressSpace,
        payload: u64,
        layouts: &[InterfaceLayout32<'_>],
    ) -> Result<ComIdentity32, GuestComError32> {
        self.create_publication(memory, payload, layouts, None, &[])
    }

    pub(crate) fn create_with_output(
        &mut self,
        memory: &mut AddressSpace,
        payload: u64,
        layouts: &[InterfaceLayout32<'_>],
        output: GuestAddress,
        iid: InterfaceId,
    ) -> Result<ComIdentity32, GuestComError32> {
        self.create_with_writes(memory, payload, layouts, output, iid, &[])
    }

    pub(crate) fn create_with_writes(
        &mut self,
        memory: &mut AddressSpace,
        payload: u64,
        layouts: &[InterfaceLayout32<'_>],
        output: GuestAddress,
        iid: InterfaceId,
        words: &[WordWrite32],
    ) -> Result<ComIdentity32, GuestComError32> {
        self.create_publication(memory, payload, layouts, Some((output, iid)), words)
    }

    fn create_publication(
        &mut self,
        memory: &mut AddressSpace,
        payload: u64,
        layouts: &[InterfaceLayout32<'_>],
        output: Option<(GuestAddress, InterfaceId)>,
        words: &[WordWrite32],
    ) -> Result<ComIdentity32, GuestComError32> {
        validate_layouts(layouts)?;
        if let Some((_, iid)) = output
            && iid != IID_IUNKNOWN
            && !layouts.iter().any(|layout| layout.iid == iid)
        {
            return Err(GuestComError32::Com(ComError32::NoInterface));
        }
        self.check_memory(memory)?;
        let mut missing = 0;
        for layout in layouts {
            if let Some(index) = self.cached_vtable(*layout) {
                let table = &self.vtables[index];
                self.check_mapping(
                    memory,
                    table.address,
                    table.mapping_version,
                    Permissions::READ,
                )?;
            } else {
                missing += 1;
            }
        }
        if missing > self.vtable_limit - self.vtables.len() {
            return Err(GuestComError32::Capacity);
        }
        let range = self.reserve_range(memory, missing as u32 + 1)?;
        let object_address = GuestAddress((range.first + range.count - 1) as u32 * PAGE_SIZE);
        let object_range = PageRange::new(object_address, 1)?;
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(missing + 1)
            .map_err(|_| GuestComError32::Allocation)?;
        let mut tables = Vec::new();
        tables
            .try_reserve_exact(missing)
            .map_err(|_| GuestComError32::Allocation)?;
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(layouts.len())
            .map_err(|_| GuestComError32::Allocation)?;
        let mut object_bytes = [0; PAGE_SIZE as usize];
        for (index, layout) in layouts.iter().enumerate() {
            let vtable = if let Some(cached) = self.cached_vtable(*layout) {
                self.vtables[cached].address
            } else {
                let address = GuestAddress((range.first + tables.len()) as u32 * PAGE_SIZE);
                let mut methods = Vec::new();
                methods
                    .try_reserve_exact(layout.methods.len())
                    .map_err(|_| GuestComError32::Allocation)?;
                methods.extend_from_slice(layout.methods);
                let mut bytes = [0; PAGE_SIZE as usize];
                for (offset, method) in layout.methods.iter().enumerate() {
                    bytes[offset * 4..offset * 4 + 4].copy_from_slice(&method.0.to_le_bytes());
                }
                pages.push(PageBytes { address, bytes });
                tables.push(Vtable32 {
                    iid: layout.iid,
                    methods: methods.into_boxed_slice(),
                    address,
                    mapping_version: 0,
                });
                address
            };
            object_bytes[index * 4..index * 4 + 4].copy_from_slice(&vtable.0.to_le_bytes());
            bindings.push(InterfaceBinding32 {
                iid: layout.iid,
                address: GuestAddress(object_address.0 + index as u32 * 4),
                vtable,
            });
        }
        pages.push(PageBytes {
            address: object_address,
            bytes: object_bytes,
        });
        let mut initialized = Vec::new();
        initialized
            .try_reserve_exact(pages.len())
            .map_err(|_| GuestComError32::Allocation)?;
        initialized.extend(pages.iter().map(|page| PageInitialization {
            address: page.address,
            permissions: if page.address == object_address {
                Permissions::READ_WRITE
            } else {
                Permissions::READ
            },
            bytes: &page.bytes,
        }));
        if words.len() + usize::from(output.is_some()) > MAX_WORD_WRITES32 {
            return Err(GuestComError32::Memory(MemoryError::InvalidRange));
        }
        let mut publication_words = Vec::new();
        publication_words
            .try_reserve_exact(words.len() + usize::from(output.is_some()))
            .map_err(|_| GuestComError32::Allocation)?;
        publication_words.extend_from_slice(words);
        if let Some((address, iid)) = output {
            let target = if iid == IID_IUNKNOWN {
                bindings
                    .iter()
                    .find(|binding| binding.iid == IID_IUNKNOWN)
                    .unwrap_or(&bindings[0])
            } else {
                bindings.iter().find(|binding| binding.iid == iid).unwrap()
            };
            publication_words.push(WordWrite32 {
                address,
                value: target.address.0,
            });
        }
        publication_words.sort_unstable_by_key(|word| word.address.0);
        let prepared = self.registry.prepare_registration(payload, &bindings)?;
        memory.install_pages_with_word_writes(&initialized, &publication_words)?;
        let mapping_version = memory.transaction_version();
        let identity = prepared.publish();
        for mut table in tables {
            table.mapping_version = mapping_version;
            self.vtables.push(table);
        }
        self.allocations.push(ObjectAllocation32 {
            id: identity.id,
            range: object_range,
            mapping_version,
        });
        self.next_page = range.first + range.count;
        self.memory_identity = Some(memory.identity());
        Ok(identity)
    }

    pub(crate) fn lookup(&self, pointer: GuestAddress) -> Result<ComIdentity32, GuestComError32> {
        Ok(self.registry.lookup(pointer)?)
    }

    pub(crate) fn canonical_pointer(
        &self,
        pointer: GuestAddress,
    ) -> Result<GuestAddress, GuestComError32> {
        Ok(self.lookup(pointer)?.identity_pointer)
    }

    pub(crate) fn references(&self, pointer: GuestAddress) -> Result<u32, GuestComError32> {
        Ok(self.registry.references(pointer)?)
    }

    pub(crate) fn preflight(
        &self,
        memory: &AddressSpace,
        pointer: GuestAddress,
    ) -> Result<ComIdentity32, GuestComError32> {
        let identity = self.lookup(pointer)?;
        let index = self.allocation(identity)?;
        let allocation = &self.allocations[index];
        self.check_mapping(
            memory,
            GuestAddress(allocation.range.first as u32 * PAGE_SIZE),
            allocation.mapping_version,
            Permissions::READ_WRITE,
        )?;
        Ok(identity)
    }

    pub(crate) fn query_target(
        &self,
        memory: &AddressSpace,
        pointer: GuestAddress,
        iid: InterfaceId,
    ) -> Result<GuestAddress, GuestComError32> {
        self.preflight(memory, pointer)?;
        Ok(self.registry.query_target(pointer, iid)?)
    }

    pub(crate) fn check_add_ref(
        &self,
        memory: &AddressSpace,
        pointer: GuestAddress,
    ) -> Result<(), GuestComError32> {
        self.preflight(memory, pointer)?;
        self.references(pointer)?
            .checked_add(1)
            .ok_or(GuestComError32::Com(ComError32::Exhausted))?;
        Ok(())
    }

    pub(crate) fn query_interface(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        iid: InterfaceId,
        output: GuestAddress,
    ) -> Result<Option<GuestAddress>, GuestComError32> {
        self.preflight(memory, pointer)?;
        let target = match self.registry.query_target(pointer, iid) {
            Ok(target) => Some(target),
            Err(ComError32::NoInterface) => None,
            Err(error) => return Err(error.into()),
        };
        if target.is_some() {
            self.check_add_ref(memory, pointer)?;
        }
        memory.write(output, &target.map_or(0, |address| address.0).to_le_bytes())?;
        if target.is_some() {
            self.registry
                .add_ref(pointer)
                .expect("com reference increment was preflighted against unchanged identity");
        }
        Ok(target)
    }

    pub(crate) fn add_ref(
        &mut self,
        memory: &AddressSpace,
        pointer: GuestAddress,
    ) -> Result<u32, GuestComError32> {
        self.check_add_ref(memory, pointer)?;
        Ok(self.registry.add_ref(pointer)?)
    }

    pub(crate) fn preflight_release(
        &self,
        memory: &AddressSpace,
        pointer: GuestAddress,
    ) -> Result<ComIdentity32, GuestComError32> {
        let identity = self.preflight(memory, pointer)?;
        if self.references(pointer)? == 1
            && self.registry.holds(pointer)? == 0
            && memory.transaction_version() == u64::MAX
        {
            return Err(GuestComError32::Memory(MemoryError::VersionExhausted));
        }
        Ok(identity)
    }

    pub(crate) fn release(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
    ) -> Result<ReleaseOutcome32, GuestComError32> {
        let identity = self.preflight_release(memory, pointer)?;
        if self.references(pointer)? > 1 || self.registry.holds(pointer)? > 0 {
            return Ok(self.registry.release(pointer)?);
        }
        let index = self.allocation(identity)?;
        memory.unmap(self.allocations[index].range)?;
        let outcome = self
            .registry
            .release(pointer)
            .expect("com release was preflighted against unchanged identity");
        self.allocations.remove(index);
        Ok(outcome)
    }

    fn allocation(&self, identity: ComIdentity32) -> Result<usize, GuestComError32> {
        self.allocations
            .binary_search_by_key(&identity.id, |allocation| allocation.id)
            .map_err(|_| GuestComError32::StaleMapping)
    }
}
