use super::*;
impl User32State32 {
    pub(crate) fn standard_kind(
        &self,
        atom: u16,
    ) -> Option<crate::windows::controls::ControlKind32> {
        (atom != 0)
            .then(|| self.standard_atoms.iter().position(|value| *value == atom))
            .flatten()
            .map(|index| crate::windows::controls::ControlKind32::STANDARD[index])
    }
    pub(crate) fn standard_atom(&self, kind: crate::windows::controls::ControlKind32) -> u16 {
        self.standard_atoms[kind as usize]
    }
    pub(crate) fn text(&self, window: WindowHandle32) -> Result<&[u16], User32Error32> {
        let index = self.slot(window)?;
        let slot = &self.slots[index];
        if self.standard_kind(slot.data.class_atom)
            == Some(crate::windows::controls::ControlKind32::ComboBox)
            && let Some(text) = slot
                .control
                .selected
                .and_then(|selected| slot.control.items.get(selected))
        {
            return Ok(text);
        }
        Ok(&slot.text)
    }
    pub(crate) fn set_text(
        &mut self,
        window: WindowHandle32,
        text: Vec<u16>,
    ) -> Result<(), User32Error32> {
        let index = self.slot(window)?;
        if self.standard_kind(self.slots[index].data.class_atom)
            == Some(crate::windows::controls::ControlKind32::ComboBox)
        {
            self.slots[index].control.selected = None;
        }
        self.slots[index].text = text;
        Ok(())
    }
    pub(crate) fn control_state(
        &self,
        window: WindowHandle32,
    ) -> Result<&crate::windows::controls::ControlState32, User32Error32> {
        let index = self.control_slot(window)?;
        Ok(&self.slots[index].control)
    }
    pub(crate) fn control(
        &mut self,
        window: WindowHandle32,
    ) -> Result<&mut crate::windows::controls::ControlState32, User32Error32> {
        let index = self.control_slot(window)?;
        Ok(&mut self.slots[index].control)
    }
    fn control_slot(&self, window: WindowHandle32) -> Result<usize, User32Error32> {
        let index = self.slot(window)?;
        self.slots[index]
            .control
            .list_owner
            .map_or(Ok(index), |owner| self.slot(owner))
    }
    pub(crate) fn install_standard_classes(
        &mut self,
        procedure: crate::memory::GuestAddress,
    ) -> Result<(), User32Error32> {
        use crate::windows::controls::ControlKind32;
        if self.standard_atoms[0] != 0 {
            return Ok(());
        }
        let mut classes = Vec::new();
        classes
            .try_reserve_exact(ControlKind32::STANDARD.len())
            .map_err(|_| User32Error32::Allocation)?;
        for kind in ControlKind32::STANDARD {
            let mut name = Vec::new();
            name.try_reserve_exact(kind.name().len())
                .map_err(|_| User32Error32::Allocation)?;
            name.extend(kind.name().bytes().map(u16::from));
            let mut class = WindowClass32 {
                name,
                procedure,
                style: 0x4008,
                unicode: true,
                ..Default::default()
            };
            self.check_system_class(&mut class)?;
            classes.push(class);
        }
        let new = classes.iter().filter(|class| class.atom == 0).count();
        self.next_atom
            .checked_add(new as u16)
            .ok_or(User32Error32::Capacity)?;
        self.classes
            .try_reserve(new)
            .map_err(|_| User32Error32::Allocation)?;
        self.system_atoms
            .try_reserve(new)
            .map_err(|_| User32Error32::Allocation)?;
        for (index, class) in classes.into_iter().enumerate() {
            let atom = self
                .install_system_class(class)
                .expect("system class group was preflighted");
            self.standard_atoms[index] = atom;
        }
        Ok(())
    }
}
