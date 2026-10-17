use super::*;
use crate::{
    memory::{GuestAddress, Permissions},
    process::{
        EngineInstance, GdiConfig32,
        windows::user32::{User32Outcome32, callback::Continuation32},
    },
    windows::user32::{
        Message32, User32Api32 as Api, User32Error32 as E,
        menus::{MenuHandle32, MenuItem32},
    },
};

fn fixture() -> (EngineInstance, u32, MenuHandle32, u32) {
    let (mut context, mut ram, call) = crate::process::windows::user32::tests::fixture();
    let atom = crate::process::windows::user32::tests::register(&mut context, &mut ram, call);
    let owner = crate::process::windows::user32::tests::create(&mut context, &mut ram, call, atom);
    context.install_popup_class(GuestAddress(0x70010)).unwrap();
    let menu = context.menus.create(true).unwrap();
    context
        .menus
        .insert(
            menu,
            0,
            MenuItem32::command(37, "&open".encode_utf16().collect()),
        )
        .unwrap();
    let mut engine = EngineInstance::new(32, 1).unwrap();
    engine.memory = Some(ram);
    engine.user32 = Some(context);
    engine
        .configure_gdi(GdiConfig32 {
            bitmap_base: 0x100000,
            bitmap_pages: 16,
            screen_width: 320,
            screen_height: 240,
            ..Default::default()
        })
        .unwrap();
    (engine, owner, menu, call.thread_id)
}

fn start(
    engine: &mut EngineInstance,
    owner: u32,
    menu: MenuHandle32,
    flags: u32,
    thread: u32,
) -> User32Outcome32 {
    engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            Api::TrackPopupMenu,
            &[menu.get(), flags, 30, 40, 0, owner, 0],
            engine.memory.as_mut().unwrap(),
            crate::process::windows::user32::User32CallContext32 {
                thread_id: thread,
                instance: 0x400000,
                ..Default::default()
            },
        )
        .unwrap()
}

#[test]
fn popup_native_notification_rows_post_commands_only_after_return_and_publish_real_pixels() {
    for (flags, cancel) in [
        (0, false),
        (0x100, false),
        (0x80, false),
        (0x180, false),
        (0, true),
        (0x100, true),
    ] {
        let (mut engine, owner, menu, thread) = fixture();
        let mut outcome = start(&mut engine, owner, menu, flags, thread);
        let mut events = Vec::new();
        let mut injected = false;
        let mut pixels = false;
        let mut returned = None;
        for _ in 0..256 {
            match outcome {
                User32Outcome32::Return(value) => {
                    returned = Some(value);
                    break;
                }
                User32Outcome32::Callback(callback) => {
                    assert!(
                        engine
                            .user32
                            .as_ref()
                            .unwrap()
                            .state
                            .find_message(thread, owner, 0x111, 0x111)
                            .unwrap()
                            .is_none(),
                        "WM_COMMAND is queued only after TrackPopupMenu returns"
                    );
                    if callback.arguments[0] == owner {
                        events.push(callback.arguments[1]);
                    }
                    outcome = engine
                        .user32
                        .as_mut()
                        .unwrap()
                        .complete_with_gdi(
                            callback.continuation,
                            u32::from(callback.arguments[1] == 0x81),
                            engine.memory.as_mut().unwrap(),
                            engine.gdi.as_mut(),
                        )
                        .unwrap();
                }
                User32Outcome32::Wait { continuation } => {
                    let context = engine.user32.as_mut().unwrap();
                    let plan =
                        &context.menu_popups.plans[context.popup_index(continuation).unwrap()];
                    if plan.stage == types::Stage32::Wait && !injected {
                        let popup = plan.windows[0].handle.unwrap();
                        assert_eq!(context.windows().capture(), Some(popup));
                        for index in 0..32 {
                            let Some(frame) = engine.gdi.as_ref().unwrap().frame(index) else {
                                break;
                            };
                            if frame.window == popup.get() && frame.window_dc {
                                let mut first = None;
                                for offset in (0..frame.bytes).step_by(4096) {
                                    let bytes = engine
                                        .gdi
                                        .as_ref()
                                        .unwrap()
                                        .read_frame(
                                            frame.id,
                                            frame.revision,
                                            offset,
                                            (frame.bytes - offset).min(4096),
                                        )
                                        .unwrap();
                                    let first = *first.get_or_insert_with(|| {
                                        <[u8; 4]>::try_from(&bytes[..4]).unwrap()
                                    });
                                    pixels |= bytes.chunks_exact(4).any(|pixel| pixel != first);
                                }
                            }
                        }
                        for key in if cancel {
                            &[0x1b][..]
                        } else {
                            &[0x28, 0x0d][..]
                        } {
                            context
                                .state
                                .post(Message32 {
                                    thread_id: thread,
                                    window: owner,
                                    message: 0x100,
                                    wparam: *key,
                                    ..Default::default()
                                })
                                .unwrap();
                        }
                        injected = true;
                    }
                    outcome = context
                        .resume_wait_with_gdi(
                            continuation,
                            engine.memory.as_mut().unwrap(),
                            engine.gdi.as_mut(),
                        )
                        .unwrap();
                }
            }
        }
        assert!(
            pixels,
            "a real popup surface contains its rendered border/text"
        );
        assert_eq!(
            returned,
            Some(if flags & 0x100 != 0 {
                if cancel { 0 } else { 37 }
            } else {
                1
            })
        );
        let expected = if flags & 0x80 != 0 {
            vec![0x11f, 0x11f]
        } else if cancel {
            vec![0x211, 0x116, 0x117, 0x125, 0x11f, 0x212]
        } else {
            vec![0x211, 0x116, 0x117, 0x11f, 0x125, 0x11f, 0x212]
        };
        assert_eq!(events, expected, "flags={flags:#x}, cancel={cancel}");
        let context = engine.user32.as_ref().unwrap();
        assert!(context.menu_popups.plans.is_empty());
        assert!(context.pending.is_empty());
        assert_eq!(context.windows().capture(), None);
        assert!(context.menus.contains(menu));
        let command = context
            .state
            .find_message(thread, owner, 0x111, 0x111)
            .unwrap();
        assert_eq!(
            command.map(|(_, message)| message.wparam),
            if !cancel && flags & 0x100 == 0 {
                Some(37)
            } else {
                None
            }
        );
    }
}

#[test]
fn replaced_popup_scratch_is_rejected_before_callbacks_windows_or_pixels_are_adopted() {
    let (mut engine, owner, menu, thread) = fixture();
    let User32Outcome32::Wait { continuation } = start(&mut engine, owner, menu, 0, thread) else {
        panic!("popup begins at a canonical wait");
    };
    let scratch = match engine
        .user32
        .as_ref()
        .unwrap()
        .pending
        .last()
        .unwrap()
        .continuation
    {
        Continuation32::MenuPopup { scratch, .. } => scratch,
        _ => unreachable!(),
    };
    let ram = engine.memory.as_mut().unwrap();
    ram.unmap(scratch.range()).unwrap();
    ram.map_zeroed(scratch.range(), Permissions::READ_WRITE)
        .unwrap();
    ram.write(scratch.address, &[0xa5; 64]).unwrap();
    let version = ram.transaction_version();
    let context = engine.user32.as_mut().unwrap();
    assert_eq!(
        context.resume_wait_with_gdi(continuation, ram, engine.gdi.as_mut()),
        Err(E::InvalidRequest)
    );
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(context.menu_popups.plans.len(), 1);
    assert!(context.menu_popups.plans[0].windows[0].handle.is_none());
    assert!(
        context
            .windows()
            .get(crate::windows::window::WindowHandle32::try_from(owner).unwrap())
            .is_ok()
    );
    assert!(engine.gdi.as_ref().unwrap().frame(0).is_none());
}

#[test]
fn ownerdraw_callbacks_supply_dimensions_and_use_the_canonical_popup_dc_for_real_pixels() {
    let (mut engine, owner, menu, thread) = fixture();
    let mut item = MenuItem32::command(37, Vec::new());
    item.kind = 0x100;
    item.data = 0x13572468;
    engine
        .user32
        .as_mut()
        .unwrap()
        .menus
        .replace(menu, 0, true, item)
        .unwrap();
    let mut outcome = start(&mut engine, owner, menu, 0x100, thread);
    let mut measures = 0;
    let mut draws = 0;
    let mut injected = false;
    let mut result = None;
    for _ in 0..256 {
        match outcome {
            User32Outcome32::Return(value) => {
                result = Some(value);
                break;
            }
            User32Outcome32::Callback(callback) => {
                if callback.arguments[0] == owner && callback.arguments[1] == 0x2c {
                    assert_eq!(callback.arguments[2], 0);
                    let words = crate::process::windows::user32::memory::words::<6>(
                        engine.memory.as_ref().unwrap(),
                        callback.arguments[3],
                    )
                    .unwrap();
                    assert_eq!(&words[..4], &[1, 0, 37, 0]);
                    assert_eq!(words[4], 16);
                    assert_eq!(words[5], 0x13572468);
                    engine
                        .memory
                        .as_mut()
                        .unwrap()
                        .write(
                            GuestAddress(callback.arguments[3] + 12),
                            &[53u32, 17]
                                .into_iter()
                                .flat_map(u32::to_le_bytes)
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                    measures += 1;
                } else if callback.arguments[0] == owner && callback.arguments[1] == 0x2b {
                    assert_eq!(callback.arguments[2], 0);
                    let words = crate::process::windows::user32::memory::words::<12>(
                        engine.memory.as_ref().unwrap(),
                        callback.arguments[3],
                    )
                    .unwrap();
                    assert_eq!(&words[..3], &[1, 0, 37]);
                    assert_eq!(words[3], if draws == 0 { 1 } else { 2 });
                    assert_eq!(words[4], if draws == 1 { 0x101 } else { 0x100 });
                    assert_eq!(words[5], menu.get());
                    assert_ne!(words[6], 0);
                    assert_eq!(words[9] - words[7], 67);
                    assert_eq!(words[10] - words[8], 17);
                    assert_eq!(words[11], 0x13572468);
                    let rect = crate::windows::user32::Rect32 {
                        left: words[7] as i32,
                        top: words[8] as i32,
                        right: words[9] as i32,
                        bottom: words[10] as i32,
                    };
                    let gdi = engine.gdi.as_mut().unwrap();
                    let mut canvas = gdi
                        .begin_canvas(engine.memory.as_ref().unwrap(), words[6])
                        .unwrap();
                    canvas.fill(gdi, rect, 0x00664422);
                    gdi.publish_canvas(engine.memory.as_mut().unwrap(), canvas)
                        .unwrap();
                    let popup = engine
                        .user32
                        .as_ref()
                        .unwrap()
                        .menu_popups
                        .plans
                        .last()
                        .unwrap()
                        .windows[0]
                        .handle
                        .unwrap();
                    let frame = (0..32)
                        .filter_map(|index| gdi.frame(index))
                        .find(|frame| frame.window == popup.get() && !frame.window_dc)
                        .unwrap();
                    assert_eq!(
                        gdi.read_frame(frame.id, frame.revision, 0, 4).unwrap(),
                        &[0x66, 0x44, 0x22, 0]
                    );
                    draws += 1;
                }
                outcome = engine
                    .user32
                    .as_mut()
                    .unwrap()
                    .complete_with_gdi(
                        callback.continuation,
                        u32::from(matches!(callback.arguments[1], 0x81 | 0x2c | 0x2b)),
                        engine.memory.as_mut().unwrap(),
                        engine.gdi.as_mut(),
                    )
                    .unwrap();
            }
            User32Outcome32::Wait { continuation } => {
                let context = engine.user32.as_mut().unwrap();
                if context.menu_popups.plans[context.popup_index(continuation).unwrap()].stage
                    == types::Stage32::Wait
                    && !injected
                {
                    for key in [0x28, 0x0d] {
                        context
                            .state
                            .post(Message32 {
                                thread_id: thread,
                                window: owner,
                                message: 0x100,
                                wparam: key,
                                ..Default::default()
                            })
                            .unwrap();
                    }
                    injected = true;
                }
                outcome = context
                    .resume_wait_with_gdi(
                        continuation,
                        engine.memory.as_mut().unwrap(),
                        engine.gdi.as_mut(),
                    )
                    .unwrap();
            }
        }
    }
    assert_eq!(result, Some(37));
    assert_eq!(
        measures, 1,
        "selection doesn't rerun an accepted measure callback"
    );
    assert_eq!(
        draws, 3,
        "initial, selected and final deselection invoke the real owner"
    );
    assert!(engine.user32.as_ref().unwrap().menu_popups.plans.is_empty());
}
