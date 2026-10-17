use super::*;
use crate::windows::user32::{Message32, User32Error32};

fn projection(engine: &EngineInstance, boundary: &str) {
    assert_eq!(
        engine.synchronization().unwrap().queue_state(1),
        engine.user32.as_ref().unwrap().wait_queue_status(1),
        "{boundary}: the accepted USER32 queue and canonical message wait projection must agree"
    );
}

fn release_one_filler(engine: &mut EngineInstance, root: u32) {
    let prepared = engine.prepare_user32_queue_change_for(1).unwrap();
    let context = engine.user32.as_mut().unwrap();
    let (index, _) = context
        .state
        .find_message(1, root, 0x8000, 0x8000)
        .unwrap()
        .unwrap();
    context.state.remove_message(index);
    engine.commit_user32_queue_change(prepared);
}

#[test]
fn modal_dead_key_translation_retains_one_message_without_splitting_the_queue_projection_on_capacity_failure()
 {
    let mut engine = engine();
    let mut keys = [0; 256];
    keys[0x10] = 0x80;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA + 0x400), &keys)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA + 0x600), b"0000041F\0")
        .unwrap();
    call(
        &mut engine,
        Api::LoadKeyboardLayoutA.id(),
        &[DATA + 0x600, 1],
    )
    .unwrap();
    assert_eq!(
        crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0x041f041f
    );
    call(
        &mut engine,
        Api::ToUnicode.id(),
        &[u32::from(b'3'), 4, DATA + 0x400, DATA + 0x500, 2, 0],
    )
    .unwrap();
    assert_eq!(
        crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        u32::MAX
    );
    let mut accent = [0; 2];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(DATA + 0x500), &mut accent)
        .unwrap();
    assert_eq!(accent, (b'^' as u16).to_le_bytes());
    write_template(&mut engine);
    let token = begin(&mut engine, true);
    let root = reach_init(&mut engine);
    complete_callback(&mut engine, 0).unwrap();
    for _ in 0..64 {
        let Some(request) = engine.pending_windows_callback() else {
            break;
        };
        complete_callback(&mut engine, u32::from(request.arguments[1] == 0x81)).unwrap();
    }
    let frame = engine.pending_call.as_ref().unwrap().frame;
    engine
        .post_user32_message(Message32 {
            thread_id: 1,
            window: root,
            message: 0x100,
            wparam: u32::from(b'1'),
            ..Default::default()
        })
        .unwrap();
    for index in 0..31 {
        engine
            .post_user32_message(Message32 {
                thread_id: 1,
                window: root,
                message: 0x8000 + index,
                ..Default::default()
            })
            .unwrap();
    }
    projection(&engine, "before first resume");
    let version = engine.memory.as_ref().unwrap().transaction_version();
    let wait = engine.waiting_user32_message().unwrap();
    let accepted = engine.resume_user32_wait(KEY, wait);
    projection(&engine, "after first resume");
    let failure = if accepted.is_ok() {
        let version = engine.memory.as_ref().unwrap().transaction_version();
        let queue = engine.user32.as_ref().unwrap().wait_queue_status(1);
        let wait = engine.waiting_user32_message().unwrap();
        let failure = engine.resume_user32_wait(KEY, wait);
        assert_eq!(
            engine.memory.as_ref().unwrap().transaction_version(),
            version
        );
        assert_eq!(engine.user32.as_ref().unwrap().wait_queue_status(1), queue);
        failure
    } else {
        assert_eq!(
            engine.memory.as_ref().unwrap().transaction_version(),
            version
        );
        accepted
    };
    assert_eq!(failure, Err(HostError::User32(User32Error32::Win32(1816))));
    projection(&engine, "after capacity refusal");
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    assert!(engine.pending_windows_callback().is_none());
    release_one_filler(&mut engine, root);
    for _ in 0..4 {
        if engine.pending_windows_callback().is_some() {
            break;
        }
        let wait = engine.waiting_user32_message().unwrap();
        engine.resume_user32_wait(KEY, wait).unwrap();
    }
    let request = engine.pending_windows_callback().unwrap();
    assert_eq!(request.count, 4);
    assert_eq!(&request.arguments[..4], &[root, 0x100, u32::from(b'1'), 0]);
    assert!(request.arguments[4..].iter().all(|&argument| argument == 0));
    projection(&engine, "before accepted key callback");
    complete_callback(&mut engine, 0).unwrap();
    projection(&engine, "after accepted key callback");
    assert!(engine.pending_windows_callback().is_none());
    for expected in [u32::from(b'^'), u32::from(b'1')] {
        let prepared = engine.prepare_user32_queue_change_for(1).unwrap();
        let context = engine.user32.as_mut().unwrap();
        let (index, message) = context
            .state
            .find_message(1, root, 0x102, 0x102)
            .unwrap()
            .unwrap();
        assert_eq!(message.wparam, expected);
        context.state.remove_message(index);
        engine.commit_user32_queue_change(prepared);
    }
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .find_message(1, root, 0x102, 0x102)
            .unwrap()
            .is_none()
    );
}
