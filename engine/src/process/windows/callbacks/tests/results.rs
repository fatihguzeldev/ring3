use super::*;
use crate::{
    cpu::x86::State32,
    memory::{AddressSpace, PageRange, Permissions},
    windows::CallingConvention32,
};

#[test]
fn sealed_callback_results_require_exact_owner_token_frame_provider_and_continuation() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut state = State32::default();
    state.registers[4] = 0x10000;
    let frame = CallFrame32::capture(&memory, state, CallingConvention32::Stdcall, 0).unwrap();
    let mut reply = WindowsReply32 {
        owner: PendingOwner::Replacement(3),
        token: 7,
        frame,
        provider: WindowsCallbackProvider32::Ole32,
        continuation: 11,
        last_error: None,
        stage: ReplyStage32::Received(0x80004005),
        activation: None,
    };
    let result = |reply: &WindowsReply32, provider, owner, token, continuation, frame| {
        reply.received_result(provider, owner, token, continuation, frame)
    };
    assert_eq!(
        result(&reply, reply.provider, reply.owner, 7, 11, frame),
        Some(0x80004005)
    );
    assert_eq!(
        result(
            &reply,
            WindowsCallbackProvider32::User32,
            reply.owner,
            7,
            11,
            frame
        ),
        None
    );
    assert_eq!(
        result(
            &reply,
            reply.provider,
            PendingOwner::Replacement(4),
            7,
            11,
            frame
        ),
        None
    );
    assert_eq!(
        result(&reply, reply.provider, reply.owner, 8, 11, frame),
        None
    );
    assert_eq!(
        result(&reply, reply.provider, reply.owner, 7, 12, frame),
        None
    );
    state.eip = 1;
    let changed = CallFrame32::capture(&memory, state, CallingConvention32::Stdcall, 0).unwrap();
    assert_eq!(
        result(&reply, reply.provider, reply.owner, 7, 11, changed),
        None
    );
    for stage in [
        ReplyStage32::Wait,
        ReplyStage32::Complete(0x80004005),
        ReplyStage32::Resident {
            callback_id: 1,
            token: 1,
        },
    ] {
        reply.stage = stage;
        assert_eq!(
            result(&reply, reply.provider, reply.owner, 7, 11, frame),
            None
        );
    }
}
