use super::context::{NativeReference32, TransferPhase32 as P};
use crate::{
    process::{EngineInstance, HostError, windows::ole32::marshaling::Remote32},
    windows::{com::ServerThreadingModel32, ole32::HResult32},
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn accept_com_transfer_result(
        &mut self,
        index: usize,
        result: u32,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
        let phase = call.phase;
        if let P::ProxyStub(broker) = phase {
            return self.accept_registered_ps_result(index, broker, result);
        }
        if (call.abandoned || call.cancelled) && !matches!(phase, P::Release(_) | P::DiscardPacket)
        {
            if result & 0x8000_0000 == 0 {
                let field = match phase {
                    P::QueryMarshal => Some((1, 48, call.source)),
                    P::Activate => Some((2, 56, call.client)),
                    P::Unmarshal => Some((3, 60, call.client)),
                    _ => None,
                };
                if let Some((field, offset, execution)) = field {
                    let pointer = if matches!(execution.apartment, crate::windows::ole32::ApartmentId32::Sta(thread) if call.dead_sta.contains(&thread))
                    {
                        0
                    } else {
                        self.ole32_read_word(call.scratch().address.0 + offset)?
                    };
                    self.ole32.as_mut().unwrap().marshaling.transfers[index].references[field] =
                        NativeReference32 { pointer, execution };
                }
                let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
                if phase == P::Marshal
                    && !matches!(call.source.apartment, crate::windows::ole32::ApartmentId32::Sta(thread) if call.dead_sta.contains(&thread))
                {
                    call.packet = true;
                }
                if phase == P::Unmarshal {
                    self.ole32.as_mut().unwrap().marshaling.transfers[index].packet = false;
                }
            }
            return self.fail_com_transfer(index, 0x8001_0108);
        }
        if let P::Release(next) = phase {
            let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
            call.references[next].pointer = 0;
            call.phase = P::Release(next + 1);
            call.remote = Remote32::Idle;
            return self.advance_com_transfer(index);
        }
        if phase == P::DiscardPacket {
            let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
            call.packet = false;
            call.phase = P::Release(0);
            call.remote = Remote32::Idle;
            return self.advance_com_transfer(index);
        }
        if result & 0x8000_0000 != 0 {
            if phase == P::QueryMarshal
                && result == HResult32::NO_INTERFACE.0
                && self
                    .registered_com_proxy_class(call.requested)
                    .map_err(|_| HostError::InvalidRequest)?
                    .is_some()
            {
                let class = self
                    .registered_com_proxy_class(call.requested)
                    .map_err(|_| HostError::InvalidRequest)?
                    .unwrap();
                return self.begin_registered_com_transfer(index, class);
            }
            return self.fail_com_transfer(index, result);
        }
        let scratch = call.scratch().address.0;
        let next = match phase {
            P::QueryMarshal => {
                let pointer = self.ole32_read_word(scratch + 48)?;
                if pointer == 0 {
                    return self.fail_com_transfer(index, HResult32::UNEXPECTED.0);
                }
                let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
                call.references[1] = NativeReference32 {
                    pointer,
                    execution: call.source,
                };
                P::Class
            }
            P::Class => {
                let class = super::super::super::memory::iid(self.memory()?, scratch + 32)
                    .map_err(|_| HostError::InvalidRequest)?;
                let resolution = match self.com_activation_redirection_server(class)? {
                    Some(server) => Ok(Some(server)),
                    None => self.registered_com_server(class),
                };
                let server = match resolution {
                    Ok(Some(server)) => server,
                    Ok(None) => {
                        return self.fail_com_transfer(index, HResult32::CLASS_NOT_REGISTERED.0);
                    }
                    Err(error) => return self.fail_com_transfer(index, error.0),
                };
                if server.threading != ServerThreadingModel32::Both {
                    return self.fail_com_transfer(index, HResult32::INVALID_ARGUMENT.0);
                }
                P::Size
            }
            P::Size => {
                let maximum = self.ole32_read_word(scratch + 52)? as usize;
                if maximum > self.ole32.as_ref().unwrap().streams.store.buffer_limit() {
                    return self.fail_com_transfer(index, HResult32::OUT_OF_MEMORY.0);
                }
                let stream = self.create_com_stream(maximum, &[])?;
                self.ole32.as_mut().unwrap().marshaling.transfers[index].stream = Some(stream);
                P::Marshal
            }
            P::Marshal => {
                self.rewind_transfer_stream(index)?;
                self.ole32.as_mut().unwrap().marshaling.transfers[index].packet = true;
                P::Activate
            }
            P::Activate => {
                let pointer = self.ole32_read_word(scratch + 56)?;
                if pointer == 0 {
                    return self.fail_com_transfer(index, HResult32::UNEXPECTED.0);
                }
                let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
                call.references[2] = NativeReference32 {
                    pointer,
                    execution: call.client,
                };
                P::Unmarshal
            }
            P::Unmarshal => {
                let pointer = self.ole32_read_word(scratch + 60)?;
                if pointer == 0 {
                    return self.fail_com_transfer(index, HResult32::UNEXPECTED.0);
                }
                let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
                call.references[3] = NativeReference32 {
                    pointer,
                    execution: call.client,
                };
                call.output = pointer;
                call.packet = false;
                call.result = result;
                P::Release(0)
            }
            P::Release(_) | P::ProxyStub(_) | P::DiscardPacket | P::Complete => unreachable!(),
        };
        let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
        call.phase = next;
        call.remote = Remote32::Idle;
        self.advance_com_transfer(index)
    }
    pub(super) fn fail_com_transfer(&mut self, index: usize, result: u32) -> Result<(), HostError> {
        let packet = self.ole32.as_ref().unwrap().marshaling.transfers[index].packet;
        if packet {
            self.rewind_transfer_stream(index)?;
        }
        let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
        call.result = result;
        call.output = 0;
        call.phase = if packet {
            P::DiscardPacket
        } else {
            P::Release(0)
        };
        call.remote = Remote32::Idle;
        self.advance_com_transfer(index)
    }
}
