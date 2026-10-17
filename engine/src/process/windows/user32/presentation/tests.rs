use super::*;
use crate::process::windows::user32::{
    User32Outcome32,
    tests::{create, fixture, register},
};
use crate::windows::user32::User32Api32;

fn engine() -> (EngineInstance, [u32; 3]) {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let windows = std::array::from_fn(|_| create(&mut context, &mut memory, call, atom));
    let mut engine = EngineInstance::new(16, 42).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    (engine, windows)
}

#[test]
fn snapshot_projects_canonical_geometry_and_distinguishes_child_parent_from_owner() {
    let (mut engine, [root, owned, child]) = engine();
    let context = engine.user32.as_mut().unwrap();
    let root = WindowHandle32::try_from(root).unwrap();
    let mut description = context.windows().get(root).unwrap().description;
    description.visible = false;
    context
        .state
        .windows_mut()
        .update_description(root, description)
        .unwrap();
    for (raw, style) in [(owned, 0x9000_0000), (child, 0x5000_0000)] {
        let handle = WindowHandle32::try_from(raw).unwrap();
        let mut data = context.state.record(handle).unwrap();
        data.style = style;
        data.ex_style = 0x1234;
        data.parent = Some(root);
        data.outer = Rect32 {
            left: -200,
            top: -100,
            right: 200,
            bottom: 200,
        };
        data.client_origin = Point32 { x: -190, y: -80 };
        context.state.update(data).unwrap();
        let mut description = context.windows().get(handle).unwrap().description;
        description.extent = WindowExtent32 {
            width: 380,
            height: 270,
        };
        description.visible = true;
        description.enabled = raw == owned;
        context
            .state
            .windows_mut()
            .update_description(handle, description)
            .unwrap();
    }
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    let owned = engine.user32_window_info(owned).unwrap().unwrap();
    assert_eq!(
        (owned.parent, owned.owner, owned.thread),
        (0, root.get(), 7)
    );
    assert!(
        owned.visible && owned.enabled,
        "a top-level owner does not hide its owned window"
    );
    let child = engine.user32_window_info(child).unwrap().unwrap();
    assert_eq!(
        (child.parent, child.owner, child.thread),
        (root.get(), 0, 7)
    );
    assert!(!child.visible && !child.enabled);
    assert_eq!(child.style, 0x5000_0000);
    assert_eq!(child.ex_style, 0x1234);
    assert_eq!(child.outer, owned.outer);
    assert_eq!(child.client_origin, Point32 { x: -190, y: -80 });
    assert_eq!(
        child.client_extent,
        WindowExtent32 {
            width: 380,
            height: 270
        }
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}

#[test]
fn absent_destroying_and_removed_queries_preserve_cpu_and_last_error() {
    let (mut engine, [window, _, _]) = engine();
    let thread = engine
        .current_windows_thread()
        .unwrap()
        .with_last_error(0x4455);
    engine.publish_windows_thread(thread).unwrap();
    engine.user32.as_mut().unwrap().last_error = Some(0x6677);
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    for missing in [0, u32::MAX] {
        assert_eq!(engine.user32_window_info(missing).unwrap(), None);
    }
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);

    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    let mut outcome = context
        .prepare_arguments(
            User32Api32::DestroyWindow,
            &[window],
            memory,
            super::super::User32CallContext32 {
                thread_id: 7,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        context.window_info(window).unwrap(),
        None,
        "destroying windows cannot be presented"
    );
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                outcome = context.complete(callback.continuation, 0, memory).unwrap();
            }
            User32Outcome32::Return(result) => {
                assert_eq!(result, 1);
                break;
            }
            _ => panic!("destroy must complete without a provider wait"),
        }
    }
    engine.user32.as_mut().unwrap().last_error = Some(0x6677);
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(engine.user32_window_info(window).unwrap(), None);
    assert_eq!(
        engine.current_windows_thread().unwrap().last_error(),
        0x4455
    );
    assert_eq!(engine.user32.as_ref().unwrap().last_error, Some(0x6677));
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}

#[test]
fn unconfigured_and_closed_queries_report_the_existing_owner_error_without_mutation() {
    let mut engine = EngineInstance::new(1, 42).unwrap();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.user32_window_info(0),
        Err(HostError::User32(User32Error32::InvalidRequest))
    );
    assert_eq!(engine.arena(), arena);
    engine.close();
    let arena = engine.arena().to_vec();
    assert_eq!(engine.user32_window_info(0), Err(HostError::Closed));
    assert_eq!(engine.arena(), arena);
}
