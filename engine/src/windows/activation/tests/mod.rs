mod manifest;
mod policy;
mod scopes;
mod sections;
mod stack;

use super::*;
use crate::windows::activation::manifest::Manifest32;

const CLSID: [u8; 16] = [
    0x78, 0x56, 0x34, 0x12, 0xbc, 0x9a, 0xf0, 0xde, 1, 2, 3, 4, 5, 6, 7, 8,
];
fn manifest(name: &str, body: &str) -> Vec<u8> {
    format!(
        "<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'>\
         <assemblyIdentity name='{name}' version='1.2.3.4' type='win32' processorArchitecture='x86'/>\
         {body}</assembly>"
    )
    .into_bytes()
}
fn graph(name: &str) -> ActivationGraph32 {
    let bytes = manifest(
        name,
        "<file name='private.dll'><comClass clsid='{12345678-9abc-def0-0102-030405060708}' \
         threadingModel='Both' progid='Ring3.Component.1'/></file>",
    );
    ActivationGraph32::new(
        vec![ActivationAssembly32 {
            manifest: Manifest32::parse(&bytes, ActivationConfig32::default()).unwrap(),
            manifest_path: "C:\\app\\component.manifest".encode_utf16().collect(),
            directory: "C:\\app".encode_utf16().collect(),
            modified: 0,
            resource_name: None,
        }],
        ActivationConfig32::default(),
    )
    .unwrap()
}
fn create(contexts: &mut ActivationContexts32, name: &str, process_default: bool) -> u32 {
    let prepared = contexts
        .prepare_create(graph(name), process_default)
        .unwrap();
    contexts.commit_create(prepared)
}
fn activate(contexts: &mut ActivationContexts32, thread: u32, handle: u32) -> u32 {
    let prepared = contexts.prepare_activate(thread, handle).unwrap();
    contexts.commit_activate(prepared)
}
