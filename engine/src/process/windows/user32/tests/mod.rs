mod activation;
mod destruction;
mod input;
mod lifecycle;
mod messages;
mod runtime;
mod text;
mod threads;

use super::*;
use crate::{
    memory::{AddressSpace, PageRange, Permissions},
    windows::user32::{User32Api32, User32Error32, User32Limits32},
};

pub(in crate::process::windows::user32) fn fixture()
-> (User32Context32, AddressSpace, User32CallContext32) {
    let mut memory = AddressSpace::new(16).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10000), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let context = User32Context32::new(
        User32Limits32 {
            classes: 4,
            windows: 8,
            messages: 8,
            continuations: 4,
        },
        PageRange::new(GuestAddress(0x40000), 8).unwrap(),
    )
    .unwrap();
    (
        context,
        memory,
        User32CallContext32 {
            thread_id: 7,
            instance: 0x400000,
            ..Default::default()
        },
    )
}

pub(in crate::process::windows::user32) fn register(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
) -> u32 {
    memory
        .write(GuestAddress(0x10100), b"Ring3Window\0")
        .unwrap();
    let mut input = [0; 40];
    input[4..8].copy_from_slice(&0x70000u32.to_le_bytes());
    input[12..16].copy_from_slice(&8u32.to_le_bytes());
    input[16..20].copy_from_slice(&call.instance.to_le_bytes());
    input[36..40].copy_from_slice(&0x10100u32.to_le_bytes());
    memory.write(GuestAddress(0x10000), &input).unwrap();
    let User32Outcome32::Return(atom) = context
        .prepare_arguments(User32Api32::RegisterClassA, &[0x10000], memory, call)
        .unwrap()
    else {
        panic!("register must return")
    };
    assert!(atom >= 0xc000);
    atom
}

pub(in crate::process::windows::user32) fn create(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    atom: u32,
) -> u32 {
    let mut outcome = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x80000000,
                10,
                20,
                640,
                480,
                0,
                0,
                call.instance,
                0x1234,
            ],
            memory,
            call,
        )
        .unwrap();
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                let result = if callback.arguments[1] == 0x81 { 1 } else { 0 };
                outcome = context
                    .complete(callback.continuation, result, memory)
                    .unwrap();
            }
            User32Outcome32::Return(handle) => {
                assert_ne!(handle, 0);
                return handle;
            }
            _ => panic!("creation cannot wait"),
        }
    }
}

mod ui;
