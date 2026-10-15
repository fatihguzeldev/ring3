use super::super::{DirectInputRuntime32, devices::actions::read_action_map, memory::words};
use crate::{
    input::{
        dinput::{
            DeviceKind32, DirectInputResult32 as H, InterfaceKind32, actions::default_object,
            format::DeviceDescription32,
        },
        source::InputSource32,
    },
    memory::{AddressSpace, GuestAddress},
    process::WindowsProviderOutcome32,
};
pub(in crate::process::windows::dinput) struct SemanticEnumeration32 {
    pub parent: GuestAddress,
    pub kinds: Vec<DeviceKind32>,
    pub current: Option<GuestAddress>,
    pub flags: Vec<u32>,
}
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn enum_semantics(
        &mut self,
        memory: &mut AddressSpace,
        source: &InputSource32,
        parent: GuestAddress,
        interface: InterfaceKind32,
        args: &[u32],
    ) -> Result<WindowsProviderOutcome32, H> {
        if args[5] & !0x0000_7110 != 0 {
            return Err(H::INVALID_PARAM);
        }
        let map = read_action_map(memory, args[2], interface.wide())?;
        let mut callback_flags = Vec::new();
        callback_flags
            .try_reserve_exact(10)
            .map_err(|_| H::OUT_OF_MEMORY)?;
        let mut kinds = Vec::new();
        kinds.try_reserve_exact(10).map_err(|_| H::OUT_OF_MEMORY)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(10).map_err(|_| H::OUT_OF_MEMORY)?;
        for kind in self
            .controllers
            .iter()
            .enumerate()
            .filter_map(|(slot, descriptor)| {
                descriptor
                    .as_ref()
                    .map(|_| DeviceKind32::Controller(slot as u8))
            })
            .chain([DeviceKind32::Keyboard, DeviceKind32::Mouse])
        {
            if matches!(kind,DeviceKind32::Controller(slot) if !source.snapshot().controllers[usize::from(slot)].connected)
            {
                continue;
            }
            if args[5] & 0x6100 != 0
                || args[5] & 0x1000 != 0 && self.owned_devices[super::device_index(kind)]
            {
                continue;
            }
            let description = match kind {
                DeviceKind32::Keyboard => DeviceDescription32::keyboard(),
                DeviceKind32::Mouse => DeviceDescription32::mouse(),
                DeviceKind32::Controller(slot) => {
                    let descriptor = self.controllers[usize::from(slot)].as_ref().unwrap();
                    DeviceDescription32 {
                        kind,
                        axes: descriptor.axes,
                        povs: descriptor.povs,
                        buttons: descriptor.buttons,
                    }
                }
            };
            let mut priorities = 0;
            let instance = self.device_guid(kind, false);
            for action in &map.actions {
                if action.flags & 5 != 0 {
                    continue;
                }
                let matches = if action.flags & 2 != 0 {
                    action.instance == instance
                        && description.objects().any(|object| {
                            object.instance() == ((action.object_id >> 8) & 0xffff) as u16
                                && object.object_type(kind == DeviceKind32::Mouse)
                                    & action.object_id
                                    & 255
                                    != 0
                        })
                } else {
                    default_object(description, action.semantic).is_some()
                };
                if matches {
                    priorities |= if action.semantic & 0x4000 == 0 { 1 } else { 2 };
                }
            }
            if priorities == 0 {
                continue;
            }
            callback_flags.push(
                priorities
                    | if self.used_devices[super::device_index(kind)] {
                        0x10
                    } else {
                        0x20
                    },
            );
            kinds.push(kind);
            rows.push(self.instance_bytes(
                kind,
                interface.wide(),
                true,
                if interface.wide() { 1100 } else { 580 },
            )?);
        }
        if kinds.is_empty() {
            return self.start_enumeration(memory, args[3], args[4], interface, rows);
        }
        let canonical = self.object(parent)?.pointer;
        self.com.check_add_ref(memory, canonical)?;
        if memory.transaction_version() > u64::MAX - 3 {
            return Err(H::INVALID_PARAM);
        }
        let result = self.start_enumeration(memory, args[3], args[4], interface, rows)?;
        self.com
            .add_ref(memory, canonical)
            .expect("enumeration parent retain preflighted");
        self.pending.last_mut().unwrap().semantics = Some(SemanticEnumeration32 {
            parent: canonical,
            kinds,
            current: None,
            flags: callback_flags,
        });
        if let Err(error) = self.prepare_semantic_device(memory) {
            let pending = self.pending.pop().unwrap();
            memory
                .unmap(pending.range)
                .expect("newly installed enumeration scratch");
            self.com
                .release(memory, canonical)
                .expect("enumeration parent rollback retains caller");
            return Err(error);
        }
        let _ = result;
        Ok(self.enumeration_callback())
    }
    pub(in crate::process::windows::dinput) fn prepare_semantic_device(
        &mut self,
        memory: &mut AddressSpace,
    ) -> Result<(), H> {
        let pending = self.pending.last().unwrap();
        let Some(semantics) = &pending.semantics else {
            return Ok(());
        };
        let parent = semantics.parent;
        let interface = pending.interface.device_interface();
        let address = pending.scratch.0;
        let kind = semantics.kinds[pending.cursor];
        let guid = self.device_guid(kind, false);
        memory.write(GuestAddress(address + 1536), &guid)?;
        self.create_device(memory, parent, interface, address + 1536, address + 1552, 0)?;
        let [pointer] = words(memory, address + 1552)?;
        self.pending
            .last_mut()
            .unwrap()
            .semantics
            .as_mut()
            .unwrap()
            .current = Some(GuestAddress(pointer));
        Ok(())
    }
}
