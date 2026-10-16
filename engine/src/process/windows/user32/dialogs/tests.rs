use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::user32::{User32Outcome32, tests::fixture},
    process::{EngineInstance, HostError},
    windows::user32::{User32Api32 as Api, User32Error32 as E},
};
#[test]
fn indirect_dialog_freezes_unicode_and_waits_for_exact_host_response_without_inventing_idok() {
    let (context, mut memory, call) = fixture();
    let text = "日本"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    memory.write(GuestAddress(0x10100), &text).unwrap();
    let params = [40, 0, 0x400000, 0x10100, 0, 3, 0, 0, 0, 0x411]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    memory.write(GuestAddress(0x10000), &params).unwrap();
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    let dialog = engine
        .prepare_user32_dialog(Api::MessageBoxIndirectW, &[0x10000], call)
        .unwrap();
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .begin_dialog(dialog)
        .unwrap();
    let User32Outcome32::Wait { continuation } = outcome else {
        panic!("host response must be pending")
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    let (epoch, id) = (request.epoch, request.id);
    assert_eq!(request.text, "日本".encode_utf16().collect::<Vec<_>>());
    assert_eq!(request.caption, "Error".encode_utf16().collect::<Vec<_>>());
    assert_eq!(request.language, 0x411);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10100), &[0, 0])
        .unwrap();
    assert_eq!(
        engine.user32_dialog_request(0).unwrap().unwrap().text,
        "日本".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        engine.complete_user32_dialog(epoch + 1, id, 6, 0),
        Err(HostError::User32(E::InvalidToken))
    );
    assert_eq!(
        engine.complete_user32_dialog(epoch, id, 1, 0),
        Err(HostError::User32(E::InvalidRequest))
    );
    assert!(!engine.user32.as_ref().unwrap().active_thread_wait_ready());
    engine.complete_user32_dialog(epoch, id, 6, 0).unwrap();
    assert!(engine.user32.as_ref().unwrap().active_thread_wait_ready());
    assert_eq!(
        engine.complete_user32_dialog(epoch, id, 6, 0),
        Err(HostError::User32(E::InvalidRequest))
    );
    let context = engine.user32.as_mut().unwrap();
    assert_eq!(
        context
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(6)
    );
    assert!(engine.user32_dialog_request(0).unwrap().is_none());
}
#[test]
fn indirect_parameters_and_help_callback_are_preflighted_and_void_help_return_keeps_dialog_pending()
{
    let (context, mut memory, call) = fixture();
    memory
        .protect(
            PageRange::new(GuestAddress(0x10000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let params = [40, 0, 0x400000, 0, 0, 0x4001, 0, 0xabcdef, 0x10100, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    memory.write(GuestAddress(0x10000), &params).unwrap();
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    assert!(
        engine
            .prepare_user32_dialog(Api::MessageBoxIndirectW, &[0xfffffff0], call)
            .is_err()
    );
    assert!(engine.user32.as_ref().unwrap().dialogs.is_empty());
    let dialog = engine
        .prepare_user32_dialog(Api::MessageBoxIndirectW, &[0x10000], call)
        .unwrap();
    let User32Outcome32::Wait { continuation } = engine
        .user32
        .as_mut()
        .unwrap()
        .begin_dialog(dialog)
        .unwrap()
    else {
        panic!()
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    let (epoch, id) = (request.epoch, request.id);
    engine.request_user32_dialog_help(epoch, id).unwrap();
    let User32Outcome32::Callback(callback) = engine
        .user32
        .as_mut()
        .unwrap()
        .resume_wait(continuation, engine.memory.as_mut().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(callback.count, 1);
    assert_eq!(callback.procedure.0, 0x10100);
    let mut help = [0; 28];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(callback.arguments[0]), &mut help)
        .unwrap();
    assert_eq!(u32::from_le_bytes(help[..4].try_into().unwrap()), 28);
    assert_eq!(
        u32::from_le_bytes(help[16..20].try_into().unwrap()),
        0xabcdef
    );
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete(continuation, u32::MAX, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Wait { continuation }
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(callback.arguments[0]))
            .is_err()
    );
    engine.complete_user32_dialog(epoch, id, 2, 0).unwrap();
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(2)
    );
}

#[test]
fn remote_help_result_wakes_only_the_saved_sender_and_releases_its_exact_loan() {
    use crate::process::windows::user32::{callback::Continuation32, memory::Scratch32};
    let (context, mut memory, call) = fixture();
    let mut engine = EngineInstance::new(32, 7).unwrap();
    let params = [40, 0, 0, 0, 0, 0x4001, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    memory.write(GuestAddress(0x10000), &params).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    let dialog = engine
        .prepare_user32_dialog(Api::MessageBoxIndirectW, &[0x10000], call)
        .unwrap();
    let context = engine.user32.as_mut().unwrap();
    let User32Outcome32::Wait { continuation } = context.begin_dialog(dialog).unwrap() else {
        panic!("dialog must await its explicit host response")
    };
    let id = context.dialogs[0].id;
    let scratch = Scratch32::allocate(
        engine.memory.as_mut().unwrap(),
        context.scratch_bounds,
        &[0; 28],
    )
    .unwrap();
    context.pending.last_mut().unwrap().continuation = Continuation32::DialogRemoteHelp {
        id,
        scratch,
        ready: None,
    };
    let child = context.empty_thread_reply();
    let mut sender = context.take_thread_reply();
    context.restore_thread_reply(child);
    assert!(!context.thread_reply_ready(&sender));
    assert_eq!(
        sender.scratch_ranges().collect::<Vec<_>>(),
        [scratch.range()]
    );
    assert_eq!(
        sender.check_remote_result(continuation + 1),
        Err(E::InvalidToken)
    );
    sender.accept_remote_result(continuation, u32::MAX).unwrap();
    assert!(context.thread_reply_ready(&sender));
    assert_eq!(
        sender.accept_remote_result(continuation, 0),
        Err(E::InvalidToken)
    );
    context.take_thread_reply();
    context.restore_thread_reply(sender);
    assert_eq!(
        context
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Wait { continuation }
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(scratch.address)
            .is_err()
    );
    assert!(!engine.user32.as_ref().unwrap().active_thread_wait_ready());
}

#[test]
fn ownerless_application_modal_dialog_does_not_acquire_task_modal_input_scope() {
    use crate::process::windows::user32::tests::{create, register};

    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let second = create(&mut context, &mut memory, call, atom);
    let mut other = call;
    other.thread_id += 1;
    let foreign = create(&mut context, &mut memory, other, atom);
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);

    for (style, blocked) in [(0, false), (0x1000, false), (0x2000, true)] {
        let dialog = engine
            .prepare_user32_dialog(Api::MessageBoxW, &[0, 0, 0, style], call)
            .unwrap();
        let User32Outcome32::Wait { continuation } = engine
            .user32
            .as_mut()
            .unwrap()
            .begin_dialog(dialog)
            .unwrap()
        else {
            panic!("dialog must await a real host response")
        };
        let context = engine.user32.as_ref().unwrap();
        for window in [first, second] {
            assert_eq!(
                context.dialog_blocks_window(window),
                blocked,
                "owner=NULL style {style:#x} must preserve its exact modality"
            );
        }
        assert!(
            !context.dialog_blocks_window(foreign),
            "task modality cannot disable another thread's top-level window"
        );
        let request = engine.user32_dialog_request(0).unwrap().unwrap();
        let (epoch, id) = (request.epoch, request.id);
        engine.complete_user32_dialog(epoch, id, 1, 0).unwrap();
        assert_eq!(
            engine
                .user32
                .as_mut()
                .unwrap()
                .resume_wait(continuation, engine.memory.as_mut().unwrap())
                .unwrap(),
            User32Outcome32::Return(1)
        );
        assert!(engine.user32_dialog_request(0).unwrap().is_none());
    }
}

#[test]
fn dead_foreign_owner_help_request_is_neutral_and_preserves_the_original_button_completion() {
    use crate::{
        process::windows::user32::tests::{create, register},
        windows::window::{WindowError32, WindowHandle32},
    };

    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let mut foreign = call;
    foreign.thread_id += 1;
    let owner = create(&mut context, &mut memory, foreign, atom);
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    let dialog = engine
        .prepare_user32_dialog(Api::MessageBoxW, &[owner, 0, 0, 0x4001], call)
        .unwrap();
    let User32Outcome32::Wait { continuation } = engine
        .user32
        .as_mut()
        .unwrap()
        .begin_dialog(dialog)
        .unwrap()
    else {
        panic!("dialog must await a real host response")
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    let (epoch, id) = (request.epoch, request.id);
    let context = engine.user32.as_mut().unwrap();
    let exit = context
        .prepare_thread_termination(foreign.thread_id)
        .unwrap();
    context.commit_thread_termination(exit);
    assert_eq!(
        context
            .windows()
            .get(WindowHandle32::try_from(owner).unwrap()),
        Err(WindowError32::InvalidHandle)
    );
    assert_eq!(context.dialogs.len(), 1);
    assert!(!context.active_thread_wait_ready());
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.request_user32_dialog_help(epoch, id),
        Err(HostError::User32(E::Window(WindowError32::InvalidHandle)))
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    let context = engine.user32.as_ref().unwrap();
    assert!(!context.dialogs[0].help);
    assert_eq!(context.pending.last().unwrap().token, continuation);
    assert!(!context.active_thread_wait_ready());
    assert_eq!(engine.user32_dialog_request(0).unwrap().unwrap().id, id);

    engine.complete_user32_dialog(epoch, id, 1, 0).unwrap();
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert!(engine.user32_dialog_request(0).unwrap().is_none());
}

#[test]
fn parked_remote_help_validates_exact_continuation_and_mapping_before_receiver_entry() {
    use crate::process::windows::user32::{callback::Continuation32, memory::Scratch32};
    let (mut context, mut memory, _) = fixture();
    let scratch = Scratch32::allocate(&mut memory, context.scratch_bounds, &[0; 28]).unwrap();
    let outcome = context
        .start(Continuation32::DialogRemoteHelp {
            id: 1,
            scratch,
            ready: None,
        })
        .unwrap();
    let User32Outcome32::Wait { continuation } = outcome else {
        panic!("foreign HELP remains owned by its parked sender")
    };
    let sender = context.take_thread_reply();
    assert_eq!(sender.check_remote_scratch(continuation, &memory), Ok(()));
    assert_eq!(
        sender.check_remote_scratch(continuation + 1, &memory),
        Err(E::InvalidToken)
    );
    memory.unmap(scratch.range()).unwrap();
    memory
        .map_zeroed(scratch.range(), Permissions::READ_WRITE)
        .unwrap();
    memory.write(scratch.address, &[0xa5; 28]).unwrap();
    let version = memory.transaction_version();
    assert_eq!(
        sender.check_remote_scratch(continuation, &memory),
        Err(E::InvalidRequest)
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(sender.check_remote_result(continuation), Ok(()));
    let mut bytes = [0; 28];
    memory.read(scratch.address, &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 28]);
}

#[test]
fn armed_local_help_owner_destruction_finishes_as_invalid_window_without_help_callback() {
    use crate::process::windows::user32::tests::{create, register};

    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let owner = create(&mut context, &mut memory, call, atom);
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    let dialog = engine
        .prepare_user32_dialog(Api::MessageBoxW, &[owner, 0, 0, 0x4001], call)
        .unwrap();
    let User32Outcome32::Wait { continuation } = engine
        .user32
        .as_mut()
        .unwrap()
        .begin_dialog(dialog)
        .unwrap()
    else {
        panic!("dialog must await the host")
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    let (epoch, id) = (request.epoch, request.id);
    engine.request_user32_dialog_help(epoch, id).unwrap();
    assert!(engine.user32.as_ref().unwrap().active_thread_wait_ready());

    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    let mut outcome = context
        .prepare_arguments(Api::DestroyWindow, &[owner], memory, call)
        .unwrap();
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                assert_ne!(
                    callback.arguments[1], 0x53,
                    "only destruction callbacks may run"
                );
                outcome = context.complete(callback.continuation, 0, memory).unwrap();
            }
            User32Outcome32::Return(result) => {
                assert_eq!(result, 1);
                break;
            }
            other => panic!("unexpected destruction outcome: {other:?}"),
        }
    }
    assert_eq!(context.pending.last().unwrap().token, continuation);
    assert_eq!(
        context.resume_wait(continuation, memory).unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(context.take_last_error(), Some(1400));
    assert!(engine.user32_dialog_request(0).unwrap().is_none());
}

#[test]
fn armed_foreign_help_owner_thread_exit_retires_only_help_and_preserves_host_button() {
    use crate::{
        process::windows::user32::tests::{create, register},
        windows::window::{WindowError32, WindowHandle32},
    };

    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let mut foreign = call;
    foreign.thread_id += 1;
    let owner = create(&mut context, &mut memory, foreign, atom);
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    let dialog = engine
        .prepare_user32_dialog(Api::MessageBoxW, &[owner, 0, 0, 0x4001], call)
        .unwrap();
    let User32Outcome32::Wait { continuation } = engine
        .user32
        .as_mut()
        .unwrap()
        .begin_dialog(dialog)
        .unwrap()
    else {
        panic!("dialog must await the host")
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    let (epoch, id) = (request.epoch, request.id);
    engine.request_user32_dialog_help(epoch, id).unwrap();
    assert!(engine.user32.as_ref().unwrap().active_thread_wait_ready());
    let version = engine.memory().unwrap().transaction_version();
    let context = engine.user32.as_mut().unwrap();
    let exit = context
        .prepare_thread_termination(foreign.thread_id)
        .unwrap();
    context.commit_thread_termination(exit);
    assert_eq!(
        context
            .windows()
            .get(WindowHandle32::try_from(owner).unwrap()),
        Err(WindowError32::InvalidHandle)
    );
    assert_eq!(context.dialogs.len(), 1);
    assert_eq!(context.dialogs[0].result, None);
    assert_eq!(context.pending.last().unwrap().token, continuation);

    engine
        .dispatch_user32_dialog_remote_help(continuation)
        .unwrap();
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.user32_dialog_request(0).unwrap().unwrap().id, id);
    let context = engine.user32.as_mut().unwrap();
    assert_eq!(
        context
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Wait { continuation }
    );
    assert!(!context.active_thread_wait_ready());
    assert!(!context.dialogs[0].help);
    assert!(context.dialogs[0].result.is_none());

    engine.complete_user32_dialog(epoch, id, 1, 0).unwrap();
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert!(engine.user32_dialog_request(0).unwrap().is_none());
}
