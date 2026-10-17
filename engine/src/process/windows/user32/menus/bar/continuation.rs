use super::types::*;
use crate::{
    memory::AddressSpace,
    process::windows::{
        gdi::GdiRuntime32,
        user32::{User32Context32, User32Outcome32, callback::Continuation32, memory},
    },
    windows::user32::{Rect32, User32Error32 as E},
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn check_menu_bar_scratch(
        &self,
        ram: &AddressSpace,
    ) -> Result<(), E> {
        for pending in &self.pending {
            if let Continuation32::MenuBar(call) = pending.continuation {
                call.check(ram)?;
            }
        }
        Ok(())
    }

    pub(in crate::process::windows::user32) fn menu_bar_wait(
        &self,
        token: u64,
    ) -> Option<MenuBar32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        let Continuation32::MenuBar(call) = pending.continuation else {
            return None;
        };
        Some(call)
    }

    pub(in crate::process::windows::user32) fn pending_menu_bar_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        let pointer = call.scratch.address.0;
        let message = match call.stage {
            Stage32::Changing => [0x46, 0, pointer],
            Stage32::NonClient => [0x83, 1, pointer + 32],
            Stage32::NonClientPaint => [0x85, 1, 0],
            Stage32::Changed => [0x47, 0, pointer],
            Stage32::MeasureCall(_) => [0x2c, 0, pointer + 96],
            Stage32::DrawCall(_) => [0x2b, 0, pointer + 64],
            _ => return Ok(None),
        };
        Ok(Some([
            call.window.get(),
            message[0],
            message[1],
            message[2],
        ]))
    }

    pub(in crate::process::windows::user32) fn resume_menu_bar(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, E> {
        for step in 0..32 {
            let call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
            call.scratch.check(ram)?;
            if call.child.is_some() {
                return Err(E::InvalidToken);
            }
            if !self
                .state
                .record(call.window)
                .is_ok_and(|data| !data.destroying && data.menu == call.menu)
            {
                self.finish_menu_bar_dc(call, gdi)?;
                call.scratch.release(ram)?;
                self.pending.pop();
                return Ok(User32Outcome32::Return(u32::from(matches!(
                    call.purpose,
                    Purpose32::Frame
                ))));
            }
            if let Purpose32::NonClient { output, .. } = call.purpose {
                output.check(ram)?;
            }
            if let Some(message) = self.pending_menu_bar_message(token)? {
                if step != 0 {
                    // outgoing sends need the next engine preflight before creating their child.
                    return Ok(User32Outcome32::Wait {
                        continuation: token,
                    });
                }
                let outcome = self.send_message(&message, call.call)?;
                let mut next = call;
                next.child = Some(match outcome {
                    User32Outcome32::Callback(callback) => callback.continuation,
                    User32Outcome32::Wait { continuation } => continuation,
                    User32Outcome32::Return(_) => return Err(E::InvalidRequest),
                });
                next.completed = None;
                self.publish_menu_bar(token, next);
                return Ok(outcome);
            }
            match call.stage {
                Stage32::PrepareNonClient => {
                    let [hwnd, _, x, y, width, height, flags] =
                        memory::words::<7>(ram, call.scratch.address.0)?;
                    if hwnd != call.window.get() {
                        return Err(E::InvalidRequest);
                    }
                    super::super::super::lifecycle::check_position_flags(flags)?;
                    let outer = self.position_rect(call.window, x, y, width, height, flags)?;
                    let old = self.state.record(call.window)?;
                    let description = self.windows().get(call.window)?.description;
                    let client = Rect32 {
                        left: old.client_origin.x,
                        top: old.client_origin.y,
                        right: old
                            .client_origin
                            .x
                            .checked_add(description.extent.width as i32)
                            .ok_or(E::Capacity)?,
                        bottom: old
                            .client_origin
                            .y
                            .checked_add(description.extent.height as i32)
                            .ok_or(E::Capacity)?,
                    };
                    let mut bytes = [0; 48];
                    for (output, value) in bytes.chunks_exact_mut(4).zip(
                        [outer, old.outer, client]
                            .into_iter()
                            .flat_map(|rect| [rect.left, rect.top, rect.right, rect.bottom]),
                    ) {
                        output.copy_from_slice(&value.to_le_bytes());
                    }
                    ram.write(
                        crate::memory::GuestAddress(call.scratch.address.0 + 32),
                        &bytes,
                    )?;
                    self.menu_bar_stage(token, Stage32::NonClient)?;
                }
                Stage32::Publish => {
                    let client = memory::rect(ram, call.scratch.address.0 + 32)?;
                    let words = memory::words::<7>(ram, call.scratch.address.0)?;
                    let requested = self.position_rect(
                        call.window,
                        words[2],
                        words[3],
                        words[4],
                        words[5],
                        words[6],
                    )?;
                    let old = self.state.record(call.window)?;
                    let description = self.windows().get(call.window)?.description;
                    let mut flags = words[6] & !0x1800;
                    if old.client_origin.x == client.left && old.client_origin.y == client.top {
                        flags |= 0x1000;
                    }
                    if description.extent.width == client.extent()?.width
                        && description.extent.height == client.extent()?.height
                    {
                        flags |= 0x800;
                    }
                    ram.write(
                        crate::memory::GuestAddress(call.scratch.address.0 + 24),
                        &flags.to_le_bytes(),
                    )?;
                    self.publish_position(call.window, requested, client, flags)?;
                    self.menu_bar_stage(token, Stage32::NonClientPaint)?;
                    return Ok(User32Outcome32::Wait {
                        continuation: token,
                    });
                }
                Stage32::Measure(position) => {
                    self.prepare_menu_bar_measure(token, position, ram, gdi)?
                }
                Stage32::MeasureRead(position) => {
                    self.accept_menu_bar_measure(token, position, ram)?
                }
                Stage32::Layout => self.layout_menu_bar(token, gdi)?,
                Stage32::Paint => self.paint_menu_bar(token, ram, gdi)?,
                Stage32::Draw(position) => self.draw_menu_bar_item(token, position, ram, gdi)?,
                Stage32::DrawFinish(position) => {
                    self.finish_menu_bar_dc(call, gdi)?;
                    let mut next = call;
                    next.dc = None;
                    next.stage = Stage32::Draw(position + 1);
                    self.publish_menu_bar(token, next);
                }
                Stage32::Finish => {
                    let result = if let Purpose32::NonClient { output, mut base } = call.purpose {
                        output.check(ram)?;
                        let height = self
                            .state
                            .menu_bar_cache(call.window)?
                            .and_then(|cache| cache.layout.as_ref())
                            .map_or(0, |layout| layout.extent.y + 1);
                        base.top = base.top.saturating_add(height).min(base.bottom);
                        let values = [base.left, base.top, base.right, base.bottom];
                        let writes =
                            std::array::from_fn::<_, 4, _>(|index| crate::memory::WordWrite32 {
                                address: crate::memory::GuestAddress(
                                    output.address().0 + index as u32 * 4,
                                ),
                                value: values[index] as u32,
                            });
                        ram.unmap_ranges_with_word_writes(&[call.scratch.range()], &writes)?;
                        0
                    } else {
                        call.scratch.release(ram)?;
                        u32::from(matches!(call.purpose, Purpose32::Frame))
                    };
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(result));
                }
                _ => return Err(E::InvalidRequest),
            }
        }
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }

    pub(super) fn menu_bar_stage(&mut self, token: u64, stage: Stage32) -> Result<(), E> {
        let mut call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
        call.stage = stage;
        self.publish_menu_bar(token, call);
        Ok(())
    }

    pub(super) fn publish_menu_bar(&mut self, token: u64, call: MenuBar32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("accepted menu bar continuation")
            .continuation = Continuation32::MenuBar(call);
    }

    pub(in crate::process::windows::user32) fn complete_menu_bar_child(
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
        let Continuation32::MenuBar(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        if call.child != Some(child) {
            return Ok(outcome);
        }
        call.child = None;
        call.completed = Some(child);
        call.stage = match call.stage {
            Stage32::Changing => Stage32::PrepareNonClient,
            Stage32::NonClient => Stage32::Publish,
            Stage32::NonClientPaint => Stage32::Changed,
            Stage32::Changed => Stage32::Finish,
            Stage32::MeasureCall(position) => Stage32::MeasureRead(position),
            Stage32::DrawCall(position) => Stage32::DrawFinish(position),
            _ => return Err(E::InvalidToken),
        };
        let token = pending.token;
        self.publish_menu_bar(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }

    pub(in crate::process::windows::user32) fn retry_menu_bar_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::MenuBar(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
