use crate::{
    memory::{Access, GuestAddress, WordWrite32},
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, winmm::WinmmApi32},
};
const TIMERR_NOCANDO: u32 = 97;
impl EngineInstance {
    pub fn register_winmm_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::winmm::winmm_module(base))
    }
    pub fn multimedia_timer_period(&self) -> Result<Option<u32>, HostError> {
        self.memory()?;
        Ok(self.winmm.periods.minimum())
    }
    pub(in crate::process) fn try_complete_winmm_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = WinmmApi32::from_id(id) else {
            return Ok(false);
        };
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        let args = frame.arguments();
        let value = match api {
            WinmmApi32::TimeSetEvent => self.set_multimedia_timer(args)?,
            WinmmApi32::TimeKillEvent => {
                let Some(value) = self.kill_multimedia_timer(owner, token, frame)? else {
                    return Ok(true);
                };
                value
            }
            WinmmApi32::TimeGetTime => self.multimedia_milliseconds()?,
            WinmmApi32::TimeBeginPeriod => self.winmm.periods.begin(args[0]),
            WinmmApi32::TimeEndPeriod => self.winmm.periods.end(args[0]),
            WinmmApi32::TimeGetDevCaps => {
                if args[1] < 8 || !self.multimedia_output(args[0], 8) {
                    TIMERR_NOCANDO
                } else {
                    self.memory
                        .as_mut()
                        .unwrap()
                        .write_words32(&[word(args[0], 1), word(args[0] + 4, 65535)])
                        .map_err(HostError::Memory)?;
                    0
                }
            }
            WinmmApi32::TimeGetSystemTime => {
                if args[1] < 12 || !self.multimedia_output(args[0], 12) {
                    TIMERR_NOCANDO
                } else {
                    let millis = self.multimedia_milliseconds()?;
                    self.memory
                        .as_mut()
                        .unwrap()
                        .write_words32(&[word(args[0], 1), word(args[0] + 4, millis)])
                        .map_err(HostError::Memory)?;
                    0
                }
            }
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
    fn multimedia_output(&self, address: u32, length: usize) -> bool {
        address != 0
            && self.memory.as_ref().is_some_and(|memory| {
                memory
                    .check_span(GuestAddress(address), length, Access::Write)
                    .is_ok()
            })
    }
    pub(in crate::process) fn multimedia_milliseconds(&self) -> Result<u32, HostError> {
        let (origin, now) = self.cpu_clock.monotonic_bounds()?;
        Ok(((now - origin) / 1_000_000) as u32)
    }
}
fn word(address: u32, value: u32) -> WordWrite32 {
    WordWrite32 {
        address: GuestAddress(address),
        value,
    }
}
