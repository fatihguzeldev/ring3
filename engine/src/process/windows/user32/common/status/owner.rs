use super::*;
use crate::process::windows::user32::{callback::Continuation32, memory::Scratch32};
use crate::windows::gdi::Region32;

#[derive(Clone, Copy)]
enum Stage32 {
    Paint,
    Scan,
    Send,
    Next,
    Finish,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct StatusDraw32 {
    window: WindowHandle32,
    parent: WindowHandle32,
    call: User32CallContext32,
    scratch: Scratch32,
    index: usize,
    only: bool,
    stage: Stage32,
    dc: Option<u32>,
    result: u32,
    child: Option<u64>,
    completed: Option<u64>,
}

impl StatusDraw32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Scratch32 {
        self.scratch
    }
}

impl User32Context32 {
    pub(in crate::process::windows::user32::common::status) fn begin_status_draw(
        &mut self,
        window: WindowHandle32,
        index: usize,
        paint: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let parent = self.state.record(window)?.parent.ok_or(E::Win32(1406))?;
        self.windows().get(parent)?;
        self.prepare_thread_slots()?;
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &[0; 48])?;
        self.start(Continuation32::StatusDraw(StatusDraw32 {
            window,
            parent,
            call,
            scratch,
            index,
            only: !paint,
            stage: if paint { Stage32::Paint } else { Stage32::Scan },
            dc: None,
            result: u32::from(!paint),
            child: None,
            completed: None,
        }))
    }

    fn status_draw(&self, token: u64) -> Result<StatusDraw32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::StatusDraw(call) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(call)
    }
    fn publish_status_draw(&mut self, token: u64, call: StatusDraw32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("status draw owner")
            .continuation = Continuation32::StatusDraw(call);
    }

    pub(in crate::process::windows::user32) fn status_draw_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token && matches!(pending.continuation, Continuation32::StatusDraw(_))
        })
    }

    pub(in crate::process::windows::user32) fn pending_status_draw_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let call = self.status_draw(token)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        if !matches!(call.stage, Stage32::Send) {
            return Ok(None);
        }
        let id = self.state.record(call.window)?.menu;
        Ok(Some([call.parent.get(), 0x2b, id, call.scratch.address.0]))
    }

    pub(in crate::process::windows::user32) fn resume_status_draw(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        let mut call = self.status_draw(token).map_err(HostError::User32)?;
        call.scratch.check(ram).map_err(HostError::User32)?;
        if call.child.is_some() {
            return Err(HostError::User32(E::InvalidToken));
        }
        if self.windows().get(call.window).is_err() || self.windows().get(call.parent).is_err() {
            call.stage = Stage32::Finish;
        }
        match call.stage {
            Stage32::Paint => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.render_status_text_to(call.window, 0, ram, gdi, call.call.thread_id)?;
                self.state
                    .set_update_region(call.window, Region32::default(), false)
                    .expect("status paint was preflighted");
                call.stage = Stage32::Scan;
            }
            Stage32::Scan => {
                let status = self.status(call.window).map_err(HostError::User32)?;
                if status.simple_mode {
                    call.stage = Stage32::Finish;
                } else {
                    if !call.only {
                        while call.index < status.count()
                            && status.part(call.index).unwrap().kind & 0x1000 == 0
                        {
                            call.index += 1;
                        }
                    }
                    let part = status.part(call.index);
                    if part.is_none_or(|part| part.kind & 0x1000 == 0) {
                        call.stage = Stage32::Finish;
                    } else {
                        let part = part.unwrap();
                        let extent = self
                            .windows()
                            .get(call.window)
                            .map_err(E::from)
                            .map_err(HostError::User32)?
                            .description
                            .extent;
                        let rect = status
                            .rect(call.index, extent.width as i32, extent.height as i32)
                            .unwrap();
                        let data = part.data;
                        let font = if status.font == 0 {
                            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
                        } else {
                            status.font
                        };
                        let background = if status.background == 0xff000000 {
                            gdi.system_color(15)
                        } else {
                            status.background
                        };
                        let id = self
                            .state
                            .record(call.window)
                            .map_err(HostError::User32)?
                            .menu;
                        ram.ensure_transaction_versions(1)
                            .map_err(HostError::Memory)?;
                        ram.check_span(call.scratch.address, 48, crate::memory::Access::Write)
                            .map_err(HostError::Memory)?;
                        let projection = self
                            .gdi_window(call.window.get(), false)
                            .map_err(HostError::User32)?;
                        let dc = gdi
                            .begin_window_paint_dc(
                                projection,
                                Region32::rectangle(rect),
                                font,
                                background,
                            )
                            .map_err(HostError::Gdi)?;
                        let mut bytes = [0; 48];
                        for (output, value) in bytes.chunks_exact_mut(4).zip([
                            0,
                            id,
                            call.index as u32,
                            0,
                            0,
                            call.window.get(),
                            dc,
                            rect.left as u32,
                            rect.top as u32,
                            rect.right as u32,
                            rect.bottom as u32,
                            data,
                        ]) {
                            output.copy_from_slice(&value.to_le_bytes());
                        }
                        ram.write(call.scratch.address, &bytes)
                            .expect("draw output was preflighted before dc publication");
                        call.dc = Some(dc);
                        call.stage = Stage32::Send;
                    }
                }
            }
            Stage32::Send => {
                let message = self
                    .pending_status_draw_message(token)
                    .map_err(HostError::User32)?
                    .ok_or(HostError::InvalidRequest)?;
                let outcome = self
                    .send_message(&message, call.call)
                    .map_err(HostError::User32)?;
                call.child = Some(match outcome {
                    User32Outcome32::Callback(callback) => callback.continuation,
                    User32Outcome32::Wait { continuation } => continuation,
                    _ => return Err(HostError::InvalidRequest),
                });
                call.completed = None;
                self.publish_status_draw(token, call);
                return Ok(outcome);
            }
            Stage32::Next => {
                self.release_status_draw_dc(&mut call, gdi)?;
                call.index += 1;
                call.stage = if call.only {
                    Stage32::Finish
                } else {
                    Stage32::Scan
                };
            }
            Stage32::Finish => {
                self.release_status_draw_dc(&mut call, gdi)?;
                self.publish_status_draw(token, call);
                call.scratch.release(ram).map_err(HostError::User32)?;
                self.pending.pop();
                return Ok(User32Outcome32::Return(call.result));
            }
        }
        self.publish_status_draw(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }

    fn release_status_draw_dc(
        &self,
        call: &mut StatusDraw32,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), HostError> {
        if let Some(dc) = call.dc {
            if gdi
                .check_control_dc(dc, call.window.get(), call.call.thread_id)
                .is_ok()
            {
                gdi.release_control_dc(dc, call.window.get(), call.call.thread_id)
                    .map_err(HostError::Gdi)?;
            } else if self.windows().get(call.window).is_ok() {
                return Err(HostError::Gdi(
                    crate::windows::gdi::GdiError32::InvalidHandle,
                ));
            }
            call.dc = None;
        }
        Ok(())
    }

    pub(in crate::process::windows::user32) fn complete_status_draw_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        if !matches!(outcome, User32Outcome32::Return(_)) {
            return Ok(outcome);
        }
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let Continuation32::StatusDraw(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        if call.child != Some(child) {
            return Ok(outcome);
        }
        let token = pending.token;
        call.child = None;
        call.completed = Some(child);
        call.stage = Stage32::Next;
        self.publish_status_draw(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_status_draw_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::StatusDraw(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
