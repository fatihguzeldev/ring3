use super::super::{
    User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32,
};
use crate::{
    memory::AddressSpace,
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::CommonControlState32,
        gdi::GdiContext32,
        user32::{User32Api32, User32Error32 as E},
        window::WindowHandle32,
    },
};

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct CommonLayout32 {
    pub child: Option<u64>,
    pub completed: Option<u64>,
}

impl User32Context32 {
    pub(super) fn begin_status_layout(
        &mut self,
        window: WindowHandle32,
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
        call: User32CallContext32,
        font_override: Option<u32>,
    ) -> Result<User32Outcome32, HostError> {
        let data = self.state.record(window).map_err(HostError::User32)?;
        if data.style & 4 != 0 {
            return Ok(User32Outcome32::Return(0));
        }
        if data.style & 0x80 != 0 {
            return Err(HostError::User32(E::Win32(50)));
        }
        let CommonControlState32::Status(status) =
            self.state.common_state(window).map_err(HostError::User32)?
        else {
            return Err(HostError::InvalidRequest);
        };
        let selected = font_override.unwrap_or(status.font);
        let font = if selected == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            selected
        };
        let metrics = gdi.font_metrics(font).map_err(HostError::Gdi)?;
        let height = metrics
            .height
            .max(status.minimum_height)
            .checked_add(4)
            .ok_or(HostError::User32(E::Capacity))?;
        let parent = data.parent.ok_or(HostError::User32(E::Win32(1406)))?;
        let parent_extent = self
            .windows()
            .get(parent)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent;
        let current = self
            .state
            .client_position(window)
            .map_err(HostError::User32)?;
        let x = if data.style & 8 != 0 { current.x } else { 0 };
        let y = if data.style & (8 | 2) != 0 {
            current.y
        } else if data.style & 3 == 1 {
            0
        } else {
            (parent_extent.height as i32).saturating_sub(height)
        };
        let width = parent_extent.width;
        let extent = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent;
        if current.x == x
            && current.y == y
            && extent.width == width
            && extent.height == height as u32
        {
            return Ok(User32Outcome32::Return(0));
        }
        self.prepare_thread_slots().map_err(HostError::User32)?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(HostError::User32(E::Capacity));
        }
        self.next_token
            .checked_add(2)
            .ok_or(HostError::User32(E::Capacity))?;
        let position = self
            .prepare_window_position(
                User32Api32::SetWindowPos,
                &[
                    window.get(),
                    0,
                    x as u32,
                    y as u32,
                    width,
                    height as u32,
                    0x14,
                ],
                ram,
                call,
            )
            .map_err(HostError::User32)?;
        let User32Outcome32::Wait {
            continuation: parent,
        } = self
            .start(Continuation32::CommonLayout(CommonLayout32 {
                child: None,
                completed: None,
            }))
            .expect("layout pair was preflighted")
        else {
            unreachable!()
        };
        let outcome = self
            .start(position)
            .expect("position was prepared before parent publication");
        let child = match outcome {
            User32Outcome32::Callback(callback) => callback.continuation,
            User32Outcome32::Wait { continuation } => continuation,
            _ => unreachable!(),
        };
        let pending = self
            .pending
            .iter_mut()
            .find(|pending| pending.token == parent)
            .unwrap();
        pending.continuation = Continuation32::CommonLayout(CommonLayout32 {
            child: Some(child),
            completed: None,
        });
        Ok(outcome)
    }

    pub(in crate::process::windows::user32) fn complete_common_layout_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        if !matches!(outcome, User32Outcome32::Return(_)) {
            return Ok(outcome);
        }
        let Some(pending) = self.pending.last_mut() else {
            return Ok(outcome);
        };
        let Continuation32::CommonLayout(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        if call.child != Some(child) {
            return Ok(outcome);
        }
        call.child = None;
        call.completed = Some(child);
        pending.continuation = Continuation32::CommonLayout(call);
        Ok(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }

    pub(in crate::process::windows::user32) fn retry_common_layout_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::CommonLayout(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }

    pub(in crate::process::windows::user32) fn resume_common_layout(
        &mut self,
        token: u64,
    ) -> Result<User32Outcome32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::CommonLayout(call) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        if call.child.is_some() || call.completed.is_none() {
            return Err(E::InvalidToken);
        }
        self.pending.pop();
        Ok(User32Outcome32::Return(0))
    }
}
