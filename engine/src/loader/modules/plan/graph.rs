pub(super) mod placement;
mod system;

fn default_resolution(_: &str, name: &str) -> Result<String, crate::loader::ModuleError32> {
    crate::loader::normalize_module_name(name)
}

use std::sync::Arc;

use super::{LoadPlan32, pages};
use crate::{
    loader::{
        LoadedModule32, ModuleCatalog32, ModuleError32, ModuleId32, ModuleSet32, ResolvedExport32,
        modules::{
            catalog::ModuleSource32,
            linking::{self, ExportValue32},
            normalize_module_identity, normalize_module_name,
        },
        pe32::module::ImportSymbol32,
    },
    memory::{AddressSpace, GuestAddress, PageRange},
};

struct Builder<'a> {
    catalog: &'a ModuleCatalog32,
    set: &'a ModuleSet32,
    memory: &'a AddressSpace,
    reserved: &'a [PageRange],
    plan: LoadPlan32,
    resolver: &'a dyn Fn(&str, &str) -> Result<String, ModuleError32>,
}

impl ModuleCatalog32 {
    pub fn plan_load_headers(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        request: &str,
        actual_base: Option<u32>,
        reserved: &[PageRange],
        runtime: &[&str],
    ) -> Result<LoadPlan32, ModuleError32> {
        if runtime.len() > self.limits.modules {
            return Err(ModuleError32::Capacity);
        }
        let mut builder = Builder::new(self, set, memory, reserved, &default_resolution);
        builder.plan.root = builder.ensure(&normalize_module_identity(request)?, actual_base)?;
        for dependency in runtime {
            let id = builder.ensure(&normalize_module_name(dependency)?, None)?;
            builder.dependency(builder.plan.root, id)?;
        }
        builder.finish()
    }

    pub fn plan_load(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        request: &str,
        actual_base: Option<u32>,
    ) -> Result<LoadPlan32, ModuleError32> {
        self.plan_load_with_reservations(set, memory, request, actual_base, &[])
    }

    pub fn plan_load_with_reservations(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        request: &str,
        actual_base: Option<u32>,
        reserved: &[PageRange],
    ) -> Result<LoadPlan32, ModuleError32> {
        self.plan_load_with_runtime_dependencies(set, memory, request, actual_base, reserved, &[])
    }
    pub fn plan_load_with_runtime_dependencies(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        request: &str,
        actual_base: Option<u32>,
        reserved: &[PageRange],
        runtime: &[&str],
    ) -> Result<LoadPlan32, ModuleError32> {
        self.plan_load_with_resolver(
            set,
            memory,
            request,
            actual_base,
            reserved,
            runtime,
            &default_resolution,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan_load_with_resolver(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        request: &str,
        actual_base: Option<u32>,
        reserved: &[PageRange],
        runtime: &[&str],
        resolver: &dyn Fn(&str, &str) -> Result<String, ModuleError32>,
    ) -> Result<LoadPlan32, ModuleError32> {
        if runtime.len() > self.limits.modules {
            return Err(ModuleError32::Capacity);
        }
        let name = normalize_module_identity(request)?;
        let mut builder = Builder::new(self, set, memory, reserved, resolver);
        builder.plan.root = builder.ensure(&name, actual_base)?;
        for dependency in runtime {
            let dependency = normalize_module_name(dependency)?;
            let id = builder.ensure(&dependency, None)?;
            builder.dependency(builder.plan.root, id)?;
        }
        builder.bind_imports()?;
        builder.finish()
    }

    pub fn plan_export_live(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        module: ModuleId32,
        symbol: &ImportSymbol32,
    ) -> Result<(LoadPlan32, ResolvedExport32), ModuleError32> {
        self.plan_export_live_with_reservations(set, memory, module, symbol, &[])
    }
    pub fn plan_export_live_with_reservations(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        module: ModuleId32,
        symbol: &ImportSymbol32,
        reserved: &[PageRange],
    ) -> Result<(LoadPlan32, ResolvedExport32), ModuleError32> {
        self.plan_export_live_with_resolver(
            set,
            memory,
            module,
            symbol,
            reserved,
            &default_resolution,
        )
    }

    pub fn plan_export_live_with_resolver(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        module: ModuleId32,
        symbol: &ImportSymbol32,
        reserved: &[PageRange],
        resolver: &dyn Fn(&str, &str) -> Result<String, ModuleError32>,
    ) -> Result<(LoadPlan32, ResolvedExport32), ModuleError32> {
        set.get(module).ok_or(ModuleError32::NotFound)?;
        let mut builder = Builder::new(self, set, memory, reserved, resolver);
        builder.plan.root = module;
        let result = builder.resolve(module, symbol, module)?;
        builder.bind_imports()?;
        Ok((builder.finish()?, result))
    }
}

impl<'a> Builder<'a> {
    fn new(
        catalog: &'a ModuleCatalog32,
        set: &'a ModuleSet32,
        memory: &'a AddressSpace,
        reserved: &'a [PageRange],
        resolver: &'a dyn Fn(&str, &str) -> Result<String, ModuleError32>,
    ) -> Self {
        Self {
            catalog,
            set,
            memory,
            reserved,
            resolver,
            plan: LoadPlan32 {
                root: ModuleId32::from_sequence(0),
                memory_identity: memory.identity(),
                memory_version: memory.transaction_version(),
                set_revision: set.revision,
                set_identity: Arc::clone(&set.identity),
                next_id: set.next_id,
                additions: Vec::new(),
                pages: Vec::new(),
                initialization_order: Vec::new(),
                dependency_updates: Vec::new(),
                forwarder_limit: catalog.limits.forwarder_depth,
                reservations: Vec::new(),
            },
        }
    }

    fn ensure(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
    ) -> Result<ModuleId32, ModuleError32> {
        if let Some(module) = self.set.find_normalized(name).or_else(|| {
            self.plan
                .additions
                .iter()
                .find(|module| module.identity() == name)
        }) {
            if actual_base.is_some_and(|base| base != module.base) {
                return Err(ModuleError32::AddressConflict);
            }
            return Ok(module.id);
        }
        if self.set.modules.len() + self.set.datafile_count + self.plan.additions.len()
            >= self.catalog.limits.modules
        {
            return Err(ModuleError32::Capacity);
        }
        let source = self.catalog.source(name)?;
        let (preferred, image_size) = match &*source {
            ModuleSource32::Guest { image, .. } => (image.preferred_base, image.image_size),
            ModuleSource32::System(module) => (module.preferred_base, system::image_size(module)?),
        };
        let required_pages = match &*source {
            ModuleSource32::Guest { image, .. } => image.mapped_pages as usize,
            ModuleSource32::System(_) => (image_size / 4096) as usize,
        };
        if required_pages
            > ((self.memory.capacity_pages() - self.memory.mapped_pages()) as usize)
                .saturating_sub(self.plan.pages.len())
        {
            return Err(ModuleError32::Memory(crate::memory::MemoryError::Capacity));
        }
        let base = placement::place(
            self.memory,
            self.set,
            &self.plan.additions,
            self.reserved,
            preferred,
            image_size,
            actual_base,
        )?;
        let id = ModuleId32::from_sequence(
            self.plan
                .next_id
                .checked_add(1)
                .ok_or(ModuleError32::Capacity)?,
        );
        let (mut prepared, gates, tls) = match &*source {
            ModuleSource32::Guest { bytes, image, .. } => {
                let mut prepared = image.prepare_pages(bytes, base)?;
                prepared.sort_unstable_by_key(|page| page.address.0);
                let tls = image
                    .tls
                    .as_ref()
                    .map(|info| pages::tls(info, base, &prepared))
                    .transpose()?;
                (prepared, Vec::new(), tls)
            }
            ModuleSource32::System(module) => {
                let (prepared, gates) = system::prepare(module, base, image_size)?;
                (prepared, gates, None)
            }
        };
        let ranges = pages::ranges(&prepared)?;
        self.plan
            .pages
            .try_reserve(prepared.len())
            .map_err(|_| ModuleError32::Allocation)?;
        self.plan
            .additions
            .try_reserve(1)
            .map_err(|_| ModuleError32::Allocation)?;
        self.plan.pages.append(&mut prepared);
        self.plan.pages.sort_unstable_by_key(|page| page.address.0);
        self.plan.additions.push(LoadedModule32 {
            id,
            source,
            base,
            image_size,
            ranges,
            dependencies: Vec::new(),
            tls,
            gates,
        });
        self.plan.next_id = id.0;
        Ok(id)
    }

    fn get(&self, id: ModuleId32) -> Result<&LoadedModule32, ModuleError32> {
        self.plan
            .additions
            .iter()
            .find(|module| module.id == id)
            .or_else(|| self.set.get(id))
            .ok_or(ModuleError32::NotFound)
    }

    fn dependency(
        &mut self,
        importer: ModuleId32,
        target: ModuleId32,
    ) -> Result<(), ModuleError32> {
        if importer == target {
            return Ok(());
        }
        let dependencies = if let Some(module) = self
            .plan
            .additions
            .iter_mut()
            .find(|module| module.id == importer)
        {
            &mut module.dependencies
        } else {
            if !self
                .plan
                .dependency_updates
                .iter()
                .any(|(id, _)| *id == importer)
            {
                let original = self.set.get(importer).ok_or(ModuleError32::NotFound)?;
                let mut dependencies = Vec::new();
                dependencies
                    .try_reserve_exact(original.dependencies.len() + 1)
                    .map_err(|_| ModuleError32::Allocation)?;
                dependencies.extend_from_slice(&original.dependencies);
                self.plan
                    .dependency_updates
                    .try_reserve(1)
                    .map_err(|_| ModuleError32::Allocation)?;
                self.plan.dependency_updates.push((importer, dependencies));
            }
            &mut self
                .plan
                .dependency_updates
                .iter_mut()
                .find(|(id, _)| *id == importer)
                .unwrap()
                .1
        };
        if !dependencies.contains(&target) {
            dependencies
                .try_reserve(1)
                .map_err(|_| ModuleError32::Allocation)?;
            dependencies.push(target);
        }
        Ok(())
    }

    fn resolve(
        &mut self,
        mut module: ModuleId32,
        symbol: &ImportSymbol32,
        importer: ModuleId32,
    ) -> Result<ResolvedExport32, ModuleError32> {
        let mut symbol = symbol.clone();
        let mut visited = Vec::new();
        loop {
            if visited
                .iter()
                .any(|(id, previous)| *id == module && *previous == symbol)
            {
                return Err(ModuleError32::ForwarderCycle);
            }
            if visited.len() == self.catalog.limits.forwarder_depth {
                return Err(ModuleError32::Capacity);
            }
            visited
                .try_reserve(1)
                .map_err(|_| ModuleError32::Allocation)?;
            visited.push((module, symbol.clone()));
            self.dependency(importer, module)?;
            let value = if let Some(loaded) = self.set.get(module) {
                linking::export_value_live(self.memory, loaded, &symbol)?
            } else {
                linking::export_value(self.get(module)?, &symbol)?
            };
            match value {
                ExportValue32::Resolved(value) => return Ok(value),
                ExportValue32::Forwarder(value) => {
                    let (name, next) = linking::parse_forwarder(&value)?;
                    let name = (self.resolver)(self.get(module)?.identity(), &name)?;
                    module = self.ensure(&name, None)?;
                    symbol = next;
                }
            }
        }
    }

    fn bind_imports(&mut self) -> Result<(), ModuleError32> {
        let mut index = 0;
        while index < self.plan.additions.len() {
            let id = self.plan.additions[index].id;
            let base = self.plan.additions[index].base;
            let source = Arc::clone(&self.plan.additions[index].source);
            if let ModuleSource32::Guest { image, .. } = &*source {
                for import in &image.imports {
                    let name = (self.resolver)(source.name(), &import.name)?;
                    let provider = self.ensure(&name, None)?;
                    self.dependency(id, provider)?;
                    for slot in &import.slots {
                        let result = self.resolve(provider, &slot.symbol, id)?;
                        pages::stage_write(
                            &mut self.plan.pages,
                            GuestAddress(base + slot.iat_rva),
                            &result.address().0.to_le_bytes(),
                        )?;
                    }
                }
            }
            index += 1;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<LoadPlan32, ModuleError32> {
        if self.plan.pages.len()
            > (self.memory.capacity_pages() - self.memory.mapped_pages()) as usize
        {
            return Err(ModuleError32::Memory(crate::memory::MemoryError::Capacity));
        }
        self.plan.initialization_order = initialization_order(&self.plan.additions)?;
        self.plan
            .reservations
            .try_reserve_exact(self.set.modules.len() + self.plan.additions.len())
            .map_err(|_| ModuleError32::Allocation)?;
        self.plan.reservations.extend(
            self.set
                .iter()
                .chain(&self.plan.additions)
                .map(|module| (module.base(), module.image_size())),
        );
        Ok(self.plan)
    }
}

fn initialization_order(modules: &[LoadedModule32]) -> Result<Vec<ModuleId32>, ModuleError32> {
    let mut states = Vec::new();
    states
        .try_reserve_exact(modules.len())
        .map_err(|_| ModuleError32::Allocation)?;
    states.resize(modules.len(), 0);
    let mut order = Vec::new();
    order
        .try_reserve_exact(modules.len())
        .map_err(|_| ModuleError32::Allocation)?;
    let mut stack = Vec::new();
    stack
        .try_reserve_exact(modules.len())
        .map_err(|_| ModuleError32::Allocation)?;
    for index in 0..modules.len() {
        if states[index] != 0 {
            continue;
        }
        states[index] = 1;
        stack.push((index, 0));
        while let Some((index, next)) = stack.last_mut() {
            if *next == modules[*index].dependencies.len() {
                let index = *index;
                states[index] = 2;
                order.push(modules[index].id);
                stack.pop();
                continue;
            }
            let id = modules[*index].dependencies[*next];
            *next += 1;
            if let Some(target) = modules.iter().position(|module| module.id == id)
                && states[target] == 0
            {
                states[target] = 1;
                stack.push((target, 0));
            }
        }
    }
    Ok(order)
}
