use super::*;

impl User32State32 {
    pub(crate) fn is_system_class(&self, atom: u16) -> bool {
        self.system_atoms.contains(&atom)
    }

    pub(super) fn check_system_class(
        &self,
        class: &mut WindowClass32,
    ) -> Result<Option<u16>, User32Error32> {
        if class.name.is_empty()
            || class.name.len() > 255
            || class.procedure.0 == 0
            || class.extra_bytes > 4096
        {
            return Err(User32Error32::Win32(87));
        }
        class.style |= 0x4000;
        class.atom = 0;
        if let Some(existing) = self.classes.iter().find(|existing| {
            existing.instance == class.instance && names_equal(&existing.name, &class.name)
        }) {
            class.atom = existing.atom;
            if self.is_system_class(existing.atom) && existing == class {
                return Ok(Some(existing.atom));
            }
            return Err(User32Error32::Win32(1410));
        }
        Ok(None)
    }

    pub(crate) fn install_system_class(
        &mut self,
        mut class: WindowClass32,
    ) -> Result<u16, User32Error32> {
        if let Some(atom) = self.check_system_class(&mut class)? {
            return Ok(atom);
        }
        let atom = self
            .next_atom
            .checked_add(1)
            .ok_or(User32Error32::Capacity)?;
        self.classes
            .try_reserve(1)
            .map_err(|_| User32Error32::Allocation)?;
        self.system_atoms
            .try_reserve(1)
            .map_err(|_| User32Error32::Allocation)?;
        class.atom = atom;
        self.classes.push(class);
        self.system_atoms.push(atom);
        self.next_atom = atom;
        Ok(atom)
    }
}

#[cfg(test)]
mod tests;
