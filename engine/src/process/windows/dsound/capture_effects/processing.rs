use crate::{
    audio::{AudioContext, capture_effects::AecParameters, dsound::HResult},
    process::windows::dsound::objects::CaptureBuffer32,
};

impl CaptureBuffer32 {
    pub(in crate::process::windows::dsound) fn process_capture(
        &mut self,
        audio: &AudioContext,
        first: u64,
        bytes: &mut [u8],
        parameters: AecParameters,
        noise: bool,
        playing: bool,
    ) {
        let Some(effects) = &mut self.effects else {
            return;
        };
        let capture_rate = self.native.format.pcm.sample_rate;
        let render_rate = audio.config().sample_rate;
        effects.process(bytes, parameters, noise, |index| {
            if !playing {
                return None;
            }
            let at = u128::from(first)
                .checked_add(index as u128)?
                .checked_mul(u128::from(render_rate))?
                / u128::from(capture_rate);
            let frame = self.reference_origin.checked_add(u64::try_from(at).ok()?)?;
            audio.capture_reference_sample(frame)
        });
    }
    pub(in crate::process::windows::dsound) fn prepare_capture_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<Option<Vec<u8>>, HResult> {
        if self.effects.is_none() {
            return Ok(None);
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        owned.extend_from_slice(bytes);
        Ok(Some(owned))
    }
}
