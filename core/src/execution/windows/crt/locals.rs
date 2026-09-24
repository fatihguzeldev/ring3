use std::collections::BTreeMap;

use super::super::thread::{BASE, Teb};
use super::{DispatchError, ERRNO, random};

struct Local {
    errno: u32,
    random: random::Sequence,
}

pub(super) struct Locals(BTreeMap<u32, Local>);

impl Default for Locals {
    fn default() -> Self {
        Self(BTreeMap::from([(
            BASE,
            Local {
                errno: ERRNO,
                random: random::Sequence::default(),
            },
        )]))
    }
}

impl Locals {
    pub(super) fn register(&mut self, teb: Teb) {
        // child pages are already zeroed; this is private crt storage, not a teb field.
        let errno = teb
            .0
            .checked_add(0x40)
            .expect("registered child page fits guest32");
        self.0.entry(teb.0).or_insert_with(|| Local {
            errno,
            random: random::Sequence::default(),
        });
    }

    pub(super) fn errno(&self, teb: u32) -> Result<u32, DispatchError> {
        self.0
            .get(&teb)
            .map(|local| local.errno)
            .ok_or(DispatchError::Unsupported)
    }

    pub(super) fn random(&mut self, teb: u32) -> Result<&mut random::Sequence, DispatchError> {
        self.0
            .get_mut(&teb)
            .map(|local| &mut local.random)
            .ok_or(DispatchError::Unsupported)
    }
}
