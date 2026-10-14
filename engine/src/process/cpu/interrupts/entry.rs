use super::detection::DetectedEvent32;
use crate::{
    cpu::x86::{
        DetectedEntryResult32, EscalationClass32, EscalationDecision32, EventDeliveryResult32,
        EventOrigin32, ExceptionCause32, SecondaryClass32, escalate_exception,
    },
    process::{EngineInstance, HostError},
};

mod state;
use state::PreparedDetectionState32;

impl EngineInstance {
    /// consume one sealed detector record and route its event without repeating physical currentness.
    pub(in crate::process) fn deliver_detected_exception(
        &mut self,
        epoch: u64,
    ) -> Result<DetectedEntryResult32, HostError> {
        self.memory()?;
        let record = self.consume_exception_detection(epoch)?;
        let initial = match record.event() {
            DetectedEvent32::Exception(origin) => origin,
            DetectedEvent32::NeedsPagingAuthority { fault, length } => {
                // no certified event origin exists; preserve the existing diagnostic helper and stop.
                return Ok(DetectedEntryResult32::NeedsPagingAuthority {
                    initial: None,
                    origin: None,
                    fault,
                    length,
                });
            }
        };
        // encode both possible retirement images before any attempt can commit frame memory.
        let mut state = PreparedDetectionState32::new(&record, self.arena())?;
        let mut origin = initial;
        let mut input = record.input();
        let mut secondary_seen = false;
        loop {
            let prepared = self.attempt_detected_exception(&record, origin, input)?;
            match prepared.result {
                EventDeliveryResult32::Entered(entered) => {
                    state.entered(entered);
                    self.publish_detected_state(state);
                    self.publish_prepared_exception(prepared);
                    return Ok(DetectedEntryResult32::Entered {
                        initial,
                        origin,
                        entered,
                    });
                }
                EventDeliveryResult32::ArchitecturalSecondary(cause) => {
                    if !secondary_seen {
                        state.attempted_restart(&record);
                        input = state.input();
                        secondary_seen = true;
                    }
                    let secondary = match cause.escalation_class() {
                        EscalationClass32::Benign => SecondaryClass32::Benign,
                        EscalationClass32::Contributory => SecondaryClass32::Contributory,
                        EscalationClass32::PageFault => SecondaryClass32::PageFault,
                        // a double fault is generated only by this escalation owner, never the core.
                        EscalationClass32::DoubleFault => return Err(HostError::Infrastructure),
                    };
                    match escalate_exception(origin.escalation_class(), secondary) {
                        EscalationDecision32::DeliverSecondary => {
                            origin = EventOrigin32::Exception(cause);
                        }
                        EscalationDecision32::DeliverDoubleFault => {
                            // diagnostic abort image: same trusted failed-delivery input, rf preserved.
                            origin = EventOrigin32::Exception(ExceptionCause32::DoubleFault);
                        }
                        EscalationDecision32::Shutdown => {
                            self.publish_detected_state(state);
                            self.publish_prepared_exception(prepared);
                            // all final diagnostic copies precede the irreversible execution latch.
                            self.latch_exception_shutdown();
                            return Ok(DetectedEntryResult32::Shutdown {
                                initial,
                                secondary: cause,
                            });
                        }
                    }
                }
                result => {
                    if !secondary_seen {
                        if matches!(
                            result,
                            EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority { .. }
                        ) {
                            state.unattempted_restart(&record);
                        } else {
                            state.attempted_restart(&record);
                        }
                    }
                    let result = terminal_result(initial, origin, result);
                    self.publish_detected_state(state);
                    self.publish_prepared_exception(prepared);
                    return Ok(result);
                }
            }
        }
    }

    fn publish_detected_state(&mut self, state: PreparedDetectionState32) {
        state.publish(self.arena.as_mut().get_mut());
    }
}

fn terminal_result(
    initial: EventOrigin32,
    origin: EventOrigin32,
    result: EventDeliveryResult32,
) -> DetectedEntryResult32 {
    match result {
        EventDeliveryResult32::NeedsPagingAuthority { fault, length } => {
            DetectedEntryResult32::NeedsPagingAuthority {
                initial: Some(initial),
                origin: Some(origin),
                fault,
                length,
            }
        }
        EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority { kind, selector } => {
            DetectedEntryResult32::NeedsTaskOrPrivilegeAuthority {
                initial,
                origin,
                kind,
                selector,
            }
        }
        EventDeliveryResult32::Infrastructure(error) => DetectedEntryResult32::Infrastructure {
            initial,
            origin,
            error,
        },
        EventDeliveryResult32::Entered(_) | EventDeliveryResult32::ArchitecturalSecondary(_) => {
            unreachable!("entry and architectural secondary handled before terminal publication")
        }
    }
}

#[cfg(test)]
mod tests;
