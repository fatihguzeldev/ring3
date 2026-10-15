use super::LifetimeChange32;
use crate::process::windows::d3d8::D3D8Runtime32;
use crate::{
    graphics::d3d8::api::{HResult, InterfaceKind},
    memory::{AddressSpace, GuestAddress, WordWrite32},
};
use crate::{
    process::windows::d3d8::memory::{check_output, read_guid, word},
    windows::com::{ComError32, IID_IUNKNOWN, InterfaceId},
};
impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn query_interface(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        iid_pointer: u32,
        output: u32,
    ) -> Result<HResult, HResult> {
        check_output(memory, output, 4)?;
        let iid = InterfaceId(read_guid(memory, iid_pointer)?);
        self.com.preflight(memory, pointer)?;
        let object = self.object(pointer)?;
        let supported = iid == IID_IUNKNOWN
            || InterfaceKind::ALL
                .iter()
                .any(|kind| kind.iid() == iid && object.interface.supports(*kind));
        if !supported {
            memory.write_words32(&[word(output, 0)])?;
            return Ok(HResult::NO_INTERFACE);
        }
        let target = object.pointer;
        let counter = self.counter_pointer(pointer)?;
        self.change_lifetime(
            memory,
            LifetimeChange32 {
                retain: vec![counter],
                ..Default::default()
            },
            &[word(output, target.0)],
        )?;
        Ok(HResult::OK)
    }
    pub(in crate::process::windows::d3d8) fn add_ref(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
    ) -> Result<u32, HResult> {
        self.com.preflight(memory, pointer)?;
        let counter = self.counter_pointer(pointer)?;
        if self.com.references(counter)? != 0 {
            return Ok(self.com.add_ref(memory, counter)?);
        }
        self.change_lifetime(
            memory,
            LifetimeChange32 {
                retain: vec![counter],
                ..Default::default()
            },
            &[],
        )?;
        Ok(self.com.references(counter)?)
    }
    pub(in crate::process::windows::d3d8) fn release(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
    ) -> Result<u32, HResult> {
        self.com.preflight(memory, pointer)?;
        let counter = self.counter_pointer(pointer)?;
        let references = self.com.references(counter)?;
        if references == 0 {
            return Ok(0);
        }
        if references > 1 {
            return Ok(self.com.release(memory, counter)?.remaining);
        }
        self.change_lifetime(
            memory,
            LifetimeChange32 {
                release: vec![counter],
                ..Default::default()
            },
            &[],
        )?;
        Ok(0)
    }
    pub(in crate::process::windows::d3d8) fn acquire_output(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        output: u32,
    ) -> Result<(), HResult> {
        check_output(memory, output, 4)?;
        let counter = self.counter_pointer(pointer)?;
        self.change_lifetime(
            memory,
            LifetimeChange32 {
                retain: vec![counter],
                ..Default::default()
            },
            &[word(output, pointer.0)],
        )
    }
    pub(in crate::process::windows::d3d8) fn change_lifetime(
        &mut self,
        memory: &mut AddressSpace,
        change: LifetimeChange32,
        writes: &[WordWrite32],
    ) -> Result<(), HResult> {
        if change.retain.len() == 1
            && change.release.is_empty()
            && change.hold.is_empty()
            && change.unhold.is_empty()
            && self.com.references(change.retain[0])? != 0
        {
            self.com.check_add_ref(memory, change.retain[0])?;
            memory.write_words32(writes)?;
            self.com
                .add_ref(memory, change.retain[0])
                .expect("preflighted reference increment remains current");
            return Ok(());
        }
        let prepared = self.prepare_lifetime(memory, change, writes)?;
        self.commit_lifetime(memory, prepared);
        Ok(())
    }
}
impl From<ComError32> for HResult {
    fn from(error: ComError32) -> Self {
        if error == ComError32::NoInterface {
            Self::NO_INTERFACE
        } else {
            Self::INVALID_CALL
        }
    }
}
