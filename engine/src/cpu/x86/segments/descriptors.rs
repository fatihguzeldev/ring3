use super::{SegmentRegister, SegmentState};

/// Configured flat 32-bit descriptor kinds used by the software execution profile.
/// Limits, expand-down segments, call gates, and task switching are separate features.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentDescriptorKind {
    Data { writable: bool },
    Code { readable: bool, conforming: bool },
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentDescriptor {
    /// Selector index plus TI; RPL is ignored when installing or looking up an entry.
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
}

impl SegmentLoad {
    pub fn apply(self, state: &mut SegmentState, register: SegmentRegister) {
        state.selectors[register.index()] = self.selector;
        state.bases[register.index()] = self.base;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentDescriptorTable {
    entries: Vec<SegmentDescriptor>,
}

impl Default for SegmentDescriptorTable {
    fn default() -> Self {
        Self {
            entries: vec![
                SegmentDescriptor {
                    selector: 0x18,
                    base: 0,
                    dpl: 3,
                    present: true,
                    kind: SegmentDescriptorKind::Code {
                        readable: true,
                        conforming: false,
                    },
                },
                SegmentDescriptor {
                    selector: 0x20,
                    base: 0,
                    dpl: 3,
                    present: true,
                    kind: SegmentDescriptorKind::Data { writable: true },
                },
                SegmentDescriptor {
                    selector: 0x38,
                    base: 0,
                    dpl: 3,
                    present: true,
                    kind: SegmentDescriptorKind::Data { writable: true },
                },
            ],
        }
    }
}

impl SegmentDescriptorTable {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Configuration is explicit: unknown selectors are never synthesized from current state.
    pub fn configure(&mut self, mut descriptor: SegmentDescriptor) -> Result<(), SegmentLoadError> {
        descriptor.selector &= !3;
        if descriptor.selector == 0 || descriptor.dpl > 3 {
            return Err(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code: u32::from(descriptor.selector),
            });
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.selector == descriptor.selector)
        {
            *entry = descriptor;
        } else {
            self.entries.push(descriptor);
        }
        Ok(())
    }

    pub fn descriptor(&self, selector: u16) -> Option<&SegmentDescriptor> {
        self.entries
            .iter()
            .find(|entry| entry.selector == selector & !3)
    }

    /// Resolve all checks before either the selector or cached base is published.
    /// The caller maps the typed failure to the execution profile's exception record.
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
                base: if matches!(target, SegmentRegister::Fs | SegmentRegister::Gs) {
                    current.bases[target.index()]
                } else {
                    0
                },
            });
        }
        let descriptor = self
            .descriptor(selector)
            .ok_or_else(|| fail(SegmentLoadFailure::UnknownSelector))?;
        let rpl = (selector & 3) as u8;
        if target == SegmentRegister::Ss {
            if !matches!(
                descriptor.kind,
                SegmentDescriptorKind::Data { writable: true }
            ) {
                return Err(fail(SegmentLoadFailure::Type));
            }
            if rpl != cpl || descriptor.dpl != cpl {
                return Err(fail(SegmentLoadFailure::Privilege));
            }
        } else {
            let privilege_check = match descriptor.kind {
                SegmentDescriptorKind::Data { .. } => true,
                SegmentDescriptorKind::Code {
                    readable: true,
                    conforming,
                } => !conforming,
                _ => return Err(fail(SegmentLoadFailure::Type)),
            };
            if privilege_check && (rpl > descriptor.dpl || cpl > descriptor.dpl) {
                return Err(fail(SegmentLoadFailure::Privilege));
            }
        }
        if !descriptor.present {
            return Err(fail(SegmentLoadFailure::NotPresent));
        }
        if !matches!(target, SegmentRegister::Fs | SegmentRegister::Gs) && descriptor.base != 0 {
            return Err(fail(SegmentLoadFailure::NonFlatLegacy));
        }
        Ok(SegmentLoad {
            selector,
            base: descriptor.base,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
