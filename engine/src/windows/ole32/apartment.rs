use super::HResult32;

#[derive(Clone, Copy)]
struct Apartment32 {
    thread: u32,
    single_threaded: bool,
    references: u32,
    internal: bool,
    contexts: [u8; 32],
    depth: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApartmentId32 {
    Sta(u32),
    Mta,
    Neutral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApartmentTarget32 {
    Current(ApartmentId32),
    Sta(u32),
    HostSta,
    HostMta,
}

pub struct Apartments32 {
    entries: Vec<Apartment32>,
    maximum: usize,
    main_sta: Option<u32>,
}
impl Apartments32 {
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
    pub(crate) fn mta_threads(&self) -> impl Iterator<Item = u32> + '_ {
        self.entries
            .iter()
            .filter(|entry| !entry.single_threaded && entry.references != 0)
            .map(|entry| entry.thread)
    }
    pub(crate) fn mark_host(&mut self, thread: u32) {
        self.entries
            .iter_mut()
            .find(|entry| entry.thread == thread)
            .expect("canonical host apartment was initialized")
            .internal = true;
    }
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
        self.check_initialize(thread, reserved, flags)?;
        let single_threaded = flags & 2 != 0;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.thread == thread) {
            entry.references += 1;
            return Ok(HResult32::FALSE);
        }
        self.entries.push(Apartment32 {
            thread,
            single_threaded,
            references: 1,
            internal: false,
            contexts: [0; 32],
            depth: 0,
        });
        if single_threaded && self.main_sta_thread().is_none() {
            self.main_sta = Some(thread);
        }
        Ok(HResult32::OK)
    }
    pub(crate) fn check_initialize(
        &self,
        thread: u32,
        reserved: u32,
        flags: u32,
    ) -> Result<(), HResult32> {
        if thread == 0 || reserved != 0 || flags & !14 != 0 {
            return Err(HResult32::INVALID_ARGUMENT);
        }
        if let Some(entry) = self.entries.iter().find(|entry| entry.thread == thread) {
            if self.identity(thread)? == ApartmentId32::Neutral
                || entry.single_threaded != (flags & 2 != 0)
            {
                return Err(HResult32::CHANGED_MODE);
            }
            entry
                .references
                .checked_add(1)
                .ok_or(HResult32::OUT_OF_MEMORY)?;
        } else if self.entries.len() == self.maximum {
            return Err(HResult32::OUT_OF_MEMORY);
        }
        Ok(())
    }
    pub(crate) fn identity(&self, thread: u32) -> Result<ApartmentId32, HResult32> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.thread == thread)
            .ok_or(HResult32::NOT_INITIALIZED)?;
        Ok(
            match entry
                .depth
                .checked_sub(1)
                .map(|index| entry.contexts[index])
            {
                Some(1) => ApartmentId32::Sta(thread),
                Some(2) => ApartmentId32::Mta,
                Some(3) => ApartmentId32::Neutral,
                None if entry.single_threaded => ApartmentId32::Sta(thread),
                None => ApartmentId32::Mta,
                _ => unreachable!(),
            },
        )
    }

    pub(crate) fn server_target(
        &self,
        thread: u32,
        model: crate::windows::com::ServerThreadingModel32,
    ) -> Result<ApartmentTarget32, HResult32> {
        use crate::windows::com::ServerThreadingModel32 as M;
        let apartment = self.identity(thread)?;
        Ok(match model {
            M::Both => ApartmentTarget32::Current(apartment),
            M::Neutral => ApartmentTarget32::Current(ApartmentId32::Neutral),
            M::Apartment => match apartment {
                ApartmentId32::Sta(_) => ApartmentTarget32::Current(apartment),
                ApartmentId32::Mta | ApartmentId32::Neutral => ApartmentTarget32::HostSta,
            },
            M::Free => match apartment {
                ApartmentId32::Mta => ApartmentTarget32::Current(apartment),
                ApartmentId32::Sta(_) | ApartmentId32::Neutral => ApartmentTarget32::HostMta,
            },
            M::MainSta => match self.main_sta_thread() {
                Some(main) if apartment == ApartmentId32::Sta(main) => {
                    ApartmentTarget32::Current(apartment)
                }
                Some(main) => ApartmentTarget32::Sta(main),
                None => ApartmentTarget32::HostSta,
            },
        })
    }
    pub(crate) fn check_context_entry(
        &self,
        thread: u32,
        apartment: ApartmentId32,
    ) -> Result<(), HResult32> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.thread == thread)
            .ok_or(HResult32::NOT_INITIALIZED)?;
        if matches!(apartment, ApartmentId32::Sta(owner) if owner != thread) {
            return Err(HResult32(0x8001_010e));
        }
        if entry.depth == entry.contexts.len() {
            return Err(HResult32::OUT_OF_MEMORY);
        }
        Ok(())
    }
    pub(crate) fn enter_context(&mut self, thread: u32, apartment: ApartmentId32) {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.thread == thread)
            .expect("invocation apartment was preflighted");
        entry.contexts[entry.depth] = match apartment {
            ApartmentId32::Sta(owner) => {
                assert_eq!(owner, thread);
                1
            }
            ApartmentId32::Mta => 2,
            ApartmentId32::Neutral => 3,
        };
        entry.depth += 1;
    }
    pub(crate) fn leave_context(&mut self, thread: u32, apartment: ApartmentId32) {
        assert_eq!(self.identity(thread), Ok(apartment));
        let index = self
            .entries
            .iter()
            .position(|entry| entry.thread == thread)
            .expect("invocation retains its apartment until return");
        self.entries[index].depth = self.entries[index]
            .depth
            .checked_sub(1)
            .expect("context exit matches an accepted entry");
        if self.entries[index].depth == 0 && self.entries[index].references == 0 {
            self.entries.swap_remove(index);
        }
    }
    pub fn uninitialize(&mut self, thread: u32) {
        if let Some(index) = self.entries.iter().position(|entry| entry.thread == thread) {
            self.entries[index].references = self.entries[index].references.saturating_sub(1);
            if self.entries[index].references == 0 && self.entries[index].depth == 0 {
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

    #[test]
    fn server_models_choose_apartments_without_confusing_threads_with_the_mta() {
        use crate::windows::com::ServerThreadingModel32 as M;
        use ApartmentId32 as I;
        use ApartmentTarget32 as T;
        let mut apartments = Apartments32::new(4).unwrap();
        apartments.initialize(7, 0, 2).unwrap();
        apartments.initialize(8, 0, 2).unwrap();
        apartments.initialize(9, 0, 0).unwrap();
        apartments.initialize(10, 0, 0).unwrap();
        assert_eq!(apartments.identity(9), apartments.identity(10));
        assert_ne!(apartments.identity(7), apartments.identity(8));
        for (caller, model, expected) in [
            (7, M::Apartment, T::Current(I::Sta(7))),
            (9, M::Apartment, T::HostSta),
            (7, M::Free, T::HostMta),
            (9, M::Free, T::Current(I::Mta)),
            (10, M::Both, T::Current(I::Mta)),
            (8, M::Both, T::Current(I::Sta(8))),
            (7, M::MainSta, T::Current(I::Sta(7))),
            (8, M::MainSta, T::Sta(7)),
            (9, M::MainSta, T::Sta(7)),
            (7, M::Neutral, T::Current(I::Neutral)),
            (9, M::Neutral, T::Current(I::Neutral)),
        ] {
            assert_eq!(apartments.server_target(caller, model), Ok(expected));
        }
        apartments.terminate_thread(7);
        assert_eq!(apartments.main_sta_thread(), None);
        assert_eq!(apartments.server_target(9, M::MainSta), Ok(T::HostSta));
    }

    #[test]
    fn neutral_calls_switch_context_without_changing_the_underlying_thread_model() {
        use crate::windows::com::ServerThreadingModel32 as M;
        let mut apartments = Apartments32::new(1).unwrap();
        apartments.initialize(7, 0, 2).unwrap();
        apartments
            .check_context_entry(7, ApartmentId32::Neutral)
            .unwrap();
        apartments.enter_context(7, ApartmentId32::Neutral);
        apartments
            .check_context_entry(7, ApartmentId32::Neutral)
            .unwrap();
        apartments.enter_context(7, ApartmentId32::Neutral);
        assert_eq!(apartments.identity(7), Ok(ApartmentId32::Neutral));
        assert_eq!(apartments.initialize(7, 0, 2), Err(HResult32::CHANGED_MODE));
        assert_eq!(apartments.initialize(7, 0, 0), Err(HResult32::CHANGED_MODE));
        assert_eq!(apartments.initialization_count(7), 1);
        assert_eq!(apartments.is_single_threaded(7), Ok(true));
        assert_eq!(
            apartments.server_target(7, M::Apartment),
            Ok(ApartmentTarget32::HostSta)
        );
        assert_eq!(
            apartments.server_target(7, M::Both),
            Ok(ApartmentTarget32::Current(ApartmentId32::Neutral))
        );
        apartments.leave_context(7, ApartmentId32::Neutral);
        assert_eq!(apartments.identity(7), Ok(ApartmentId32::Neutral));
        apartments.leave_context(7, ApartmentId32::Neutral);
        assert_eq!(apartments.identity(7), Ok(ApartmentId32::Sta(7)));
        assert_eq!(apartments.initialize(7, 0, 2), Ok(HResult32::FALSE));
    }
}
