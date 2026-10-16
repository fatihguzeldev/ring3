use super::{Remote32, context::Rundown32};
use crate::{
    process::{EngineInstance, HostError},
    windows::ole32::ApartmentId32,
};

pub(in crate::process::windows::ole32) struct PreparedComRundown32 {
    rows: Vec<(usize, Rundown32)>,
    next: u64,
    after: u64,
}

impl EngineInstance {
    pub(in crate::process::windows::ole32) fn prepare_com_rundown(
        &self,
        thread: u32,
        retiring: bool,
        after: u64,
    ) -> Result<PreparedComRundown32, HostError> {
        let Some(runtime) = self.ole32.as_ref() else {
            return Ok(PreparedComRundown32 {
                rows: Vec::new(),
                next: 0,
                after: 0,
            });
        };
        if after < runtime.activation.next {
            return Err(HostError::InvalidRequest);
        }
        let apartment = runtime.apartments.identity(thread).ok();
        let closes = match apartment {
            Some(ApartmentId32::Sta(_)) => true,
            Some(ApartmentId32::Mta) => !runtime
                .apartments
                .mta_threads()
                .any(|other| other != thread && !self.com_apartment_host(other)),
            _ => false,
        };
        let mut result = PreparedComRundown32 {
            rows: Vec::new(),
            next: after,
            after,
        };
        if !closes {
            return Ok(result);
        }
        let selected = |proxy: &super::context::Proxy32| {
            Some(proxy.client) == apartment
                && proxy.rundown.is_none()
                && proxy.owned.iter().any(|pointer| *pointer != 0)
                && self
                    .windows_thread_info(proxy.execution.thread)
                    .is_ok_and(|info| {
                        info.state != crate::windows::thread::ThreadState32::Terminated
                    })
                && (!retiring || proxy.execution.thread != thread)
        };
        let count = runtime
            .marshaling
            .proxies
            .iter()
            .flatten()
            .filter(|proxy| selected(proxy))
            .count();
        result
            .rows
            .try_reserve_exact(count)
            .map_err(|_| HostError::Infrastructure)?;
        result
            .next
            .checked_add(count as u64)
            .ok_or(HostError::Infrastructure)?;
        for (index, proxy) in runtime
            .marshaling
            .proxies
            .iter()
            .enumerate()
            .filter_map(|(index, proxy)| proxy.as_ref().map(|proxy| (index, proxy)))
        {
            if selected(proxy) {
                result.next += 1;
                result.rows.push((
                    index,
                    Rundown32 {
                        continuation: result.next,
                        sender: thread,
                        next: 0,
                        remote: Remote32::Idle,
                    },
                ));
            }
        }
        Ok(result)
    }

    pub(in crate::process::windows::ole32) fn commit_com_rundown(
        &mut self,
        prepared: PreparedComRundown32,
    ) {
        let Some(runtime) = self.ole32.as_mut() else {
            return;
        };
        assert_eq!(prepared.after, runtime.activation.next);
        let sender = prepared.rows.first().map(|(_, rundown)| rundown.sender);
        runtime.activation.next = prepared.next;
        for (index, rundown) in prepared.rows {
            let proxy = runtime.marshaling.proxies[index]
                .as_mut()
                .expect("rundown retains the exact proxy slot until commit");
            assert!(proxy.rundown.is_none());
            proxy.disconnected = true;
            proxy.rundown = Some(rundown);
        }
        if let Some(sender) = sender {
            self.wake_com_apartment_sender(sender);
        }
    }

    pub(in crate::process::windows::ole32) fn com_rundown_pending(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.marshaling.proxies.iter().flatten().any(|proxy| {
                proxy
                    .rundown
                    .is_some_and(|rundown| rundown.sender == thread)
            })
        })
    }

    pub(super) fn com_rundown_work_ready(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .marshaling
                .proxies
                .iter()
                .enumerate()
                .any(|(index, proxy)| {
                    proxy.as_ref().is_some_and(|proxy| {
                        proxy.execution.thread == thread
                            && proxy
                                .rundown
                                .is_some_and(|rundown| rundown.remote != Remote32::Queued)
                            && !runtime
                                .marshaling
                                .calls
                                .iter()
                                .any(|call| call.proxy == index)
                    })
                })
        })
    }

    pub(super) fn service_com_rundown(&mut self) -> Result<(), HostError> {
        let thread = self.threads.current_id();
        let Some(runtime) = self.ole32.as_ref() else {
            return Ok(());
        };
        let Some(index) =
            runtime
                .marshaling
                .proxies
                .iter()
                .enumerate()
                .position(|(index, proxy)| {
                    proxy.as_ref().is_some_and(|proxy| {
                        proxy.execution.thread == thread
                            && proxy
                                .rundown
                                .is_some_and(|rundown| rundown.remote != Remote32::Queued)
                            && !runtime
                                .marshaling
                                .calls
                                .iter()
                                .any(|call| call.proxy == index)
                    })
                })
        else {
            return Ok(());
        };
        let proxy = *runtime.marshaling.proxies[index].as_ref().unwrap();
        let mut rundown = proxy.rundown.unwrap();
        while rundown.next < proxy.owned.len() && proxy.owned[rundown.next] == 0 {
            rundown.next += 1;
        }
        if rundown.next == proxy.owned.len() {
            let runtime = self.ole32.as_mut().unwrap();
            if proxy.local_alive {
                runtime.marshaling.proxies[index].as_mut().unwrap().rundown = None;
            } else {
                runtime.marshaling.proxies[index] = None;
            }
            self.wake_com_apartment_sender(rundown.sender);
        } else {
            let pointer = proxy.owned[rundown.next];
            let procedure = self.com_guest_method(pointer, 2)?;
            self.queue_com_apartment_invocation(
                thread,
                procedure,
                &[pointer],
                rundown.sender,
                rundown.continuation,
            )?;
            rundown.remote = Remote32::Queued;
            self.ole32.as_mut().unwrap().marshaling.proxies[index]
                .as_mut()
                .unwrap()
                .rundown = Some(rundown);
        }
        Ok(())
    }
}
