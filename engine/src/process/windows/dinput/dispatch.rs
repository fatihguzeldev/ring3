use super::{
    DirectInputRuntime32,
    memory::{output, read},
    objects::ObjectKind32,
};
use crate::{
    input::{
        dinput::{
            DirectInputResult32 as H, FactoryGate, InterfaceKind32, MethodGate,
            api::{SYSTEM_KEYBOARD, SYSTEM_MOUSE},
        },
        source::InputSource32,
    },
    memory::{AddressSpace, GuestAddress},
    process::WindowsProviderOutcome32,
    windows::{
        com::InterfaceId, synchronization::SynchronizationContext32, window::WindowContext32,
    },
};
impl DirectInputRuntime32 {
    pub(super) fn factory(
        &mut self,
        memory: &mut AddressSpace,
        gate: FactoryGate,
        args: &[u32],
    ) -> Result<WindowsProviderOutcome32, H> {
        let (interface, out, outer) = match gate {
            FactoryGate::Create8 | FactoryGate::CreateEx => {
                let iid = InterfaceId(read(memory, args[2])?);
                (
                    InterfaceKind32::from_iid(iid)
                        .filter(|kind| {
                            !kind.device()
                                && (kind.generation() == 8) == (gate == FactoryGate::Create8)
                        })
                        .ok_or(H::NO_INTERFACE)?,
                    args[3],
                    args[4],
                )
            }
            FactoryGate::CreateA => (InterfaceKind32::Input1A, args[2], args[3]),
            FactoryGate::CreateW => (InterfaceKind32::Input1W, args[2], args[3]),
        };
        output(memory, out, 4)?;
        if outer != 0 {
            return Err(H::NO_AGGREGATION);
        }
        if args[0] == 0 {
            return Err(H::HANDLE);
        }
        if args[1]
            < (if gate == FactoryGate::Create8 {
                0x800
            } else {
                0x300
            })
        {
            return Err(H(0x8007_047e));
        }
        if args[1]
            > (if gate == FactoryGate::Create8 {
                0x800
            } else {
                0x700
            })
        {
            return Err(H(0x8007_000a));
        }
        self.publish(memory, interface, args[1], ObjectKind32::Input, out, None)?;
        Ok(WindowsProviderOutcome32::Return(0))
    }
    pub(super) fn method(
        &mut self,
        memory: &mut AddressSpace,
        windows: Option<&WindowContext32>,
        sync: Option<&mut SynchronizationContext32>,
        source: &InputSource32,
        gate: MethodGate,
        args: &[u32],
    ) -> Result<WindowsProviderOutcome32, H> {
        let pointer = GuestAddress(args[0]);
        self.com.preflight(memory, pointer)?;
        let record = self.object(pointer)?;
        if !record.interface.compatible(gate.interface) {
            return Err(H::INVALID_PARAM);
        }
        match gate.slot {
            0 => {
                return self
                    .query(memory, pointer, args[1], args[2])
                    .map(WindowsProviderOutcome32::Return);
            }
            1 => {
                return self
                    .com
                    .add_ref(memory, pointer)
                    .map(WindowsProviderOutcome32::Return)
                    .map_err(H::from);
            }
            2 => {
                return self
                    .release(memory, sync, pointer)
                    .map(WindowsProviderOutcome32::Return);
            }
            _ => {}
        }
        if gate.interface.device() {
            if gate.slot == 4 {
                return self.enum_objects(
                    memory,
                    pointer,
                    gate.interface,
                    args[1],
                    args[2],
                    args[3],
                );
            }
            return self
                .device_call(
                    memory,
                    windows,
                    sync,
                    source,
                    gate.interface,
                    args,
                    gate.slot,
                )
                .map(WindowsProviderOutcome32::Return);
        }
        match gate.slot {
            3 => {
                let interface = gate.interface.device_interface();
                self.create_device(memory, pointer, interface, args[1], args[2], args[3])
                    .map(WindowsProviderOutcome32::Return)
            }
            4 => self.enum_devices(
                memory,
                source,
                gate.interface,
                args[1],
                args[2],
                args[3],
                args[4],
            ),
            5 => {
                let kind = self.identify(read(memory, args[1])?)?;
                let result = if matches!(kind,crate::input::dinput::DeviceKind32::Controller(slot) if !source.snapshot().controllers[usize::from(slot)].connected)
                {
                    1
                } else {
                    0
                };
                Ok(WindowsProviderOutcome32::Return(result))
            }
            6 => Err(H::UNSUPPORTED),
            7 => {
                if args[1] == 0 || args[2] == 0 {
                    Err(H::INVALID_PARAM)
                } else {
                    Ok(WindowsProviderOutcome32::Return(0))
                }
            }
            8 => {
                output(memory, args[3], 16)?;
                let class = read::<16>(memory, args[1])?;
                let guid = if class == SYSTEM_KEYBOARD || class == SYSTEM_MOUSE {
                    class
                } else {
                    self.controllers
                        .iter()
                        .flatten()
                        .find(|controller| {
                            controller.product == class || controller.instance == class
                        })
                        .map(|controller| controller.instance)
                        .ok_or(H::OBJECT_NOT_FOUND)?
                };
                memory.write(GuestAddress(args[3]), &guid)?;
                Ok(WindowsProviderOutcome32::Return(0))
            }
            9 if gate.interface.generation() == 7 => {
                let interface = InterfaceKind32::from_iid(InterfaceId(read(memory, args[2])?))
                    .filter(|kind| kind.device() && kind.generation() != 8)
                    .ok_or(H::NO_INTERFACE)?;
                self.create_device(memory, pointer, interface, args[1], args[3], args[4])
                    .map(WindowsProviderOutcome32::Return)
            }
            9 => self.enum_semantics(memory, source, pointer, gate.interface, args),
            10 => Err(H::UNSUPPORTED),
            _ => Err(H::INVALID_PARAM),
        }
    }
}
