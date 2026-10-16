use crate::{
    abi::{arena::TRANSFER_OFFSET, header},
    process::wasm::mutate,
    windows::thread::{ThreadLimits32, ThreadSchedule32},
};
pub(crate) fn configure_threads(
    threads: u32,
    handles: u32,
    apcs: u32,
    stack_pages: u32,
    stack_reserve_pages: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_threads(ThreadLimits32 {
            threads,
            handles,
            apcs,
            stack_pages,
            stack_reserve_pages,
        })
    })
}
pub(crate) fn schedule_windows_thread() -> u32 {
    mutate(|instance| {
        let value = instance.schedule_windows_thread()?;
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 40];
        header::write(output, *b"R3TS");
        match value {
            ThreadSchedule32::CpuEvent {thread}=>{header::write_u32(output,12,5);header::write_u32(output,16,thread);}
            ThreadSchedule32::WindowsCallback { thread } => {header::write_u32(output,12,4);header::write_u32(output,16,thread);}
        ThreadSchedule32::Running { thread } => {
                header::write_u32(output, 12, 1);
                header::write_u32(output, 16, thread);
            }
            ThreadSchedule32::Waiting { deadline_ns } => {
                header::write_u32(output, 12, 2);
                if let Some(deadline) = deadline_ns {
                    output[24..32].copy_from_slice(&deadline.to_le_bytes());
                    header::write_u32(output, 32, 1);
                }
            }
            ThreadSchedule32::Exited { code } => {
                header::write_u32(output, 12, 3);
                header::write_u32(output, 20, code);
            }
        }
        Ok(())
    })
}

pub(crate) fn report_windows_thread_execution(el:u32,eh:u32,dl:u32,dh:u32)->u32 {mutate(|instance|instance.report_windows_thread_execution(super::wide(el,eh),super::wide(dl,dh)))}

pub(crate) fn pending_windows_thread_schedule()->u32 {crate::process::wasm::inspect(|instance|instance.pending_windows_thread_schedule().unwrap_or(0))}
