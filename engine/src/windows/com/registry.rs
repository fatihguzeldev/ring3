use super::{
    ComError32, ComIdentity32, ComObjectId32, IID_IUNKNOWN, InterfaceBinding32, InterfaceId,
    ReferenceState32, ReferenceTransition32, ReleaseOutcome32,
};
use crate::memory::GuestAddress;

mod references;

struct Object {
    identity: ComIdentity32,
    interfaces: Box<[InterfaceBinding32]>,
    references: u32,
    holds: u32,
}

#[derive(Clone, Copy)]
struct Pointer {
    address: GuestAddress,
    slot: usize,
}

pub(crate) struct PreparedRegistration32<'a> {
    registry: &'a mut ComRegistry32,
    object: Object,
    slot: usize,
    interfaces: usize,
    next_id: u64,
    retained: Vec<(usize, u32)>,
}

impl PreparedRegistration32<'_> {
    pub(crate) fn publish(self) -> ComIdentity32 {
        let Self {
            registry,
            object,
            slot,
            interfaces,
            next_id,
            retained,
        } = self;
        let identity = object.identity;
        for (slot, references) in retained {
            registry.objects[slot].as_mut().unwrap().references = references;
        }
        let mut previous = None;
        for binding in &object.interfaces {
            if previous != Some(binding.address) {
                registry.pointers.push(Pointer {
                    address: binding.address,
                    slot,
                });
                previous = Some(binding.address);
            }
        }
        registry.highest_address = object.interfaces.last().unwrap().address.0;
        registry.objects[slot] = Some(object);
        registry.interfaces = interfaces;
        registry.next_id = next_id;
        registry.live += 1;
        identity
    }
}

pub struct ComRegistry32 {
    objects: Vec<Option<Object>>,
    pointers: Vec<Pointer>,
    interface_limit: usize,
    interfaces: usize,
    next_id: u64,
    highest_address: u32,
    live: usize,
}

impl ComRegistry32 {
    pub fn new(max_objects: usize, max_interfaces: usize) -> Result<Self, ComError32> {
        if max_objects == 0
            || max_objects > 65_536
            || max_interfaces < max_objects
            || max_interfaces > 262_144
        {
            return Err(ComError32::InvalidLimits);
        }
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(max_objects)
            .map_err(|_| ComError32::Allocation)?;
        objects.resize_with(max_objects, || None);
        let mut pointers = Vec::new();
        pointers
            .try_reserve_exact(max_interfaces)
            .map_err(|_| ComError32::Allocation)?;
        Ok(Self {
            objects,
            pointers,
            interface_limit: max_interfaces,
            interfaces: 0,
            next_id: 0,
            highest_address: 0,
            live: 0,
        })
    }

    pub fn live_objects(&self) -> usize {
        self.live
    }

    /// guest interface storage is allocated monotonically for the process lifetime.
    pub fn register(
        &mut self,
        payload: u64,
        bindings: &[InterfaceBinding32],
    ) -> Result<ComObjectId32, ComError32> {
        Ok(self.prepare_registration(payload, bindings)?.publish().id)
    }

    pub(crate) fn prepare_registration(
        &mut self,
        payload: u64,
        bindings: &[InterfaceBinding32],
    ) -> Result<PreparedRegistration32<'_>, ComError32> {
        self.prepare_registration_with_lifetime(
            payload,
            bindings,
            ReferenceState32 {
                references: 1,
                holds: 0,
            },
            &[],
        )
    }

    pub(crate) fn prepare_registration_with_lifetime(
        &mut self,
        payload: u64,
        bindings: &[InterfaceBinding32],
        initial: ReferenceState32,
        retain: &[GuestAddress],
    ) -> Result<PreparedRegistration32<'_>, ComError32> {
        if initial.references == 0 && initial.holds == 0 {
            return Err(ComError32::InvalidBinding);
        }
        let retained = self.creation_retains(retain)?;
        if bindings.is_empty() || bindings.len() > 64 {
            return Err(ComError32::InvalidBinding);
        }
        for (index, binding) in bindings.iter().enumerate() {
            if binding.address.0 == 0
                || !binding.address.0.is_multiple_of(4)
                || binding.vtable.0 == 0
                || !binding.vtable.0.is_multiple_of(4)
                || bindings[..index].iter().any(|earlier| {
                    earlier.iid == binding.iid
                        || earlier.address == binding.address && earlier.vtable != binding.vtable
                })
            {
                return Err(ComError32::InvalidBinding);
            }
        }
        if bindings
            .iter()
            .any(|binding| binding.address.0 <= self.highest_address)
        {
            return Err(ComError32::PointerReuse);
        }
        let interfaces = self
            .interfaces
            .checked_add(bindings.len())
            .filter(|total| *total <= self.interface_limit)
            .ok_or(ComError32::Capacity)?;
        let slot = self
            .objects
            .iter()
            .position(Option::is_none)
            .ok_or(ComError32::Capacity)?;
        let next_id = self.next_id.checked_add(1).ok_or(ComError32::Exhausted)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bindings.len())
            .map_err(|_| ComError32::Allocation)?;
        owned.extend_from_slice(bindings);
        owned.sort_unstable_by_key(|binding| binding.address.0);
        let identity_pointer = bindings
            .iter()
            .find(|binding| binding.iid == IID_IUNKNOWN)
            .unwrap_or(&bindings[0])
            .address;
        let id = ComObjectId32::new(next_id);
        let object = Object {
            identity: ComIdentity32 {
                id,
                identity_pointer,
                payload,
            },
            interfaces: owned.into_boxed_slice(),
            references: initial.references,
            holds: initial.holds,
        };
        Ok(PreparedRegistration32 {
            registry: self,
            object,
            slot,
            interfaces,
            next_id,
            retained,
        })
    }

    pub fn lookup(&self, pointer: GuestAddress) -> Result<ComIdentity32, ComError32> {
        Ok(self.object(pointer)?.identity)
    }

    pub fn references(&self, pointer: GuestAddress) -> Result<u32, ComError32> {
        Ok(self.object(pointer)?.references)
    }

    pub(crate) fn holds(&self, pointer: GuestAddress) -> Result<u32, ComError32> {
        Ok(self.object(pointer)?.holds)
    }

    pub fn state(&self, pointer: GuestAddress) -> Result<ReferenceState32, ComError32> {
        let object = self.object(pointer)?;
        Ok(ReferenceState32 {
            references: object.references,
            holds: object.holds,
        })
    }

    pub fn query_target(
        &self,
        pointer: GuestAddress,
        iid: InterfaceId,
    ) -> Result<GuestAddress, ComError32> {
        let object = self.object(pointer)?;
        if iid == IID_IUNKNOWN {
            return Ok(object.identity.identity_pointer);
        }
        object
            .interfaces
            .iter()
            .find(|binding| binding.iid == iid)
            .map(|binding| binding.address)
            .ok_or(ComError32::NoInterface)
    }

    pub fn query_interface(
        &mut self,
        pointer: GuestAddress,
        iid: InterfaceId,
    ) -> Result<GuestAddress, ComError32> {
        let target = self.query_target(pointer, iid)?;
        self.add_ref(pointer)?;
        Ok(target)
    }

    pub fn add_ref(&mut self, pointer: GuestAddress) -> Result<u32, ComError32> {
        let slot = self.slot(pointer)?;
        let object = self.objects[slot].as_mut().unwrap();
        let references = object
            .references
            .checked_add(1)
            .ok_or(ComError32::Exhausted)?;
        object.references = references;
        Ok(references)
    }

    pub fn release(&mut self, pointer: GuestAddress) -> Result<ReleaseOutcome32, ComError32> {
        let slot = self.slot(pointer)?;
        let object = self.objects[slot].as_mut().unwrap();
        if object.references == 0 {
            return Ok(ReleaseOutcome32 {
                remaining: 0,
                destroyed: None,
            });
        }
        object.references -= 1;
        if object.references != 0 || object.holds != 0 {
            return Ok(ReleaseOutcome32 {
                remaining: object.references,
                destroyed: None,
            });
        }
        let object = self.objects[slot].take().unwrap();
        self.pointers.retain(|entry| entry.slot != slot);
        self.interfaces -= object.interfaces.len();
        self.live -= 1;
        Ok(ReleaseOutcome32 {
            remaining: 0,
            destroyed: Some(object.identity),
        })
    }

    fn object(&self, pointer: GuestAddress) -> Result<&Object, ComError32> {
        Ok(self.objects[self.slot(pointer)?].as_ref().unwrap())
    }

    fn slot(&self, pointer: GuestAddress) -> Result<usize, ComError32> {
        self.pointers
            .binary_search_by_key(&pointer.0, |entry| entry.address.0)
            .map(|index| self.pointers[index].slot)
            .map_err(|_| ComError32::InvalidPointer)
    }
}

#[cfg(test)]
mod tests;
