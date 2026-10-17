use crate::{
    process::{CallError, EngineInstance, HostError},
    windows::{
        imm32::{ImmError32, InputComposition32, InputCompositionEvent32 as Event},
        user32::{Message32, User32Error32 as Error},
        window::WindowHandle32,
    },
};

impl EngineInstance {
    pub fn submit_ime_composition(
        &mut self,
        window: WindowHandle32,
        event: Event,
    ) -> Result<(), HostError> {
        self.memory()?;
        if self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let record = context
            .windows()
            .get(window)
            .map_err(|e| HostError::User32(e.into()))?;
        let thread = record.description.thread_id;
        if !matches!(event, Event::Cancel)
            && (context.windows().focus() != Some(window) || !record.description.enabled)
        {
            return Err(HostError::InvalidRequest);
        }
        if self.threads.info(thread).map_err(HostError::Thread)?.state
            == crate::windows::thread::ThreadState32::Terminated
        {
            return Err(HostError::InvalidRequest);
        }
        // queued notifications must retain the composition they describe until retrieval.
        if context
            .state
            .find_message(thread, 0, 0x10d, 0x10f)
            .map_err(HostError::User32)?
            .is_some()
        {
            return Err(HostError::Call(CallError::Busy));
        }
        let handle = context.ime.associated_context(window.get(), thread);
        if handle == Some(0) {
            return Err(HostError::InvalidRequest);
        }
        let active = handle
            .and_then(|h| context.ime.context(h).ok())
            .and_then(|c| c.composing_window);
        let begin = matches!(event, Event::Begin);
        if (begin && active.is_some()) || (!begin && active != Some(window.get())) {
            return Err(HostError::InvalidRequest);
        }
        let (composition, messages, count, active) = match event {
            Event::Begin => (
                InputComposition32::default(),
                [(0x10d, 0), (0, 0)],
                1,
                Some(window.get()),
            ),
            Event::Update(value) => {
                if !value.result.is_empty() {
                    return Err(HostError::InvalidRequest);
                }
                let flags = 8
                    | 0x80
                    | if value.attributes.is_empty() { 0 } else { 0x10 }
                    | if value.clauses.is_empty() { 0 } else { 0x20 };
                (value, [(0x10f, flags), (0, 0)], 1, Some(window.get()))
            }
            Event::Commit(result) => (
                InputComposition32 {
                    result,
                    ..Default::default()
                },
                [(0x10f, 0x800), (0x10e, 0)],
                2,
                None,
            ),
            Event::Cancel => (
                InputComposition32::default(),
                [(0x10f, 0), (0x10e, 0)],
                2,
                None,
            ),
        };
        context
            .ime
            .check_composition(&composition)
            .map_err(ime_error)?;
        context.state.can_post(count).map_err(HostError::User32)?;
        let queue = self.prepare_user32_queue_change_for(thread)?;
        let context = self.user32.as_mut().unwrap();
        let handle = context
            .ime
            .window_context(window.get(), thread)
            .map_err(ime_error)?;
        let target = context.ime.context_mut(handle, thread).map_err(ime_error)?;
        target.composition = composition;
        target.composing_window = active;
        for &(message, lparam) in &messages[..count] {
            context
                .state
                .post_raw_input(Message32 {
                    window: window.get(),
                    thread_id: thread,
                    message,
                    lparam,
                    ..Default::default()
                })
                .expect("preflighted composition message batch");
        }
        self.commit_user32_queue_change(queue);
        Ok(())
    }
}

fn ime_error(error: ImmError32) -> HostError {
    HostError::User32(match error {
        ImmError32::Allocation => Error::Allocation,
        ImmError32::Capacity | ImmError32::Exhausted => Error::Capacity,
        _ => Error::Win32(87),
    })
}

#[cfg(test)]
#[path = "../../service/tests/support.rs"]
mod support;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory::GuestAddress,
        windows::{imm32::ImmApi32 as Api, text::CodePage32},
    };

    fn fixture() -> (EngineInstance, WindowHandle32) {
        let (mut context, mut ram, mut call) = super::super::super::tests::fixture();
        call.thread_id = 1;
        let atom = super::super::super::tests::register(&mut context, &mut ram, call);
        let window = WindowHandle32::try_from(super::super::super::tests::create(
            &mut context,
            &mut ram,
            call,
            atom,
        ))
        .unwrap();
        context
            .state
            .windows_mut()
            .set_focus(1, Some(window))
            .unwrap();
        let mut engine = EngineInstance::new(16, 42).unwrap();
        engine.memory = Some(ram);
        engine.user32 = Some(context);
        (engine, window)
    }

    fn drain(engine: &mut EngineInstance) -> Vec<(u32, u32)> {
        let state = &mut engine.user32.as_mut().unwrap().state;
        let mut result = Vec::new();
        while let Some((index, msg)) = state.find_message(1, 0, 0x10d, 0x10f).unwrap() {
            result.push((msg.message, msg.lparam));
            state.remove_message(index);
        }
        result
    }

    #[test]
    fn host_composition_delivers_owned_text_and_result_through_real_imm_queries() {
        let (mut engine, window) = fixture();
        engine.submit_ime_composition(window, Event::Begin).unwrap();
        assert_eq!(drain(&mut engine), [(0x10d, 0)]);
        engine
            .submit_ime_composition(
                window,
                Event::Update(InputComposition32 {
                    text: "日本".encode_utf16().collect(),
                    attributes: vec![0, 1],
                    clauses: vec![0, 2],
                    cursor: 2,
                    ..Default::default()
                }),
            )
            .unwrap();
        assert_eq!(drain(&mut engine), [(0x10f, 0xb8)]);
        let context = engine.user32.as_mut().unwrap();
        let handle = context.ime.window_context(window.get(), 1).unwrap();
        assert_eq!(
            context
                .complete_imm32_call(
                    Api::ImmGetCompositionStringW,
                    &[handle, 8, 0x10100, 8],
                    1,
                    CodePage32::Windows1252,
                    engine.memory.as_mut().unwrap()
                )
                .unwrap(),
            4
        );
        let mut bytes = [0; 4];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x10100), &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0xe5, 0x65, 0x2c, 0x67]);
        engine
            .submit_ime_composition(window, Event::Commit(vec![0x65e5, 0x672c]))
            .unwrap();
        assert!(matches!(
            engine.submit_ime_composition(window, Event::Begin),
            Err(HostError::Call(CallError::Busy))
        ));
        assert_eq!(drain(&mut engine), [(0x10f, 0x800), (0x10e, 0)]);
        assert_eq!(
            engine
                .user32
                .as_ref()
                .unwrap()
                .ime
                .context(handle)
                .unwrap()
                .composition
                .result,
            [0x65e5, 0x672c]
        );
        engine.submit_ime_composition(window, Event::Begin).unwrap();
        assert!(
            engine
                .user32
                .as_ref()
                .unwrap()
                .ime
                .context(handle)
                .unwrap()
                .composition
                .result
                .is_empty()
        );
    }

    #[test]
    fn full_queue_and_invalid_updates_preserve_composition_and_retry_once() {
        let (mut engine, window) = fixture();
        engine.submit_ime_composition(window, Event::Begin).unwrap();
        drain(&mut engine);
        let value = InputComposition32 {
            text: vec![65],
            cursor: 1,
            ..Default::default()
        };
        let context = engine.user32.as_mut().unwrap();
        for _ in 0..8 {
            context
                .state
                .post(Message32 {
                    thread_id: 1,
                    message: 0x400,
                    ..Default::default()
                })
                .unwrap();
        }
        let handle = context.ime.window_context(window.get(), 1).unwrap();
        assert!(
            engine
                .submit_ime_composition(window, Event::Update(value.clone()))
                .is_err()
        );
        assert!(
            engine
                .user32
                .as_ref()
                .unwrap()
                .ime
                .context(handle)
                .unwrap()
                .composition
                .text
                .is_empty()
        );
        let context = engine.user32.as_mut().unwrap();
        let (index, _) = context
            .state
            .find_message(1, 0, 0x400, 0x400)
            .unwrap()
            .unwrap();
        context.state.remove_message(index);
        engine
            .submit_ime_composition(window, Event::Update(value))
            .unwrap();
        drain(&mut engine);
        assert!(
            engine
                .submit_ime_composition(
                    window,
                    Event::Update(InputComposition32 {
                        text: vec![65],
                        cursor: 2,
                        ..Default::default()
                    })
                )
                .is_err()
        );
        assert_eq!(
            engine
                .user32
                .as_ref()
                .unwrap()
                .ime
                .context(handle)
                .unwrap()
                .composition
                .text,
            [65]
        );
    }

    #[test]
    fn media_reset_discards_ime_notifications_and_cancels_the_owned_composition() {
        let (mut engine, window) = fixture();
        engine.configure_cpu_clock(1_000_000, 0).unwrap();
        engine.configure_media(Default::default()).unwrap();
        engine.submit_ime_composition(window, Event::Begin).unwrap();
        let handle = engine
            .user32
            .as_mut()
            .unwrap()
            .ime
            .window_context(window.get(), 1)
            .unwrap();
        engine.reset_media().unwrap();
        assert!(drain(&mut engine).is_empty());
        assert_eq!(
            engine
                .user32
                .as_ref()
                .unwrap()
                .ime
                .context(handle)
                .unwrap()
                .composing_window,
            None
        );
        engine.submit_ime_composition(window, Event::Begin).unwrap();
        drain(&mut engine);
        engine
            .user32
            .as_mut()
            .unwrap()
            .ime
            .retire_window(window.get());
        assert!(
            engine
                .submit_ime_composition(window, Event::Cancel)
                .is_err()
        );
        engine
            .user32
            .as_mut()
            .unwrap()
            .state
            .windows_mut()
            .set_focus(1, None)
            .unwrap();
        assert!(engine.submit_ime_composition(window, Event::Begin).is_err());
    }
    #[test]
    fn exact_composition_owner_can_cancel_after_focus_loss_with_queue_failure_neutrality() {
        let (mut engine, window) = fixture();
        engine.submit_ime_composition(window, Event::Begin).unwrap();
        drain(&mut engine);
        engine
            .submit_ime_composition(
                window,
                Event::Update(InputComposition32 {
                    text: vec![65],
                    cursor: 1,
                    ..Default::default()
                }),
            )
            .unwrap();
        drain(&mut engine);
        let call = crate::process::windows::user32::User32CallContext32 {
            thread_id: 1,
            instance: 0x400000,
            ..Default::default()
        };
        let context = engine.user32.as_mut().unwrap();
        let atom = context.state.record(window).unwrap().class_atom;
        let other = WindowHandle32::try_from(super::super::super::tests::create(
            context,
            engine.memory.as_mut().unwrap(),
            call,
            u32::from(atom),
        ))
        .unwrap();
        context
            .state
            .windows_mut()
            .set_focus(1, Some(other))
            .unwrap();
        let mut description = context.windows().get(window).unwrap().description;
        description.enabled = false;
        context
            .state
            .windows_mut()
            .update_description(window, description)
            .unwrap();
        let handle = context.ime.window_context(window.get(), 1).unwrap();
        assert!(matches!(
            engine.submit_ime_composition(other, Event::Cancel),
            Err(HostError::InvalidRequest)
        ));
        assert!(matches!(
            engine.submit_ime_composition(window, Event::Update(InputComposition32::default())),
            Err(HostError::InvalidRequest)
        ));
        let context = engine.user32.as_mut().unwrap();
        for _ in 0..8 {
            context
                .state
                .post(Message32 {
                    thread_id: 1,
                    message: 0x400,
                    ..Default::default()
                })
                .unwrap();
        }
        assert!(
            engine
                .submit_ime_composition(window, Event::Cancel)
                .is_err()
        );
        let context = engine.user32.as_mut().unwrap();
        assert_eq!(
            context.ime.context(handle).unwrap().composing_window,
            Some(window.get())
        );
        assert_eq!(context.ime.context(handle).unwrap().composition.text, [65]);
        for _ in 0..2 {
            let (at, _) = context
                .state
                .find_message(1, 0, 0x400, 0x400)
                .unwrap()
                .unwrap();
            context.state.remove_message(at);
        }
        engine
            .submit_ime_composition(window, Event::Cancel)
            .unwrap();
        assert_eq!(drain(&mut engine), [(0x10f, 0), (0x10e, 0)]);
        let context = engine.user32.as_ref().unwrap();
        assert_eq!(context.ime.context(handle).unwrap().composing_window, None);
        assert_eq!(
            context.ime.context(handle).unwrap().composition,
            InputComposition32::default()
        );
        engine.submit_ime_composition(other, Event::Begin).unwrap();
        assert_eq!(drain(&mut engine), [(0x10d, 0)]);
    }
    #[test]
    fn media_reset_projects_nonprimary_ime_queue_once_and_failed_preflight_retains_its_owner() {
        use crate::windows::thread::{ThreadApi32, ThreadState32};
        let mut engine = super::support::engine();
        engine.configure_media(Default::default()).unwrap();
        engine
            .configure_synchronization(Default::default())
            .unwrap();
        let handle = super::support::call(
            &mut engine,
            ThreadApi32::CreateThread.id(),
            &[0, 0, 0x401000, 0, 4, super::support::DATA],
        );
        assert_ne!(handle, 0);
        let thread = super::support::word(&engine, super::support::DATA);
        assert_ne!(thread, 1);
        assert_eq!(
            engine.windows_thread_info(thread).unwrap().state,
            ThreadState32::Suspended
        );
        let (mut context, mut ram, mut call) = super::super::super::tests::fixture();
        call.thread_id = thread;
        let atom = super::super::super::tests::register(&mut context, &mut ram, call);
        let window = WindowHandle32::try_from(super::super::super::tests::create(
            &mut context,
            &mut ram,
            call,
            atom,
        ))
        .unwrap();
        context
            .state
            .windows_mut()
            .set_focus(thread, Some(window))
            .unwrap();
        engine.user32 = Some(context);
        let queue = engine.prepare_user32_queue_change_for(1).unwrap();
        engine
            .user32
            .as_mut()
            .unwrap()
            .state
            .post(Message32 {
                thread_id: 1,
                message: 0x400,
                ..Default::default()
            })
            .unwrap();
        engine.commit_user32_queue_change(queue);
        let primary = engine.synchronization().unwrap().queue_state(1);
        engine.submit_ime_composition(window, Event::Begin).unwrap();
        let input = engine
            .user32
            .as_mut()
            .unwrap()
            .ime
            .window_context(window.get(), thread)
            .unwrap();
        let before = engine.synchronization().unwrap().queue_state(thread);
        assert_ne!(before.0 & 0x108, 0);
        let revision = engine.synchronization().unwrap().revision();
        let epoch = engine.media_epoch().unwrap();
        engine
            .synchronization_mut()
            .unwrap()
            .set_revision_for_test(u64::MAX);
        assert!(matches!(
            engine.reset_media(),
            Err(HostError::Synchronization(
                crate::windows::synchronization::SynchronizationError32::Exhausted
            ))
        ));
        assert_eq!(engine.media_epoch().unwrap(), epoch);
        assert_eq!(
            engine.synchronization().unwrap().queue_state(thread),
            before
        );
        let context = engine.user32.as_ref().unwrap();
        assert_eq!(
            context.ime.context(input).unwrap().composing_window,
            Some(window.get())
        );
        assert!(
            context
                .state
                .find_message(thread, 0, 0x10d, 0x10f)
                .unwrap()
                .is_some()
        );
        engine
            .synchronization_mut()
            .unwrap()
            .set_revision_for_test(revision);
        engine.reset_media().unwrap();
        assert_eq!(engine.synchronization().unwrap().revision(), revision + 1);
        assert_eq!(engine.synchronization().unwrap().queue_state(thread).0, 0);
        assert_eq!(engine.synchronization().unwrap().queue_state(1), primary);
        let context = engine.user32.as_ref().unwrap();
        assert!(
            context
                .state
                .find_message(thread, 0, 0x10d, 0x10f)
                .unwrap()
                .is_none()
        );
        assert_eq!(context.ime.context(input).unwrap().composing_window, None);
        assert_eq!(
            context.ime.context(input).unwrap().composition,
            InputComposition32::default()
        );
    }
}
