use super::{SegmentCache, SegmentContext, SegmentDescriptorRecord, SegmentRegister, SegmentState};

/// compatibility view for explicitly configured code/data or legacy system descriptors.
/// raw records retain limits and every system/gate payload independently of this lossy view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentDescriptorKind {
    Data { writable: bool },
    Code { readable: bool, conforming: bool },
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentDescriptor {
    /// selector index plus ti; rpl is ignored when installing or looking up an entry.
    pub selector: u16,
    pub base: u32,
    pub dpl: u8,
    pub present: bool,
    pub kind: SegmentDescriptorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentLoadFailure {
    CodeRegister,
    NullStack,
    UnknownSelector,
    NotPresent,
    Privilege,
    Type,
    NonFlatLegacy,
    /// the descriptor is architectural code/data but needs an unimplemented execution profile.
    UnsupportedProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentLoadError {
    pub failure: SegmentLoadFailure,
    pub error_code: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentLoad {
    pub selector: u16,
    pub base: u32,
    pub cache: SegmentCache,
}

impl SegmentLoad {
    pub fn apply_context(self, context: &mut SegmentContext, register: SegmentRegister) {
        self.apply(&mut context.state, register);
        context.caches[register.index()] = self.cache;
    }

    pub fn apply(self, state: &mut SegmentState, register: SegmentRegister) {
        state.selectors[register.index()] = self.selector;
        state.bases[register.index()] = self.base;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DescriptorEntry {
    selector: u16,
    record: SegmentDescriptorRecord,
    legacy: Option<SegmentDescriptor>,
    legacy_constructed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentDescriptorTable {
    entries: Vec<DescriptorEntry>,
}

impl Default for SegmentDescriptorTable {
    fn default() -> Self {
        let mut table = Self::empty();
        for (selector, kind) in [
            (
                0x18,
                SegmentDescriptorKind::Code {
                    readable: true,
                    conforming: false,
                },
            ),
            (0x20, SegmentDescriptorKind::Data { writable: true }),
            (0x38, SegmentDescriptorKind::Data { writable: true }),
        ] {
            table
                .configure(SegmentDescriptor {
                    selector,
                    base: 0,
                    dpl: 3,
                    present: true,
                    kind,
                })
                .expect("default descriptors are valid legacy entries");
        }
        table
    }
}

impl SegmentDescriptorTable {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// legacy construction preserves the old selector/base/type observations, including the system variant.
    pub fn configure(&mut self, mut descriptor: SegmentDescriptor) -> Result<(), SegmentLoadError> {
        descriptor.selector &= !3;
        let record = SegmentDescriptorRecord::from_legacy(descriptor)
            .filter(|_| descriptor.selector != 0)
            .ok_or(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code: u32::from(descriptor.selector),
            })?;
        self.install(DescriptorEntry {
            selector: descriptor.selector,
            record,
            legacy: Some(descriptor),
            legacy_constructed: true,
        });
        Ok(())
    }

    /// table authority is lossless; configuration never rewrites a previously loaded cache.
    pub fn configure_raw(
        &mut self,
        selector: u16,
        record: SegmentDescriptorRecord,
    ) -> Result<(), SegmentLoadError> {
        let selector = selector & !3;
        if selector == 0 {
            return Err(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code: 0,
            });
        }
        self.install(DescriptorEntry {
            selector,
            record,
            legacy: record.legacy_code_data(selector),
            legacy_constructed: false,
        });
        Ok(())
    }

    fn install(&mut self, entry: DescriptorEntry) {
        if let Some(current) = self
            .entries
            .iter_mut()
            .find(|current| current.selector == entry.selector)
        {
            *current = entry;
        } else {
            self.entries.push(entry);
        }
    }

    fn entry(&self, selector: u16) -> Option<&DescriptorEntry> {
        self.entries
            .iter()
            .find(|entry| entry.selector == selector & !3)
    }

    /// raw system/gate descriptors have no invented base or legacy kind projection.
    pub fn descriptor(&self, selector: u16) -> Option<&SegmentDescriptor> {
        self.entry(selector)?.legacy.as_ref()
    }

    pub fn raw_descriptor(&self, selector: u16) -> Option<&SegmentDescriptorRecord> {
        self.entry(selector).map(|entry| &entry.record)
    }

    /// legacy system base updates remain explicit constructor policy; raw gates are refused.
    pub(crate) fn update_base(&mut self, selector: u16, base: u32) -> Result<(), SegmentLoadError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.selector == selector & !3)
            .ok_or(SegmentLoadError {
                failure: SegmentLoadFailure::UnknownSelector,
                error_code: u32::from(selector & !3),
            })?;
        let record = if let Some(record) = entry.record.with_code_data_base(base) {
            record
        } else if entry.legacy_constructed {
            let mut legacy = entry
                .legacy
                .expect("legacy constructors retain their compatibility view");
            legacy.base = base;
            SegmentDescriptorRecord::from_legacy(legacy).expect("stored legacy dpl is valid")
        } else {
            return Err(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code: u32::from(selector & !3),
            });
        };
        entry.record = record;
        if let Some(legacy) = &mut entry.legacy {
            legacy.base = base;
        }
        Ok(())
    }

    /// commit only after all fallible load staging has succeeded; verification never calls this.
    pub(crate) fn commit_accessed(&mut self, selector: u16) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.selector == selector & !3)
        {
            if let Some(record) = entry.record.with_accessed() {
                entry.record = record;
            }
        }
    }

    /// query raw type and visibility without loading a segment or testing its present bit.
    pub fn verify(&self, selector: u16, cpl: u8, write: bool) -> bool {
        if selector & !3 == 0 || cpl > 3 {
            return false;
        }
        let Some(record) = self.raw_descriptor(selector).copied() else {
            return false;
        };
        if !record.code_data() {
            return false;
        }
        let kind = record.descriptor_type();
        let code = kind & 8 != 0;
        let permitted = if write {
            !code && kind & 2 != 0
        } else {
            !code || kind & 2 != 0
        };
        permitted
            && ((code && kind & 4 != 0)
                || (cpl <= record.dpl() && (selector & 3) as u8 <= record.dpl()))
    }

    /// resolve architecture and profile constraints without modifying the table or loaded state.
    pub fn resolve_load(
        &self,
        target: SegmentRegister,
        selector: u16,
        cpl: u8,
        current: &SegmentState,
    ) -> Result<SegmentLoad, SegmentLoadError> {
        let fail = |failure| SegmentLoadError {
            failure,
            error_code: u32::from(selector & !3),
        };
        if target == SegmentRegister::Cs {
            return Err(fail(SegmentLoadFailure::CodeRegister));
        }
        if cpl > 3 {
            return Err(fail(SegmentLoadFailure::Privilege));
        }
        if selector & !3 == 0 {
            if target == SegmentRegister::Ss {
                return Err(fail(SegmentLoadFailure::NullStack));
            }
            return Ok(SegmentLoad {
                selector,
                cache: SegmentCache::UNUSABLE,
                base: if matches!(target, SegmentRegister::Fs | SegmentRegister::Gs) {
                    current.bases[target.index()]
                } else {
                    0
                },
            });
        }
        let entry = self
            .entry(selector)
            .ok_or_else(|| fail(SegmentLoadFailure::UnknownSelector))?;
        let record = entry.record;
        if !record.code_data() {
            return Err(fail(SegmentLoadFailure::Type));
        }
        let kind = record.descriptor_type();
        let code = kind & 8 != 0;
        let rpl = (selector & 3) as u8;
        if target == SegmentRegister::Ss {
            if code || kind & 2 == 0 {
                return Err(fail(SegmentLoadFailure::Type));
            }
            if rpl != cpl || record.dpl() != cpl {
                return Err(fail(SegmentLoadFailure::Privilege));
            }
        } else {
            if code && kind & 2 == 0 {
                return Err(fail(SegmentLoadFailure::Type));
            }
            if !(code && kind & 4 != 0) && (rpl > record.dpl() || cpl > record.dpl()) {
                return Err(fail(SegmentLoadFailure::Privilege));
            }
        }
        if !record.present() {
            return Err(fail(SegmentLoadFailure::NotPresent));
        }
        let base = record.code_data_base().expect("code/data checked above");
        if !matches!(target, SegmentRegister::Fs | SegmentRegister::Gs) && base != 0 {
            return Err(fail(if entry.legacy_constructed {
                SegmentLoadFailure::NonFlatLegacy
            } else {
                SegmentLoadFailure::UnsupportedProfile
            }));
        }
        if record.long_mode() || (target == SegmentRegister::Ss && !record.default32()) {
            return Err(fail(SegmentLoadFailure::UnsupportedProfile));
        }
        Ok(SegmentLoad {
            selector,
            base,
            cache: record
                .loaded_cache()
                .expect("present non-long code/data is cacheable"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_queries_type_and_visibility_without_testing_presence_or_loading() {
        let mut table = SegmentDescriptorTable::empty();
        let rows = [
            (
                SegmentDescriptorKind::Data { writable: false },
                true,
                false,
                false,
            ),
            (
                SegmentDescriptorKind::Data { writable: true },
                true,
                true,
                false,
            ),
            (
                SegmentDescriptorKind::Code {
                    readable: false,
                    conforming: false,
                },
                false,
                false,
                false,
            ),
            (
                SegmentDescriptorKind::Code {
                    readable: true,
                    conforming: false,
                },
                true,
                false,
                false,
            ),
            (
                SegmentDescriptorKind::Code {
                    readable: false,
                    conforming: true,
                },
                false,
                false,
                true,
            ),
            (
                SegmentDescriptorKind::Code {
                    readable: true,
                    conforming: true,
                },
                true,
                false,
                true,
            ),
            (SegmentDescriptorKind::System, false, false, false),
        ];
        for (kind, read, write, conforming) in rows {
            for present in [false, true] {
                for dpl in 0..4 {
                    for selector in [0x40, 0x44] {
                        table
                            .configure(SegmentDescriptor {
                                selector,
                                base: 0x1234_0000,
                                dpl,
                                present,
                                kind,
                            })
                            .unwrap();
                        let before = table.clone();
                        for cpl in 0..4 {
                            for rpl in 0..4 {
                                let visible = conforming || cpl.max(rpl) <= dpl;
                                assert_eq!(
                                    table.verify(selector | u16::from(rpl), cpl, false),
                                    read && visible
                                );
                                assert_eq!(
                                    table.verify(selector | u16::from(rpl), cpl, true),
                                    write && visible
                                );
                            }
                        }
                        assert_eq!(table, before);
                    }
                }
            }
        }
        for selector in [0, 1, 2, 3, 0x48, 0x4c] {
            assert!(!table.verify(selector, 3, false));
            assert!(!table.verify(selector, 3, true));
        }
        assert!(!table.verify(0x43, 4, false));
        table
            .configure(SegmentDescriptor {
                selector: 0x44,
                base: 0,
                dpl: 3,
                present: false,
                kind: SegmentDescriptorKind::Data { writable: true },
            })
            .unwrap();
        assert!(table.verify(0x47, 3, true));
        assert!(!table.verify(7, 3, true));
    }

    #[test]
    fn configured_tls_reload_and_null_cache_are_explicit() {
        let mut table = SegmentDescriptorTable::default();
        table
            .configure(SegmentDescriptor {
                selector: 0x3b,
                base: 0x7654_0000,
                dpl: 3,
                present: true,
                kind: SegmentDescriptorKind::Data { writable: true },
            })
            .unwrap();
        let mut state = SegmentState::default();
        table
            .resolve_load(SegmentRegister::Fs, 0x3b, 3, &state)
            .unwrap()
            .apply(&mut state, SegmentRegister::Fs);
        assert_eq!(state.bases[4], 0x7654_0000);
        table
            .resolve_load(SegmentRegister::Fs, 0, 3, &state)
            .unwrap()
            .apply(&mut state, SegmentRegister::Fs);
        assert_eq!((state.selectors[4], state.bases[4]), (0, 0x7654_0000));
        assert_eq!(
            table
                .resolve_load(SegmentRegister::Gs, 0x43, 3, &state)
                .unwrap_err()
                .failure,
            SegmentLoadFailure::UnknownSelector
        );
    }

    #[test]
    fn failures_publish_nothing_and_preserve_selector_error_code() {
        let mut table = SegmentDescriptorTable::empty();
        let state = SegmentState::default();
        for (kind, dpl, present, target, expected) in [
            (
                SegmentDescriptorKind::Data { writable: true },
                0,
                true,
                SegmentRegister::Fs,
                SegmentLoadFailure::Privilege,
            ),
            (
                SegmentDescriptorKind::Data { writable: true },
                3,
                false,
                SegmentRegister::Fs,
                SegmentLoadFailure::NotPresent,
            ),
            (
                SegmentDescriptorKind::Data { writable: false },
                3,
                true,
                SegmentRegister::Ss,
                SegmentLoadFailure::Type,
            ),
            (
                SegmentDescriptorKind::Code {
                    readable: false,
                    conforming: false,
                },
                3,
                true,
                SegmentRegister::Gs,
                SegmentLoadFailure::Type,
            ),
            (
                SegmentDescriptorKind::System,
                3,
                true,
                SegmentRegister::Ds,
                SegmentLoadFailure::Type,
            ),
        ] {
            table
                .configure(SegmentDescriptor {
                    selector: 0x47,
                    base: 0,
                    dpl,
                    present,
                    kind,
                })
                .unwrap();
            let error = table.resolve_load(target, 0x47, 3, &state).unwrap_err();
            assert_eq!(
                error,
                SegmentLoadError {
                    failure: expected,
                    error_code: 0x44
                }
            );
            assert_eq!(state, SegmentState::default());
        }
        assert_eq!(
            table
                .resolve_load(SegmentRegister::Ss, 0, 3, &state)
                .unwrap_err()
                .failure,
            SegmentLoadFailure::NullStack
        );
    }

    #[test]
    fn table_indicator_rpl_and_flat_legacy_constraints_are_distinct() {
        let mut table = SegmentDescriptorTable::empty();
        table
            .configure(SegmentDescriptor {
                selector: 4,
                base: 0x5000,
                dpl: 3,
                present: true,
                kind: SegmentDescriptorKind::Data { writable: true },
            })
            .unwrap();
        let state = SegmentState::default();
        assert_eq!(
            table
                .resolve_load(SegmentRegister::Gs, 7, 3, &state)
                .unwrap()
                .base,
            0x5000
        );
        assert_eq!(
            table
                .resolve_load(SegmentRegister::Ds, 7, 3, &state)
                .unwrap_err()
                .failure,
            SegmentLoadFailure::NonFlatLegacy
        );
        assert_eq!(
            table
                .resolve_load(SegmentRegister::Ss, 4, 3, &state)
                .unwrap_err()
                .failure,
            SegmentLoadFailure::Privilege
        );
    }
}

#[cfg(test)]
#[path = "descriptors/raw/tests.rs"]
mod raw_tests;
