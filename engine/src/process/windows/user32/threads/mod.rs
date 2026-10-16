mod layouts;
mod queue;
mod replies;
mod sending;
pub(in crate::process::windows::user32) use layouts::ThreadLayout32;

pub(in crate::process) use queue::PreparedUser32Sent32;
pub(in crate::process) use replies::User32ThreadReply32;

mod termination;
