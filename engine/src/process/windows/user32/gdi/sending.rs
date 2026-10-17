use super::{Continuation32, PaintOutput32, User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    process::windows::user32::User32CallContext32,
    windows::{user32::User32Error32 as Error, window::WindowHandle32},
};
#[derive(Clone, Copy)]
enum PaintSendKind32 {
    Begin { output: PaintOutput32, dc: u32 },
    Update,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct SentPaint32 {
    window: WindowHandle32,
    thread: u32,
    kind: PaintSendKind32,
    child: Option<u64>,
    completed: Option<u64>,
    result: Option<u32>,
}
impl SentPaint32 {
    pub(in crate::process::windows::user32) fn check_output(
        self,
        memory: &AddressSpace,
    ) -> Result<(), Error> {
        if let PaintSendKind32::Begin { output, .. } = self.kind {
            output.check(memory)?;
        }
        Ok(())
    }
}
impl User32Context32 {
    pub(super) fn check_sent_paint_capacity(&self) -> Result<(), Error> {
        self.next_token.checked_add(2).ok_or(Error::Capacity)?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(Error::Capacity);
        }
        Ok(())
    }
    pub(super) fn start_sent_paint(
        &mut self,
        window: WindowHandle32,
        thread: u32,
        begin: Option<(PaintOutput32, u32)>,
    ) -> Result<User32Outcome32, Error> {
        self.start(Continuation32::SentPaint(SentPaint32 {
            window,
            thread,
            kind: match begin {
                Some((output, dc)) => PaintSendKind32::Begin { output, dc },
                None => PaintSendKind32::Update,
            },
            child: None,
            completed: None,
            result: None,
        }))
    }
    pub(in crate::process::windows::user32) fn sent_paint_wait(
        &self,
        token: u64,
    ) -> Option<SentPaint32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        let Continuation32::SentPaint(paint) = pending.continuation else {
            return None;
        };
        Some(paint)
    }
    pub(in crate::process::windows::user32) fn pending_sent_paint_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, Error> {
        let paint = self.sent_paint_wait(token).ok_or(Error::InvalidToken)?;
        if paint.child.is_some()
            || paint.result.is_some()
            || self.windows().get(paint.window).is_err()
        {
            return Ok(None);
        }
        Ok(Some(match paint.kind {
            PaintSendKind32::Begin { dc, .. } => [paint.window.get(), 0x14, dc, 0],
            PaintSendKind32::Update => [paint.window.get(), 0x0f, 0, 0],
        }))
    }
    pub(in crate::process::windows::user32) fn resume_sent_paint(
        &mut self,
        token: u64,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, Error> {
        let mut paint = self.sent_paint_wait(token).ok_or(Error::InvalidToken)?;
        if self.windows().get(paint.window).is_err() {
            self.last_error = Some(1400);
            self.pending.pop();
            return Ok(User32Outcome32::Return(0));
        }
        if let Some(result) = paint.result {
            let value = match paint.kind {
                PaintSendKind32::Begin { output, dc } => {
                    output.complete(memory, result != 0)?;
                    dc
                }
                PaintSendKind32::Update => 1,
            };
            self.pending.pop();
            return Ok(User32Outcome32::Return(value));
        }
        let message = self
            .pending_sent_paint_message(token)?
            .ok_or(Error::InvalidToken)?;
        let outcome = self.send_message(
            &message,
            User32CallContext32 {
                thread_id: paint.thread,
                ..self.runtime_input
            },
        )?;
        paint.child = Some(match outcome {
            User32Outcome32::Callback(callback) => callback.continuation,
            User32Outcome32::Wait { continuation } => continuation,
            User32Outcome32::Return(_) => return Err(Error::InvalidRequest),
        });
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .ok_or(Error::InvalidToken)?
            .continuation = Continuation32::SentPaint(paint);
        Ok(outcome)
    }
    pub(in crate::process::windows::user32) fn complete_sent_paint_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, Error> {
        let User32Outcome32::Return(result) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last_mut() else {
            return Ok(outcome);
        };
        let Continuation32::SentPaint(mut paint) = pending.continuation else {
            return Ok(outcome);
        };
        if paint.child != Some(child) {
            return Ok(outcome);
        }
        paint.child = None;
        paint.completed = Some(child);
        paint.result = Some(result);
        pending.continuation = Continuation32::SentPaint(paint);
        Ok(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_sent_paint_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::SentPaint(paint) = pending.continuation else {
            return None;
        };
        (paint.child.is_none() && paint.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
