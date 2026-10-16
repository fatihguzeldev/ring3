use super::Manifest32;
use crate::windows::{activation::ActivationError32 as E, text::ordinal_key};
use std::collections::BTreeSet;

pub(super) fn validate(manifest: &Manifest32) -> Result<(), E> {
    let mut files = BTreeSet::new();
    let mut classes = BTreeSet::new();
    let mut progids = BTreeSet::new();
    let mut interfaces = BTreeSet::new();
    let mut libraries = BTreeSet::new();
    let mut windows = BTreeSet::new();
    for file in &manifest.files {
        if !files.insert(key(&file.name)) {
            return Err(E::DuplicateDll);
        }
        for class in &file.classes {
            if !classes.insert(class.clsid) {
                return Err(E::DuplicateClass);
            }
            for progid in &class.progids {
                if progid.is_empty() || !progids.insert(key(progid)) {
                    return Err(E::DuplicateProgId);
                }
            }
        }
        for interface in &file.interfaces {
            if !interfaces.insert(interface.iid) {
                return Err(E::DuplicateInterface);
            }
        }
        for library in &file.type_libraries {
            if !libraries.insert((library.id, library.version)) {
                return Err(E::DuplicateTypeLibrary);
            }
        }
        for window in &file.window_classes {
            if !windows.insert(key(&window.name)) {
                return Err(E::DuplicateWindowClass);
            }
        }
    }
    for interface in &manifest.interfaces {
        if !interfaces.insert(interface.iid) {
            return Err(E::DuplicateInterface);
        }
    }
    Ok(())
}
fn key(value: &str) -> Vec<u16> {
    ordinal_key(&value.encode_utf16().collect::<Vec<_>>())
}
