use crate::{
    memory::{GuestAddress, PageRange},
    process::{
        EngineInstance, GdiConfig32,
        windows::user32::{User32CallContext32, User32Context32, User32Outcome32, memory},
    },
    windows::user32::{User32Api32 as Api, User32Limits32, menus::MenuItem32},
};

fn fixture() -> (EngineInstance, u32, u32, User32CallContext32) {
    let (_, mut ram, call) = crate::process::windows::user32::tests::fixture();
    let mut context = User32Context32::new(
        User32Limits32 {
            classes: 4,
            windows: 8,
            messages: 8,
            continuations: 32,
        },
        PageRange::new(GuestAddress(0x40000), 8).unwrap(),
    )
    .unwrap();
    let atom = crate::process::windows::user32::tests::register(&mut context, &mut ram, call);
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
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            Api::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0xcf0000,
                20,
                30,
                240,
                180,
                0,
                0,
                call.instance,
                0,
            ],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    let window = finish(
        &mut engine,
        outcome,
        call,
        &mut Vec::new(),
        &mut [0, 0],
        &mut 256,
    );
    assert_ne!(window, 0);
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            Api::ShowWindow,
            &[window, 1],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    finish(
        &mut engine,
        outcome,
        call,
        &mut Vec::new(),
        &mut [0, 0],
        &mut 256,
    );
    let context = engine.user32.as_mut().unwrap();
    let menu = context.menus.create(false).unwrap();
    let mut item = MenuItem32::command(37, Vec::new());
    item.kind = 0x100;
    item.data = 0x13572468;
    context.menus.insert(menu, 0, item).unwrap();
    (engine, window, menu.get(), call)
}

fn finish(
    engine: &mut EngineInstance,
    mut outcome: User32Outcome32,
    call: User32CallContext32,
    events: &mut Vec<u32>,
    counts: &mut [u32; 2],
    budget: &mut usize,
) -> u32 {
    loop {
        assert!(*budget != 0, "menu bar callbacks must terminate");
        *budget -= 1;
        outcome = match outcome {
            User32Outcome32::Return(value) => return value,
            User32Outcome32::Wait { continuation } => engine
                .user32
                .as_mut()
                .unwrap()
                .resume_wait_with_gdi(
                    continuation,
                    engine.memory.as_mut().unwrap(),
                    engine.gdi.as_mut(),
                )
                .unwrap(),
            User32Outcome32::Callback(callback) => {
                events.push(callback.arguments[1]);
                let result = if callback.arguments[1] == 0x2c {
                    let words =
                        memory::words::<6>(engine.memory.as_ref().unwrap(), callback.arguments[3])
                            .unwrap();
                    assert_eq!(words, [1, 0, 37, 0, 16, 0x13572468]);
                    engine
                        .memory
                        .as_mut()
                        .unwrap()
                        .write(
                            GuestAddress(callback.arguments[3] + 12),
                            &[53u32.to_le_bytes(), 17u32.to_le_bytes()].concat(),
                        )
                        .unwrap();
                    counts[0] += 1;
                    1
                } else if callback.arguments[1] == 0x2b {
                    let words =
                        memory::words::<12>(engine.memory.as_ref().unwrap(), callback.arguments[3])
                            .unwrap();
                    assert_eq!(
                        &words[..6],
                        &[
                            1,
                            0,
                            37,
                            1,
                            0x100,
                            engine
                                .user32
                                .as_ref()
                                .unwrap()
                                .state
                                .record(
                                    crate::windows::window::WindowHandle32::try_from(
                                        callback.arguments[0]
                                    )
                                    .unwrap()
                                )
                                .unwrap()
                                .menu
                        ]
                    );
                    assert_ne!(words[6], 0);
                    assert_eq!(&words[7..], &[8, 31, 75, 50, 0x13572468]);
                    let gdi = engine.gdi.as_mut().unwrap();
                    let mut canvas = gdi
                        .begin_canvas(engine.memory.as_ref().unwrap(), words[6])
                        .unwrap();
                    canvas.fill(
                        gdi,
                        crate::windows::user32::Rect32 {
                            left: 8,
                            top: 31,
                            right: 75,
                            bottom: 50,
                        },
                        0x664422,
                    );
                    gdi.publish_canvas(engine.memory.as_mut().unwrap(), canvas)
                        .unwrap();
                    let frame = (0..32)
                        .filter_map(|slot| gdi.frame(slot))
                        .find(|frame| frame.window == callback.arguments[0] && frame.window_dc)
                        .unwrap();
                    assert_eq!(
                        gdi.read_frame(frame.id, frame.revision, (31 * 240 + 8) * 4, 4)
                            .unwrap(),
                        &[0x66, 0x44, 0x22, 0]
                    );
                    counts[1] += 1;
                    1
                } else {
                    let nested = engine
                        .user32
                        .as_mut()
                        .unwrap()
                        .prepare_arguments(
                            Api::DefWindowProcA,
                            &callback.arguments,
                            engine.memory.as_mut().unwrap(),
                            call,
                        )
                        .unwrap();
                    finish(engine, nested, call, events, counts, budget)
                };
                engine
                    .user32
                    .as_mut()
                    .unwrap()
                    .complete_with_gdi(
                        callback.continuation,
                        result,
                        engine.memory.as_mut().unwrap(),
                        engine.gdi.as_mut(),
                    )
                    .unwrap()
            }
        };
    }
}

#[test]
fn assigning_redrawing_and_detaching_a_menu_preserve_real_nonclient_callbacks_pixels_and_client_geometry()
 {
    let (mut engine, window, menu, call) = fixture();
    let mut counts = [0, 0];
    let mut events = Vec::new();
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            Api::SetMenu,
            &[window, menu],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    assert_eq!(
        finish(
            &mut engine,
            outcome,
            call,
            &mut events,
            &mut counts,
            &mut 256
        ),
        1
    );
    assert_eq!(events, [0x46, 0x83, 0x2c, 0x85, 0x2b, 0x47, 3, 5]);
    let record = engine
        .user32
        .as_ref()
        .unwrap()
        .windows()
        .get(crate::windows::window::WindowHandle32::try_from(window).unwrap())
        .unwrap();
    assert_eq!(
        (
            record.description.extent.width,
            record.description.extent.height
        ),
        (224, 121)
    );
    let api = Api::from_id(0x200a4).expect("GetMenuItemRect is a real public menu API");
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .prepare_arguments(
                api,
                &[window, menu, 0, 0x10080],
                engine.memory.as_mut().unwrap(),
                call
            )
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        memory::words::<4>(engine.memory.as_ref().unwrap(), 0x10080).unwrap(),
        [28, 61, 95, 80]
    );
    events.clear();
    let api = Api::from_id(0x200a3).expect("DrawMenuBar is a real public menu API");
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(api, &[window], engine.memory.as_mut().unwrap(), call)
        .unwrap();
    assert_eq!(
        finish(
            &mut engine,
            outcome,
            call,
            &mut events,
            &mut counts,
            &mut 256
        ),
        1
    );
    assert_eq!(events, [0x46, 0x83, 0x85, 0x2b, 0x47]);
    assert_eq!(counts, [1, 2]);
    events.clear();
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            Api::SetMenu,
            &[window, 0],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    assert_eq!(
        finish(
            &mut engine,
            outcome,
            call,
            &mut events,
            &mut counts,
            &mut 256
        ),
        1
    );
    assert_eq!(events, [0x46, 0x83, 0x85, 0x47, 3, 5]);
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .menus
            .contains(crate::windows::user32::menus::MenuHandle32::try_from(menu).unwrap())
    );
    let record = engine
        .user32
        .as_ref()
        .unwrap()
        .windows()
        .get(crate::windows::window::WindowHandle32::try_from(window).unwrap())
        .unwrap();
    assert_eq!(record.description.extent.height, 141);
    assert!(engine.user32.as_ref().unwrap().pending.is_empty());
}
