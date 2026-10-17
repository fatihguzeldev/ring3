use super::*;
use crate::{
    process::windows::user32::{User32CallContext32, User32Outcome32},
    windows::user32::User32Api32 as Api,
};

fn window(engine: &mut EngineInstance, thread: u32) -> u32 {
    let call = User32CallContext32 {
        thread_id: thread,
        instance: 0x400000 + thread * 0x1000,
        ..Default::default()
    };
    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    let atom = crate::process::windows::user32::tests::register(context, memory, call);
    crate::process::windows::user32::tests::create(context, memory, call, atom)
}

#[test]
fn destroying_a_window_retires_the_current_attached_tree_and_preserves_the_replaced_menu() {
    let mut engine = engine();
    let window = window(&mut engine, 1);
    let replaced = call(&mut engine, Api::CreateMenu.id(), &[]).unwrap();
    let current = call(&mut engine, Api::CreateMenu.id(), &[]).unwrap();
    let submenu = call(&mut engine, Api::CreatePopupMenu.id(), &[]).unwrap();
    call(
        &mut engine,
        Api::AppendMenuW.id(),
        &[current, 0x10, submenu, 0],
    )
    .unwrap();
    call(&mut engine, Api::SetMenu.id(), &[window, replaced]).unwrap();
    call(&mut engine, Api::SetMenu.id(), &[window, current]).unwrap();
    let context = engine.user32.as_mut().unwrap();
    let ram = engine.memory.as_mut().unwrap();
    let mut outcome = context
        .prepare_arguments(
            Api::DestroyWindow,
            &[window],
            ram,
            User32CallContext32 {
                thread_id: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let mut messages = Vec::new();
    for _ in 0..16 {
        match outcome {
            User32Outcome32::Callback(callback) => {
                messages.push(callback.arguments[1]);
                outcome = context.complete(callback.continuation, 0, ram).unwrap();
            }
            User32Outcome32::Return(value) => {
                assert_eq!(value, 1);
                break;
            }
            other => panic!("unexpected window destruction outcome: {other:?}"),
        }
    }
    assert!(messages.contains(&2));
    assert!(messages.contains(&0x82));
    assert_eq!(call(&mut engine, Api::IsMenu.id(), &[current]), Ok(0));
    assert_eq!(call(&mut engine, Api::IsMenu.id(), &[submenu]), Ok(0));
    assert_eq!(call(&mut engine, Api::IsMenu.id(), &[replaced]), Ok(1));
}

#[test]
fn forced_thread_retirement_only_retires_that_threads_current_attached_menu_union() {
    let mut engine = engine();
    let first = window(&mut engine, 1);
    let second = window(&mut engine, 2);
    let first_menu = call(&mut engine, Api::CreateMenu.id(), &[]).unwrap();
    let second_menu = call(&mut engine, Api::CreateMenu.id(), &[]).unwrap();
    call(&mut engine, Api::SetMenu.id(), &[first, first_menu]).unwrap();
    call(&mut engine, Api::SetMenu.id(), &[second, second_menu]).unwrap();
    let context = engine.user32.as_mut().unwrap();
    let prepared = context.prepare_thread_termination(1).unwrap();
    assert!(
        context
            .windows()
            .get(crate::windows::window::WindowHandle32::try_from(first).unwrap())
            .is_ok()
    );
    context.commit_thread_termination(prepared);
    assert_eq!(call(&mut engine, Api::IsMenu.id(), &[first_menu]), Ok(0));
    assert_eq!(call(&mut engine, Api::IsMenu.id(), &[second_menu]), Ok(1));
    assert!(
        engine
            .user32_windows()
            .unwrap()
            .get(crate::windows::window::WindowHandle32::try_from(second).unwrap())
            .is_ok()
    );
}
