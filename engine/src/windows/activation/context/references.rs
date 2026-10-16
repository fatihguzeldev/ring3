use super::ActivationContexts32;
use crate::windows::activation::ActivationError32 as E;

impl ActivationContexts32 {
    pub fn can_add_ref(&self, handle: u32) -> Result<(), E> {
        if handle != 0 {
            self.context(handle)?
                .references
                .checked_add(1)
                .ok_or(E::NotEnoughMemory)?;
            self.next_revision()?;
        }
        Ok(())
    }
    pub fn add_ref(&mut self, handle: u32) -> Result<(), E> {
        self.can_add_ref(handle)?;
        if handle != 0 {
            self.context_mut(handle)?.references += 1;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn can_release(&self, handle: u32) -> Result<bool, E> {
        if handle == 0 {
            return Ok(false);
        }
        let context = self.context(handle)?;
        if context.references == context.holds {
            return Err(E::InvalidHandle);
        }
        self.next_revision()?;
        Ok(context.references == 1)
    }
    pub fn release(&mut self, handle: u32) -> Result<bool, E> {
        let disposed = self.can_release(handle)?;
        if handle == 0 {
            return Ok(false);
        }
        let slot = self
            .contexts
            .iter()
            .position(|slot| {
                slot.as_ref()
                    .is_some_and(|context| context.handle == handle)
            })
            .unwrap();
        if disposed {
            self.contexts[slot] = None;
        } else {
            self.contexts[slot].as_mut().unwrap().references -= 1;
        }
        self.revision += 1;
        Ok(disposed)
    }
    pub fn zombify(&mut self, handle: u32) -> Result<(), E> {
        if handle == 0 {
            return Err(E::InvalidParameter);
        }
        self.context(handle)?;
        self.next_revision()?;
        self.context_mut(handle)?.disabled = true;
        self.revision += 1;
        Ok(())
    }
    pub fn references(&self, handle: u32) -> Result<u32, E> {
        Ok(self.context(handle)?.references)
    }
}

impl ActivationContexts32 {
    pub fn can_retain_hold(&self, handle: u32) -> Result<(), E> {
        if handle != 0 {
            let context = self.context(handle)?;
            context
                .references
                .checked_add(1)
                .ok_or(E::NotEnoughMemory)?;
            context.holds.checked_add(1).ok_or(E::NotEnoughMemory)?;
            self.next_revision()?;
        }
        Ok(())
    }
    pub fn retain_hold(&mut self, handle: u32) -> Result<(), E> {
        self.can_retain_hold(handle)?;
        if handle != 0 {
            let context = self.context_mut(handle)?;
            context.references += 1;
            context.holds += 1;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn check_release_holds(&self, handles: &[u32], clear_default: bool) -> Result<Vec<u32>, E> {
        let mut disposed = Vec::new();
        if !handles.is_empty() {
            self.next_revision()?;
        }
        for (index, &handle) in handles.iter().enumerate() {
            if handle == 0 || handles[..index].contains(&handle) {
                continue;
            }
            let context = self.context(handle)?;
            let count = handles
                .iter()
                .filter(|&&candidate| candidate == handle)
                .count()
                + usize::from(clear_default && self.default == handle);
            if count > context.holds as usize {
                return Err(E::InvalidHandle);
            }
            if count == context.references as usize {
                disposed.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                disposed.push(handle);
            }
        }
        Ok(disposed)
    }
    pub fn release_holds(&mut self, handles: &[u32], clear_default: bool) {
        assert!(
            handles.is_empty() || self.revision < u64::MAX,
            "activation hold retirement was preflighted"
        );
        for (index, &handle) in handles.iter().enumerate() {
            if handle == 0 || handles[..index].contains(&handle) {
                continue;
            }
            let count = handles
                .iter()
                .filter(|&&candidate| candidate == handle)
                .count() as u32
                + u32::from(clear_default && self.default == handle);
            let slot = self
                .contexts
                .iter()
                .position(|slot| {
                    slot.as_ref()
                        .is_some_and(|context| context.handle == handle)
                })
                .unwrap();
            let context = self.contexts[slot].as_mut().unwrap();
            context.references -= count;
            context.holds -= count;
            if context.references == 0 {
                self.contexts[slot] = None;
            }
            if clear_default && self.default == handle {
                self.default = 0;
            }
        }
        self.revision += u64::from(!handles.is_empty());
    }
}
