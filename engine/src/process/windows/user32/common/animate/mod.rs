mod loading;
mod painting;
mod waiting;
use super::super::{
    User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32,
};
use crate::{
    memory::AddressSpace,
    process::{
        HostError,
        windows::{files::ProviderFile32, gdi::GdiRuntime32},
    },
    windows::{
        comctl32::{
            CommonControlState32 as State,
            animate::{Animate32, AviClip32},
        },
        user32::{User32Api32, User32Error32 as E},
        window::WindowHandle32,
    },
};
const TIMER_ID: u32 = 1;
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct AnimationCall32 {
    window: WindowHandle32,
    call: User32CallContext32,
    source: Option<ProviderFile32>,
    length: usize,
    file_id: u32,
    source_version: Option<crate::files::FileContentVersion32>,
    pub bytes_wait: bool,
    ready: bool,
    layout: Option<(u32, u32)>,
    notifications: [u32; 2],
    count: usize,
    next: usize,
    child: Option<(u64, bool)>,
    completed: Option<u64>,
    result: u32,
}
impl AnimationCall32 {
    pub(in crate::process::windows::user32) fn ready(self, context: &User32Context32) -> bool {
        !self.bytes_wait || context.windows().get(self.window).is_err()
    }
}
impl User32Context32 {
    fn animation(&self, window: WindowHandle32) -> Result<&Animate32, E> {
        match self.state.common_state(window)? {
            State::Animate(value) => Ok(value),
            _ => Err(E::Win32(1407)),
        }
    }
    fn animation_mut(&mut self, window: WindowHandle32) -> Result<&mut Animate32, E> {
        match self.state.common_state_mut(window)? {
            State::Animate(value) => Ok(value),
            _ => Err(E::Win32(1407)),
        }
    }
    fn check_animation_call(&mut self) -> Result<(), E> {
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        Ok(())
    }
    fn begin_animation_call(
        &mut self,
        window: WindowHandle32,
        call: User32CallContext32,
        source: Option<ProviderFile32>,
        length: usize,
        result: u32,
    ) -> Result<User32Outcome32, E> {
        self.start(Continuation32::Animation(AnimationCall32 {
            window,
            call,
            source,
            length,
            file_id: 0,
            source_version: None,
            bytes_wait: false,
            ready: source.is_none(),
            layout: None,
            notifications: [0; 2],
            count: 0,
            next: 0,
            child: None,
            completed: None,
            result,
        }))
    }
    fn publish_animation(
        &mut self,
        window: WindowHandle32,
        clip: Option<AviClip32>,
        call: User32CallContext32,
    ) -> Result<AnimationCall32, E> {
        self.check_animation_call()?;
        self.state.check_paint_change()?;
        let style = self.state.record(window)?.style;
        let playing = clip.is_some() && style & 4 != 0;
        if playing {
            self.timers.check_window_set(
                call.thread_id,
                window.get(),
                TIMER_ID,
                clip.as_ref().unwrap().period_ms,
                call.now_ns,
            )?;
        }
        let old_playing = self.animation(window)?.playing;
        let mut notifications = [0; 2];
        let mut count = 0;
        if old_playing {
            notifications[count] = 2;
            count += 1;
        }
        if playing {
            notifications[count] = 1;
            count += 1;
        }
        let layout = clip
            .as_ref()
            .filter(|_| style & 1 == 0)
            .map(|clip| (clip.layout.width, clip.layout.height));
        self.kill_timer(&[window.get(), TIMER_ID], call)?;
        if playing {
            self.set_timer(
                &[window.get(), TIMER_ID, clip.as_ref().unwrap().period_ms, 0],
                call,
            )
            .expect("animation timer was preflighted");
        }
        let last = clip.as_ref().map_or(0, |clip| clip.frames.len() - 1);
        *self.animation_mut(window)? = Animate32 {
            clip,
            playing,
            last,
            remaining: u32::MAX,
            ..Default::default()
        };
        self.state
            .invalidate_control(window)
            .expect("animation publication preflighted paint");
        Ok(AnimationCall32 {
            window,
            call,
            source: None,
            length: 0,
            file_id: 0,
            source_version: None,
            bytes_wait: false,
            ready: true,
            layout,
            notifications,
            count,
            next: 0,
            child: None,
            completed: None,
            result: 1,
        })
    }
    pub(in crate::process::windows::user32::common) fn animate_control_message(
        &mut self,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: Option<&mut GdiRuntime32>,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        let window = self
            .owned(args[0], call.thread_id)
            .map_err(HostError::User32)?;
        let result = match args[1] {
            0x81 => {
                if self.state.record(window).map_err(HostError::User32)?.style & 2 != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                1
            }
            1 => 0,
            0xf | 0x14 | 0x318 => {
                return self.paint_animation(
                    window,
                    args[2],
                    args[1],
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call.thread_id,
                );
            }
            0x84 => (-1i32) as u32,
            5 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.state
                    .invalidate_control(window)
                    .expect("animation size paint preflight");
                0
            }
            0x468 => u32::from(self.animation(window).map_err(HostError::User32)?.playing),
            _ => {
                return self
                    .animation_command(window, args, call)
                    .map_err(HostError::User32);
            }
        };
        Ok(User32Outcome32::Return(result))
    }
}
impl User32Context32 {
    fn animation_command(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        match args[1] {
            0x465 => {
                if self.animation(window)?.clip.is_none() {
                    return Ok(User32Outcome32::Return(0));
                }
                if args[2] == 0 {
                    return Ok(User32Outcome32::Return(0));
                }
                let (first, last) = self
                    .animation(window)?
                    .play_range(args[3] as u16, (args[3] >> 16) as u16)?;
                self.check_animation_call()?;
                self.state.check_paint_change()?;
                let delay = self.animation(window)?.clip.as_ref().unwrap().period_ms;
                let playing = first != last;
                if playing {
                    self.timers.check_window_set(
                        call.thread_id,
                        window.get(),
                        TIMER_ID,
                        delay,
                        call.now_ns,
                    )?;
                }
                let old = self.animation(window)?.playing;
                self.kill_timer(&[window.get(), TIMER_ID], call)?;
                if playing {
                    self.set_timer(&[window.get(), TIMER_ID, delay, 0], call)
                        .expect("animation play timer preflighted");
                }
                let value = self.animation_mut(window)?;
                value.current = first;
                value.first = first;
                value.last = last;
                value.remaining = args[2];
                value.playing = playing;
                self.state
                    .invalidate_control(window)
                    .expect("animation play paint preflighted");
                let outcome = self.begin_animation_call(window, call, None, 0, 1)?;
                let User32Outcome32::Wait { continuation } = outcome else {
                    unreachable!()
                };
                let mut pending = self.animation_wait(continuation).unwrap();
                if old {
                    pending.notifications[pending.count] = 2;
                    pending.count += 1;
                }
                if playing {
                    pending.notifications[pending.count] = 1;
                    pending.count += 1;
                }
                self.publish_animation_call(continuation, pending);
                Ok(outcome)
            }
            0x10 => {
                let prepared = self.publish_animation(window, None, call)?;
                self.start(Continuation32::Animation(prepared))
            }
            0x466 => {
                self.check_animation_call()?;
                let old = self.animation(window)?.playing;
                self.kill_timer(&[window.get(), TIMER_ID], call)?;
                self.animation_mut(window)?.playing = false;
                let outcome = self.begin_animation_call(window, call, None, 0, 1)?;
                let User32Outcome32::Wait { continuation } = outcome else {
                    unreachable!()
                };
                if old {
                    let mut pending = self.animation_wait(continuation).unwrap();
                    pending.notifications[0] = 2;
                    pending.count = 1;
                    self.publish_animation_call(continuation, pending);
                }
                Ok(outcome)
            }
            0x113 if args[2] == TIMER_ID && self.animation(window)?.playing => {
                self.check_animation_call()?;
                self.state.check_paint_change()?;
                let stop = self.animation_mut(window)?.advance();
                if stop {
                    self.kill_timer(&[window.get(), TIMER_ID], call)?;
                }
                self.state
                    .invalidate_control(window)
                    .expect("animation tick paint preflighted");
                if stop {
                    let outcome = self.begin_animation_call(window, call, None, 0, 0)?;
                    let User32Outcome32::Wait { continuation } = outcome else {
                        unreachable!()
                    };
                    let mut pending = self.animation_wait(continuation).unwrap();
                    pending.notifications[0] = 2;
                    pending.count = 1;
                    self.publish_animation_call(continuation, pending);
                    Ok(outcome)
                } else {
                    Ok(User32Outcome32::Return(0))
                }
            }
            0x400..=0xffff => Err(E::Win32(50)),
            _ => Ok(User32Outcome32::Return(0)),
        }
    }
}

fn error_code(error: E) -> u32 {
    match error {
        E::Win32(code) => code,
        E::Allocation | E::Capacity => 8,
        _ => 87,
    }
}

#[cfg(test)]
impl User32Context32 {
    pub(in crate::process) fn destroy_animation_window_for_test(
        &mut self,
        window: u32,
        memory: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<(), E> {
        let mut outcome =
            self.prepare_arguments(User32Api32::DestroyWindow, &[window], memory, call)?;
        loop {
            match outcome {
                User32Outcome32::Callback(callback) => {
                    outcome = self.complete(callback.continuation, 0, memory)?
                }
                User32Outcome32::Return(1) => return Ok(()),
                _ => return Err(E::InvalidRequest),
            }
        }
    }
}
