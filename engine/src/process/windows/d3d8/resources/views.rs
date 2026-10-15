use super::super::{
    D3D8Runtime32, DispatchError32,
    memory::{check_output, read_guid, word},
    objects::{Object32, ObjectKind32, View32},
};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind},
        resources::SurfaceRef,
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::objects::{GuestComCreation32, InterfaceLayout32},
    windows::com::{IID_IUNKNOWN, InterfaceId, ReferenceState32},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn acquire_resource_view(
        &mut self,
        memory: &mut AddressSpace,
        container: GuestAddress,
        device: GuestAddress,
        surface: SurfaceRef,
        interface: InterfaceKind,
        output: u32,
    ) -> Result<(), DispatchError32> {
        check_output(memory, output, 4)?;
        if let Some(pointer) = self
            .records
            .iter()
            .flatten()
            .find_map(|object| match object.kind {
                ObjectKind32::View(view)
                    if view.container == container && view.surface == surface =>
                {
                    Some(object.pointer)
                }
                _ => None,
            })
        {
            self.acquire_output(memory, pointer, output)?;
            return Ok(());
        }
        let record = self.vacant_record()?;
        let identity = self.com.create_with_lifetime(
            memory,
            record as u64,
            &[InterfaceLayout32 {
                iid: interface.iid(),
                methods: &self.methods[interface as usize - 1],
            }],
            GuestComCreation32 {
                output: Some((GuestAddress(output), interface.iid())),
                writes: &[],
                initial: ReferenceState32 {
                    references: 1,
                    holds: 0,
                },
                retain: &[container],
            },
        )?;
        self.records[record] = Some(Object32 {
            pointer: identity.identity_pointer,
            interface,
            private_unknowns: Vec::new(),
            kind: ObjectKind32::View(View32 {
                device,
                container,
                surface,
            }),
        });
        Ok(())
    }

    pub(super) fn resource_container(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        iid_pointer: u32,
        output: u32,
    ) -> Result<u32, DispatchError32> {
        check_output(memory, output, 4)?;
        let iid = InterfaceId(read_guid(memory, iid_pointer)?);
        let container = match self.object(pointer)?.kind {
            ObjectKind32::View(view) => view.container,
            ObjectKind32::Resource(resource) => resource.device,
            _ => return Err(HResult::INVALID_CALL.into()),
        };
        let object = self.object(container)?;
        if iid != IID_IUNKNOWN
            && !InterfaceKind::ALL
                .iter()
                .any(|kind| kind.iid() == iid && object.interface.supports(*kind))
        {
            memory.write_words32(&[word(output, 0)])?;
            return Ok(HResult::NO_INTERFACE.0);
        }
        self.acquire_output(memory, object.pointer, output)?;
        Ok(0)
    }
}
