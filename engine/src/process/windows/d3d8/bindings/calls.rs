use super::super::{
    D3D8Runtime32, DispatchError32,
    lifetime::LifetimeChange32,
    memory::{check_output, word},
};
use crate::{
    graphics::d3d8::{api::HResult, state::StateBlockKind},
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn binding_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        match slot {
            52 => self.device_mut(pointer)?.begin_state_block()?,
            53 => {
                check_output(memory, args[1], 4)?;
                let prepared = self.device(pointer)?.prepare_end_state_block()?;
                self.device(pointer)?.can_commit_state_block(&prepared)?;
                self.change_lifetime(
                    memory,
                    LifetimeChange32::default(),
                    &[word(args[1], prepared.handle())],
                )?;
                self.device_mut(pointer)?.commit_state_block(prepared).expect("exclusive device state-block preparation remains current after guest publication");
            }
            54..=56 => {
                let prepared = match slot {
                    54 => self.device(pointer)?.prepare_apply_state_block(args[1])?,
                    55 => self.device(pointer)?.prepare_capture_state_block(args[1])?,
                    _ => self.device(pointer)?.prepare_delete_state_block(args[1])?,
                };
                let delta = self.binding_hold_delta(
                    pointer,
                    prepared.released_resources().iter().copied(),
                    prepared.retained_resources().iter().copied(),
                )?;
                self.device(pointer)?.can_commit_block_change(&prepared)?;
                self.commit_binding_holds(memory, pointer, delta, &[])?;
                self.device_mut(pointer)?.commit_block_change(prepared).expect("exclusive device state-block preparation remains current after COM lifetime commit");
            }
            57 => {
                check_output(memory, args[2], 4)?;
                let kind = match args[1] {
                    1 => StateBlockKind::All,
                    2 => StateBlockKind::Pixel,
                    3 => StateBlockKind::Vertex,
                    _ => return Err(HResult::INVALID_CALL.into()),
                };
                let prepared = self.device(pointer)?.prepare_state_block(kind)?;
                let delta =
                    self.binding_hold_delta(pointer, std::iter::empty(), prepared.resources())?;
                self.device(pointer)?.can_commit_state_block(&prepared)?;
                self.commit_binding_holds(
                    memory,
                    pointer,
                    delta,
                    &[word(args[2], prepared.handle())],
                )?;
                self.device_mut(pointer)?.commit_state_block(prepared).expect("exclusive device state-block preparation remains current after guest publication");
            }
            60 | 61 | 83 | 84 | 85 | 86 => {
                return self.resource_binding_call(memory, pointer, slot, args);
            }
            _ => return Ok(None),
        }
        Ok(Some(0))
    }
}
