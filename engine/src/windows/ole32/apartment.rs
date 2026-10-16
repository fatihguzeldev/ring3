use super::HResult32;

#[derive(Clone, Copy)]
struct Apartment32 {
    thread: u32,
    single_threaded: bool,
    references: u32,
    internal: bool,
}

pub struct Apartments32 {
    entries: Vec<Apartment32>,
    maximum: usize,
    main_sta: Option<u32>,
}
impl Apartments32 {
    pub(crate) fn has_other_user_mta(&self, thread: u32) -> bool {
        self.entries.iter().any(|entry| {
            entry.thread != thread
                && !entry.single_threaded
                && !entry.internal
                && entry.references != 0
        })
    }
    pub(crate) fn has_other_user(&self, thread: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.thread != thread && !entry.internal && entry.references != 0)
    }
    #[cfg(test)]
    pub(crate) fn mark_host(&mut self, thread: u32) {
        self.entries
            .iter_mut()
            .find(|entry| entry.thread == thread)
            .expect("canonical host apartment was initialized")
            .internal = true;
    }
    pub fn is_single_threaded(&self, thread: u32) -> Result<bool, HResult32> {
        self.entries
            .iter()
            .find(|entry| entry.thread == thread)
            .map(|entry| entry.single_threaded)
            .ok_or(HResult32::NOT_INITIALIZED)
    }
    pub fn main_sta_thread(&self) -> Option<u32> {
        self.main_sta
            .filter(|thread| self.entries.iter().any(|entry| entry.thread == *thread))
    }
    pub fn initialization_count(&self, thread: u32) -> u32 {
        self.entries
            .iter()
            .find(|entry| entry.thread == thread)
            .map_or(0, |entry| entry.references)
    }
    pub fn new(maximum: usize) -> Result<Self, HResult32> {
        if maximum == 0 || maximum > 65536 {
            return Err(HResult32::INVALID_ARGUMENT);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(maximum)
            .map_err(|_| HResult32::OUT_OF_MEMORY)?;
        Ok(Self {
            entries,
            maximum,
            main_sta: None,
        })
    }
    pub fn initialize(
        &mut self,
        thread: u32,
        reserved: u32,
        flags: u32,
    ) -> Result<HResult32, HResult32> {
        if thread == 0 || reserved != 0 || flags & !14 != 0 {
            return Err(HResult32::INVALID_ARGUMENT);
        }
        let single_threaded = flags & 2 != 0;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.thread == thread) {
            if entry.single_threaded != single_threaded {
                return Err(HResult32::CHANGED_MODE);
            }
            entry.references = entry
                .references
                .checked_add(1)
                .ok_or(HResult32::OUT_OF_MEMORY)?;
            return Ok(HResult32::FALSE);
        }
        if self.entries.len() == self.maximum {
            return Err(HResult32::OUT_OF_MEMORY);
        }
        self.entries.push(Apartment32 {
            thread,
            single_threaded,
            references: 1,
            internal: false,
        });
        if single_threaded && self.main_sta_thread().is_none() {
            self.main_sta = Some(thread);
        }
        Ok(HResult32::OK)
    }
    pub fn uninitialize(&mut self, thread: u32) {
        if let Some(index) = self.entries.iter().position(|entry| entry.thread == thread) {
            self.entries[index].references -= 1;
            if self.entries[index].references == 0 {
                self.entries.swap_remove(index);
            }
        }
    }
    pub fn check(&self, thread: u32) -> Result<(), HResult32> {
        self.entries
            .iter()
            .any(|entry| entry.thread == thread)
            .then_some(())
            .ok_or(HResult32::NOT_INITIALIZED)
    }
    pub fn terminate_thread(&mut self, thread: u32) {
        self.entries.retain(|entry| entry.thread != thread);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn balanced_initialization_preserves_thread_modes_and_failed_calls_are_neutral() {
        let mut apartments = Apartments32::new(2).unwrap();
        assert_eq!(apartments.initialize(7, 0, 2), Ok(HResult32(0)));
        assert_eq!(apartments.initialize(7, 0, 6), Ok(HResult32(1)));
        assert_eq!(apartments.initialize(7, 0, 0), Err(HResult32(0x8001_0106)));
        assert_eq!(apartments.initialize(8, 0, 0), Ok(HResult32(0)));
        assert_eq!(apartments.initialize(9, 0, 2), Err(HResult32(0x8007_000e)));
        apartments.uninitialize(7);
        assert_eq!(apartments.check(7), Ok(()));
        apartments.uninitialize(7);
        assert_eq!(apartments.check(7), Err(HResult32(0x8004_01f0)));
        assert_eq!(apartments.initialize(7, 0, 0), Ok(HResult32(0)));
        assert_eq!(apartments.initialize(7, 1, 0), Err(HResult32(0x8007_0057)));
        apartments.uninitialize(7);
        assert_eq!(apartments.check(7), Err(HResult32(0x8004_01f0)));
        assert_eq!(apartments.check(8), Ok(()));
    }
}
