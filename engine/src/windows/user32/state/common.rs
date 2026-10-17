use super::*;
use crate::windows::comctl32::{CommonControlKind32, CommonControlState32};

impl User32State32 {
    pub(crate) fn common_windows(
        &self,
        kind: CommonControlKind32,
    ) -> impl Iterator<Item = WindowHandle32> + '_ {
        self.slots
            .iter()
            .filter(move |slot| self.common_kind(slot.data.class_atom) == Some(kind))
            .map(|slot| slot.data.handle)
    }

    pub(crate) fn common_kind(&self, atom: u16) -> Option<CommonControlKind32> {
        (atom != 0)
            .then(|| self.common_atoms.iter().position(|&entry| entry == atom))
            .flatten()
            .map(|index| CommonControlKind32::ALL[index])
    }

    pub(crate) fn common_atom(&self, kind: CommonControlKind32) -> Option<u16> {
        let atom = self.common_atoms[kind as usize];
        (atom != 0).then_some(atom)
    }

    pub(crate) fn common_state(
        &self,
        window: WindowHandle32,
    ) -> Result<&CommonControlState32, User32Error32> {
        self.slots[self.slot(window)?]
            .common
            .as_ref()
            .ok_or(User32Error32::Win32(1407))
    }

    pub(crate) fn common_state_mut(
        &mut self,
        window: WindowHandle32,
    ) -> Result<&mut CommonControlState32, User32Error32> {
        let index = self.slot(window)?;
        self.slots[index]
            .common
            .as_mut()
            .ok_or(User32Error32::Win32(1407))
    }

    pub(crate) fn install_common_classes(
        &mut self,
        flags: u32,
        procedure: crate::memory::GuestAddress,
        instance: u32,
    ) -> Result<(), User32Error32> {
        let mut classes = Vec::new();
        classes
            .try_reserve_exact(CommonControlKind32::ALL.len())
            .map_err(|_| User32Error32::Allocation)?;
        for kind in CommonControlKind32::ALL {
            if flags & kind.flags() == 0 || self.common_atoms[kind as usize] != 0 {
                continue;
            }
            let mut name = Vec::new();
            name.try_reserve_exact(kind.name().len())
                .map_err(|_| User32Error32::Allocation)?;
            name.extend(kind.name().bytes().map(u16::from));
            let mut class = WindowClass32 {
                name,
                procedure,
                instance,
                unicode: true,
                style: match kind {
                    CommonControlKind32::ListView => 0x4009,
                    CommonControlKind32::Progress => 0x4003,
                    CommonControlKind32::Status => 0x4009,
                    CommonControlKind32::Tab => 0x400b,
                    CommonControlKind32::Trackbar => 0x4000,
                    CommonControlKind32::UpDown => 0x4003,
                    CommonControlKind32::Animate => 0x4008,
                    CommonControlKind32::Header => 0x400b,
                    CommonControlKind32::Hotkey => 0x4000,
                    CommonControlKind32::Tooltip => 0x4008,
                    CommonControlKind32::Toolbar => 0x400b,
                },
                extra_bytes: 4,
                ..Default::default()
            };
            self.check_system_class(&mut class)?;
            classes.push((kind, class));
        }
        let new = classes.iter().filter(|(_, class)| class.atom == 0).count();
        self.next_atom
            .checked_add(new as u16)
            .ok_or(User32Error32::Capacity)?;
        self.classes
            .try_reserve(new)
            .map_err(|_| User32Error32::Allocation)?;
        self.system_atoms
            .try_reserve(new)
            .map_err(|_| User32Error32::Allocation)?;
        for (kind, class) in classes {
            let atom = self
                .install_system_class(class)
                .expect("common control group was preflighted");
            self.common_atoms[kind as usize] = atom;
        }
        Ok(())
    }
}
