use super::{
    Ole32Runtime32,
    memory::{com_error, failure, output},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::objects::{GuestComCreation32, InterfaceLayout32},
    windows::{
        com::{IID_IUNKNOWN, InterfaceId},
        ole32::{HResult32, IID_ICLASSFACTORY, KnownClass32},
    },
};
impl Ole32Runtime32 {
    pub(super) fn publish_factory(
        &mut self,
        memory: &mut AddressSpace,
        class: KnownClass32,
        iid: InterfaceId,
        out: u32,
    ) -> Result<u32, HResult32> {
        output(memory, out)?;
        if iid != IID_IUNKNOWN && iid != IID_ICLASSFACTORY {
            return failure(memory, out, HResult32::NO_INTERFACE);
        }
        let slot = self
            .factories
            .iter()
            .position(Option::is_none)
            .ok_or(HResult32::OUT_OF_MEMORY)?;
        self.com
            .create_with_lifetime(
                memory,
                slot as u64,
                &[InterfaceLayout32 {
                    iid: IID_ICLASSFACTORY,
                    methods: &self.methods,
                }],
                GuestComCreation32 {
                    output: Some((GuestAddress(out), iid)),
                    ..Default::default()
                },
            )
            .map_err(com_error)?;
        self.factories[slot] = Some(class);
        Ok(0)
    }
    pub(super) fn factory_class(
        &self,
        memory: &AddressSpace,
        pointer: u32,
    ) -> Result<KnownClass32, HResult32> {
        self.com
            .preflight(memory, GuestAddress(pointer))
            .map_err(com_error)?;
        let identity = self.com.lookup(GuestAddress(pointer)).map_err(com_error)?;
        self.factories
            .get(identity.payload as usize)
            .and_then(|class| *class)
            .ok_or(HResult32::UNEXPECTED)
    }
    pub(super) fn query_factory(
        &mut self,
        memory: &mut AddressSpace,
        pointer: u32,
        iid: InterfaceId,
        out: u32,
    ) -> Result<u32, HResult32> {
        output(memory, out)?;
        self.factory_class(memory, pointer)?;
        if iid != IID_IUNKNOWN && iid != IID_ICLASSFACTORY {
            return failure(memory, out, HResult32::NO_INTERFACE);
        }
        let target = self
            .com
            .query_target(memory, GuestAddress(pointer), iid)
            .map_err(com_error)?;
        let prepared = self
            .com
            .prepare_lifetime_changes(
                memory,
                &[GuestAddress(pointer)],
                &[],
                &[],
                &[],
                &[crate::memory::WordWrite32 {
                    address: GuestAddress(out),
                    value: target.0,
                }],
            )
            .map_err(com_error)?;
        self.com
            .commit_lifetime_changes(memory, prepared)
            .map_err(com_error)?;
        Ok(0)
    }
    pub(super) fn release_factory(
        &mut self,
        memory: &mut AddressSpace,
        pointer: u32,
    ) -> Result<u32, HResult32> {
        let outcome = self
            .com
            .release(memory, GuestAddress(pointer))
            .map_err(com_error)?;
        if let Some(identity) = outcome.destroyed {
            self.factories[identity.payload as usize] = None;
        }
        Ok(outcome.remaining)
    }
    pub(super) fn lock_server(
        &mut self,
        class: KnownClass32,
        lock: bool,
    ) -> Result<u32, HResult32> {
        let index = KnownClass32::ALL
            .iter()
            .position(|candidate| *candidate == class)
            .unwrap();
        let count = &mut self.server_locks[index];
        *count = if lock {
            count.checked_add(1).ok_or(HResult32::OUT_OF_MEMORY)?
        } else {
            count.checked_sub(1).ok_or(HResult32::UNEXPECTED)?
        };
        Ok(0)
    }
}
