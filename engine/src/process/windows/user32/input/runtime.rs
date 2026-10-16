use crate::{
    input::source::PreparedSourceWindow32,
    process::{EngineInstance, HostError, MediaError},
    windows::window::WindowHandle32,
};

pub(in crate::process) struct PreparedUser32Queue32 {
    signals: crate::windows::synchronization::PreparedSignals32,
    thread: u32,
}

impl EngineInstance {
    pub(in crate::process) fn initialize_user32_cursor_source(
        &self,
        input: &mut crate::input::InputContext,
        timestamp: u64,
    ) -> Result<(), HostError> {
        let Some(context) = self.user32.as_ref() else {
            return Ok(());
        };
        let point = context.input.query_point;
        let bounds = context.input.clip.unwrap_or(context.input.desktop);
        let source = input.source_mut();
        let prepared = source
            .prepare_window(timestamp)
            .map_err(|error| HostError::Media(MediaError::Input(error)))?;
        source.commit_cursor_policy(
            prepared,
            point.x,
            point.y,
            Some((bounds.left, bounds.top, bounds.right, bounds.bottom)),
        );
        Ok(())
    }
    pub(in crate::process) fn prepare_user32_media_reset(
        &self,
    ) -> Result<Option<(u32, u32, u64)>, HostError> {
        let Some(context) = self.user32.as_ref() else {
            return Ok(None);
        };
        if context.input.mouse_exclusive && context.input.cursor_revision == u64::MAX {
            return Err(HostError::User32(
                crate::windows::user32::User32Error32::Capacity,
            ));
        }
        Ok(Some(context.state.queue_without_input(1)))
    }
    pub(in crate::process) fn commit_user32_media_reset(&mut self) {
        let (Some(context), Some(media)) = (self.user32.as_mut(), self.media.as_ref()) else {
            return;
        };
        let was_exclusive = context.input.mouse_exclusive;
        context.state.discard_input_messages();
        context.raw_input.reset_packets();
        context
            .input
            .attach(media.input.source().tail(), media.input.source().snapshot());
        if was_exclusive && !context.input.mouse_exclusive {
            context.input.cursor_revision += 1;
            context.input.cursor_request = Some(super::User32CursorRequest32 {
                revision: context.input.cursor_revision,
                changes: 4 | context
                    .input
                    .cursor_request
                    .map_or(0, |request| request.changes),
                position: context.input.point,
                clip: context.input.clip,
                visible: context.input.show_count >= 0,
                cursor: context.input.cursor_handle,
            });
        }
        context.reset_thread_layout_delivery();
        for value in &mut context.input.queued {
            *value &= 1;
        }
        context.input.focus = context.windows().focus().map_or(0, WindowHandle32::get);
        context.input.capture = context.windows().capture().map_or(0, WindowHandle32::get);
        if let Ok(window) = WindowHandle32::try_from(context.input.focus)
            .and_then(|handle| context.windows().get(handle))
        {
            context.input.delivery_layout =
                context.delivered_thread_layout(window.description.thread_id);
        }
    }
    pub(in crate::process) fn refresh_user32_input(&mut self) -> Result<(), HostError> {
        if let (Some(user32), Some(media)) = (self.user32.as_mut(), self.media.as_ref()) {
            user32.input.observe_async(media.input.source().snapshot());
        }
        match self.sync_user32_input() {
            Err(HostError::User32(crate::windows::user32::User32Error32::Win32(1816))) => Ok(()),
            result => result,
        }
    }

    pub(in crate::process) fn prepare_user32_window_event(
        &self,
    ) -> Result<Option<PreparedSourceWindow32>, HostError> {
        let Some(media) = self.media.as_ref() else {
            return Ok(None);
        };
        let (_, now) = self.cpu_clock.monotonic_bounds()?;
        let source = media.input.source();
        let cursor = self
            .user32
            .as_ref()
            .and_then(|context| context.source_cursor())
            .unwrap_or(crate::input::source::InputCursor32 {
                epoch: source.tail().epoch,
                sequence: 0,
            });
        if !source.can_retain_from(cursor, 1) {
            return Err(HostError::Media(MediaError::Input(
                crate::input::InputError::QueueFull,
            )));
        }
        source
            .prepare_window(now)
            .map(Some)
            .map_err(|error| HostError::Media(MediaError::Input(error)))
    }

    pub(in crate::process) fn user32_translation_settings(
        &self,
    ) -> Option<(crate::input::keyboard::KeyboardLayout32, bool, u64)> {
        self.user32.as_ref().map(|context| {
            (
                context.input.layout,
                context.input.swapped,
                context.process_layout.map_or(0, |policy| policy.0),
            )
        })
    }
    pub(in crate::process) fn commit_user32_translation_event(
        &mut self,
        prepared: Option<PreparedSourceWindow32>,
        previous: Option<(crate::input::keyboard::KeyboardLayout32, bool, u64)>,
    ) {
        let actual = self.user32_translation_settings();
        if actual != previous
            && let (Some(prepared), Some((layout, swapped, revision))) = (prepared, actual)
        {
            let thread = self.threads.current_id();
            let source = self.media.as_mut().unwrap().input.source_mut();
            if previous.is_some_and(|previous| previous.2 != revision) {
                source.commit_process_translation_policy(prepared, layout, swapped);
            } else {
                source.commit_thread_translation_policy(prepared, thread, layout, swapped);
            }
        }
    }
    pub(in crate::process) fn commit_user32_cursor_event(
        &mut self,
        prepared: Option<PreparedSourceWindow32>,
        clipping: bool,
    ) {
        let Some(prepared) = prepared else {
            return;
        };
        let input = &self.user32.as_ref().unwrap().input;
        let point = input.query_point;
        let source = self.media.as_mut().unwrap().input.source_mut();
        if clipping {
            source.commit_cursor_policy(
                prepared,
                point.x,
                point.y,
                Some({
                    let clip = input.clip.unwrap_or(input.desktop);
                    (clip.left, clip.top, clip.right, clip.bottom)
                }),
            );
        } else {
            source.commit_cursor(prepared, point.x, point.y);
        }
    }
    pub(in crate::process) fn user32_window_identities(&self) -> (u32, u32) {
        self.user32_windows().map_or((0, 0), |windows| {
            (
                windows.focus().map_or(0, WindowHandle32::get),
                windows.capture().map_or(0, WindowHandle32::get),
            )
        })
    }

    pub(in crate::process) fn commit_user32_window_event(
        &mut self,
        prepared: Option<PreparedSourceWindow32>,
        previous: (u32, u32),
    ) {
        let actual = self.user32_window_identities();
        if actual != previous
            && let Some(prepared) = prepared
        {
            self.media
                .as_mut()
                .unwrap()
                .input
                .source_mut()
                .commit_window(prepared, actual.0, actual.1);
        }
    }

    pub(in crate::process) fn prepare_user32_queue_change(
        &self,
    ) -> Result<Option<PreparedUser32Queue32>, HostError> {
        self.prepare_user32_queue_change_for(self.current_windows_thread_id()?)
    }
    pub(in crate::process) fn prepare_user32_queue_change_for(
        &self,
        thread: u32,
    ) -> Result<Option<PreparedUser32Queue32>, HostError> {
        let (Some(context), Some(sync)) = (self.user32.as_ref(), self.synchronization.as_ref())
        else {
            return Ok(None);
        };
        let (_, _, generation) = context.wait_queue_status(thread);
        let signals = sync
            .context()
            .prepare_messages_for_thread(thread, u32::MAX, u32::MAX, generation)
            .map_err(HostError::Synchronization)?;
        Ok(Some(PreparedUser32Queue32 { signals, thread }))
    }

    pub(in crate::process) fn commit_user32_queue_change(
        &mut self,
        prepared: Option<PreparedUser32Queue32>,
    ) {
        let Some(prepared) = prepared else {
            return;
        };
        let (current, new, generation) = self
            .user32
            .as_ref()
            .unwrap()
            .wait_queue_status(prepared.thread);
        let sync = self.synchronization.as_mut().unwrap().context_mut();
        let receipt = sync.refresh_prepared_messages_for_thread(
            prepared.signals,
            prepared.thread,
            current,
            new,
            generation,
        );
        sync.commit_signals(receipt);
    }
}
