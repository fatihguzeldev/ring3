use super::{LifetimeChange32, PreparedLifetimeChange32};
use crate::process::windows::d3d8::{
    D3D8Runtime32, DispatchError32, garbage::Garbage32, locks::PreparedLockDisposal32,
    objects::ObjectKind32,
};
use crate::windows::com::ComObjectId32;
use crate::{
    graphics::d3d8::api::HResult,
    memory::{AddressSpace, WordWrite32},
};
impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn prepare_lifetime(
        &mut self,
        memory: &AddressSpace,
        mut change: LifetimeChange32,
        writes: &[WordWrite32],
    ) -> Result<PreparedLifetimeChange32, HResult> {
        let basic = self.com.prepare_reference_receipt(
            memory,
            &change.retain,
            &change.release,
            &change.hold,
            &change.unhold,
        )?;
        if basic.destroyed().is_empty()
            && basic.transitions().iter().all(|transition| {
                !matches!(
                    self.records[transition.identity.payload as usize]
                        .as_ref()
                        .map(|object| &object.kind),
                    Some(ObjectKind32::Resource(_))
                ) || (transition.before.references == 0) == (transition.after.references == 0)
            })
        {
            return Ok(PreparedLifetimeChange32 {
                com: self
                    .com
                    .prepare_lifetime_receipt(memory, basic, writes, &[])?,
                external_releases: Vec::new(),
                garbage: Vec::new(),
                locks: PreparedLockDisposal32 {
                    ranges: Vec::new(),
                    tokens: Vec::new(),
                },
            });
        }
        drop(basic);
        let mut edges: Vec<(ComObjectId32, i8)> = Vec::new();
        let mut disposed: Vec<ComObjectId32> = Vec::new();
        let mut external_releases = Vec::new();
        loop {
            let preview = self.com.preview_reference_changes(
                memory,
                &change.retain,
                &change.release,
                &change.hold,
                &change.unhold,
            )?;
            let mut edge_changed = false;
            for transition in &preview {
                let object = self
                    .records
                    .get(transition.identity.payload as usize)
                    .and_then(Option::as_ref)
                    .ok_or(HResult::INVALID_CALL)?;
                let ObjectKind32::Resource(resource) = &object.kind else {
                    continue;
                };
                let contribution = if transition.before.references == 0
                    && transition.after.references != 0
                {
                    1
                } else if transition.before.references != 0 && transition.after.references == 0 {
                    -1
                } else {
                    0
                };
                let index = edges
                    .iter()
                    .position(|(id, _)| *id == transition.identity.id);
                let previous = index.map_or(0, |index| edges[index].1);
                let delta = contribution - previous;
                if delta == 0 {
                    continue;
                }
                if delta > 0 {
                    change
                        .retain
                        .try_reserve(delta as usize)
                        .map_err(|_| HResult::OUT_OF_MEMORY)?;
                    for _ in 0..delta {
                        change.retain.push(resource.device);
                    }
                } else {
                    change
                        .release
                        .try_reserve((-delta) as usize)
                        .map_err(|_| HResult::OUT_OF_MEMORY)?;
                    for _ in 0..-delta {
                        change.release.push(resource.device);
                    }
                }
                if let Some(index) = index {
                    edges[index].1 = contribution;
                } else {
                    edges.try_reserve(1).map_err(|_| HResult::OUT_OF_MEMORY)?;
                    edges.push((transition.identity.id, contribution));
                }
                edge_changed = true;
            }
            if edge_changed {
                continue;
            }
            let mut added = false;
            for transition in preview {
                if transition.after.references != 0
                    || transition.after.holds != 0
                    || disposed.contains(&transition.identity.id)
                {
                    continue;
                }
                let object = self
                    .records
                    .get(transition.identity.payload as usize)
                    .and_then(Option::as_ref)
                    .ok_or(HResult::INVALID_CALL)?;
                disposed
                    .try_reserve(1)
                    .map_err(|_| HResult::OUT_OF_MEMORY)?;
                disposed.push(transition.identity.id);
                added = true;
                match &object.kind {
                    ObjectKind32::Device(device) => {
                        change
                            .release
                            .try_reserve(1)
                            .map_err(|_| HResult::OUT_OF_MEMORY)?;
                        change
                            .unhold
                            .try_reserve(
                                device.implicit.len()
                                    + device.resource_holds.len()
                                    + device.target_holds.iter().flatten().count(),
                            )
                            .map_err(|_| HResult::OUT_OF_MEMORY)?;
                        change.release.push(device.parent);
                        change.unhold.extend_from_slice(&device.implicit);
                        change.unhold.extend_from_slice(&device.resource_holds);
                        change
                            .unhold
                            .extend(device.target_holds.iter().flatten().copied());
                    }
                    ObjectKind32::SwapChain(chain) => {
                        change
                            .release
                            .try_reserve(1)
                            .map_err(|_| HResult::OUT_OF_MEMORY)?;
                        change
                            .unhold
                            .try_reserve(chain.surfaces.len())
                            .map_err(|_| HResult::OUT_OF_MEMORY)?;
                        change.release.push(chain.device);
                        change.unhold.extend_from_slice(&chain.surfaces);
                    }
                    _ => {}
                }
                change
                    .release
                    .try_reserve(object.private_unknowns.len())
                    .map_err(|_| HResult::OUT_OF_MEMORY)?;
                for entry in &object.private_unknowns {
                    if self.com.lookup(entry.pointer).is_ok() {
                        change.release.push(self.counter_pointer(entry.pointer)?);
                    } else {
                        external_releases
                            .try_reserve(1)
                            .map_err(|_| HResult::OUT_OF_MEMORY)?;
                        external_releases.push(entry.pointer);
                    }
                }
                let views = self.records.iter().flatten().filter(
                    |view| matches!(view.kind,ObjectKind32::View(v) if v.container==object.pointer),
                );
                change
                    .release
                    .try_reserve(views.clone().count())
                    .map_err(|_| HResult::OUT_OF_MEMORY)?;
                change.release.extend(views.map(|view| view.pointer));
            }
            if !added {
                break;
            }
        }
        let identities = self.com.prepare_lifetime_changes(
            memory,
            &change.retain,
            &change.release,
            &change.hold,
            &change.unhold,
            &[],
        )?;
        let mut garbage = Vec::new();
        let mut lock_resources = Vec::new();
        let chain_resources = identities
            .destroyed()
            .iter()
            .map(
                |id| match &self.records[id.payload as usize].as_ref().unwrap().kind {
                    ObjectKind32::SwapChain(chain) => chain.resources.len(),
                    _ => 0,
                },
            )
            .sum::<usize>();
        garbage
            .try_reserve_exact(
                identities
                    .destroyed()
                    .len()
                    .checked_add(chain_resources)
                    .ok_or(HResult::OUT_OF_MEMORY)?,
            )
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for identity in identities.destroyed() {
            let object = self.records[identity.payload as usize]
                .as_ref()
                .ok_or(HResult::INVALID_CALL)?;
            match &object.kind {
                ObjectKind32::Resource(resource) => push_garbage(
                    &mut garbage,
                    &self.garbage,
                    Garbage32::Resource {
                        device: resource.device,
                        resource: resource.resource,
                    },
                ),
                ObjectKind32::View(view) => push_garbage(
                    &mut garbage,
                    &self.garbage,
                    Garbage32::Namespace {
                        device: view.device,
                        resource: view.surface.resource,
                        namespace: identity.id.get(),
                    },
                ),
                ObjectKind32::SwapChain(chain) => {
                    for surface in &chain.resources {
                        let survives=self.records.iter().enumerate().any(|(slot,record)|matches!(record.as_ref().map(|object|&object.kind),Some(ObjectKind32::Resource(resource)) if resource.resource==surface.resource)
                            && !identities.destroyed().iter().any(|identity|identity.payload as usize==slot));
                        if !survives {
                            push_garbage(
                                &mut garbage,
                                &self.garbage,
                                Garbage32::Resource {
                                    device: chain.device,
                                    resource: surface.resource,
                                },
                            );
                        }
                    }
                }
                ObjectKind32::Device(device) => {
                    lock_resources
                        .try_reserve(device.backend.resources().ids().count())
                        .map_err(|_| HResult::OUT_OF_MEMORY)?;
                    lock_resources.extend(device.backend.resources().ids());
                }
                _ => {}
            }
        }
        lock_resources
            .try_reserve(garbage.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for item in &garbage {
            if let Garbage32::Resource { resource, .. } = item
                && !lock_resources.contains(resource)
            {
                lock_resources.push(*resource);
            }
        }
        let locks = self
            .locks
            .prepare_disposal(memory, &lock_resources)
            .map_err(|error| match error {
                DispatchError32::Api(value) => value,
                _ => HResult::INVALID_CALL,
            })?;
        let devices = identities
            .destroyed()
            .iter()
            .filter(|id| {
                matches!(
                    self.records[id.payload as usize]
                        .as_ref()
                        .map(|object| &object.kind),
                    Some(ObjectKind32::Device(_))
                )
            })
            .count();
        drop(identities);
        let com = self.com.prepare_lifetime_changes_with_unmaps(
            memory,
            &change.retain,
            &change.release,
            &change.hold,
            &change.unhold,
            writes,
            &locks.ranges,
        )?;
        self.garbage
            .try_reserve(garbage.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        self.retired_devices
            .try_reserve(devices)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        self.deferred_private_releases
            .try_reserve(external_releases.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        if !external_releases.is_empty() {
            if self.private_callbacks.len() >= self.config.max_objects as usize + 32 {
                return Err(HResult::OUT_OF_MEMORY);
            }
            self.private_callbacks
                .try_reserve(1)
                .map_err(|_| HResult::OUT_OF_MEMORY)?;
        }
        Ok(PreparedLifetimeChange32 {
            com,
            external_releases,
            garbage,
            locks,
        })
    }
}
fn push_garbage(planned: &mut Vec<Garbage32>, existing: &[Garbage32], item: Garbage32) {
    if !planned.contains(&item) && !existing.contains(&item) {
        planned.push(item);
    }
}
