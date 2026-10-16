use crate::process::wasm::mutate;
pub(crate) fn begin_process_launch_input(total:u32)->u32 {mutate(|instance|instance.begin_process_launch_input(total))}
pub(crate) fn append_process_launch_input(offset:u32,length:u32)->u32 {mutate(|instance|{let bytes=super::input(instance,length)?.to_vec();instance.append_process_launch_input(offset,&bytes)})}
pub(crate) fn commit_process_launch_input()->u32 {mutate(|instance|instance.commit_process_launch_input())}
pub(crate) fn abort_process_launch_input()->u32 {mutate(|instance|instance.abort_process_launch_input())}
