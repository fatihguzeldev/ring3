use super::{
    ALL_PARAMETERS, EffectKind32, EffectParameters32, HapticCapabilities32, REQUIRED_PARAMETERS,
};
use crate::input::dinput::DirectInputResult32 as H;

pub struct EffectDevice32 {
    pub(super) capabilities: HapticCapabilities32,
    pub(super) effects: Vec<Effect32>,
    pub(super) sequence: u64,
    pub(super) gain: u32,
    pub(super) autocenter: bool,
    pub(super) paused: Option<u64>,
    pub(super) actuators: bool,
}
pub(super) struct Effect32 {
    pub id: u64,
    pub kind: EffectKind32,
    pub parameters: EffectParameters32,
    pub initialized: u32,
    pub downloaded: Option<EffectParameters32>,
    pub playing: Option<Playback32>,
    pub armed: bool,
    pub previous_trigger: bool,
    pub last_trigger: Option<u64>,
}
#[derive(Clone, Copy)]
pub(super) struct Playback32 {
    pub start: u64,
    pub iterations: u32,
    pub triggered: bool,
}
pub struct PreparedEffect32 {
    sequence: u64,
    effect: Effect32,
}
impl PreparedEffect32 {
    pub fn id(&self) -> u64 {
        self.effect.id
    }
}

impl EffectDevice32 {
    pub fn new(capabilities: HapticCapabilities32) -> Result<Self, H> {
        capabilities.validate()?;
        let mut effects = Vec::new();
        effects
            .try_reserve_exact(capabilities.max_effects as usize)
            .map_err(|_| H::OUT_OF_MEMORY)?;
        Ok(Self {
            capabilities,
            effects,
            sequence: 0,
            gain: 10000,
            autocenter: true,
            paused: None,
            actuators: true,
        })
    }
    pub fn capabilities(&self) -> HapticCapabilities32 {
        self.capabilities
    }
    pub fn len(&self) -> usize {
        self.effects.len()
    }
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
    pub fn load(&self) -> u32 {
        self.effects
            .iter()
            .filter(|effect| effect.downloaded.is_some())
            .count() as u32
            * 100
            / u32::from(self.capabilities.max_effects)
    }
    pub fn gain(&self) -> u32 {
        self.gain
    }
    pub fn autocenter(&self) -> bool {
        self.autocenter
    }
    pub fn set_gain(&mut self, gain: u32) -> Result<(), H> {
        if gain > 10000 {
            return Err(H::INVALID_PARAM);
        }
        self.gain = gain;
        Ok(())
    }
    pub fn set_autocenter(&mut self, enabled: bool) {
        self.autocenter = enabled;
    }
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.effects.iter().map(|effect| effect.id)
    }
    pub fn kind(&self, id: u64) -> Result<EffectKind32, H> {
        Ok(self.effect(id)?.kind)
    }
    pub fn parameters(&self, id: u64) -> Result<&EffectParameters32, H> {
        Ok(&self.effect(id)?.parameters)
    }
    pub fn initialized(&self, id: u64) -> Result<u32, H> {
        Ok(self.effect(id)?.initialized)
    }
    pub fn prepare_effect(
        &self,
        kind: EffectKind32,
        parameters: Option<EffectParameters32>,
        exclusive: bool,
    ) -> Result<PreparedEffect32, H> {
        if self.effects.len() == self.capabilities.max_effects as usize {
            return Err(H::DEVICE_FULL);
        }
        let id = self
            .sequence
            .checked_add(1)
            .filter(|id| *id != u64::MAX)
            .ok_or(H::DEVICE_FULL)?;
        let initialized = if parameters.is_some() {
            ALL_PARAMETERS
        } else {
            0
        };
        let parameters = parameters.unwrap_or_else(|| EffectParameters32::empty(kind));
        parameters.validate(kind, self.capabilities, initialized)?;
        let downloaded = exclusive
            .then(|| parameters.clone())
            .filter(|_| initialized & REQUIRED_PARAMETERS == REQUIRED_PARAMETERS);
        let armed = downloaded
            .as_ref()
            .is_some_and(|parameters| parameters.trigger.is_some());
        Ok(PreparedEffect32 {
            sequence: self.sequence,
            effect: Effect32 {
                id,
                kind,
                downloaded,
                parameters,
                initialized,
                playing: None,
                armed,
                previous_trigger: false,
                last_trigger: None,
            },
        })
    }
    pub fn commit_effect(&mut self, prepared: PreparedEffect32) {
        assert_eq!(
            self.sequence, prepared.sequence,
            "effect publication authority changed"
        );
        self.sequence = prepared.effect.id;
        self.effects.push(prepared.effect);
    }
    pub fn delete(&mut self, id: u64) -> Result<(), H> {
        let index = self
            .effects
            .iter()
            .position(|effect| effect.id == id)
            .ok_or(H::OBJECT_NOT_FOUND)?;
        self.effects.swap_remove(index);
        Ok(())
    }
    pub fn initialize(&mut self, id: u64, kind: EffectKind32) -> Result<(), H> {
        let effect = self.effect_mut(id)?;
        if effect.kind != kind {
            return Err(H::INVALID_PARAM);
        }
        effect.parameters = EffectParameters32::empty(kind);
        effect.initialized = 0;
        effect.downloaded = None;
        effect.playing = None;
        effect.armed = false;
        Ok(())
    }
    pub fn set_parameters(
        &mut self,
        id: u64,
        next: EffectParameters32,
        flags: u32,
        exclusive: bool,
        now: u64,
    ) -> Result<u32, H> {
        let now = self.paused.unwrap_or(now);
        let controls = flags & 0xe0000000;
        if flags & !(ALL_PARAMETERS | 0xe0000000) != 0 || controls.count_ones() > 1 {
            return Err(H::INVALID_PARAM);
        }
        let mask = flags & ALL_PARAMETERS;
        let effect = self.effect(id)?;
        let parameters = effect.parameters.merge(&next, mask);
        let initialized = effect.initialized | mask;
        if mask & 32 != 0
            && effect.initialized & 32 != 0
            && (parameters.axes != effect.parameters.axes
                || parameters.axis_count != effect.parameters.axis_count)
        {
            return Err(H::INVALID_PARAM);
        }
        parameters.validate(effect.kind, self.capabilities, initialized)?;
        let complete = initialized & REQUIRED_PARAMETERS == REQUIRED_PARAMETERS;
        if controls != 0x80000000 && !complete {
            return Err(H::INCOMPLETE_EFFECT);
        }
        if controls == 0x20000000 && !exclusive {
            return Err(H::NOT_EXCLUSIVE);
        }
        if controls == 0x20000000 {
            now.checked_add(u64::from(parameters.start_delay) * 1000)
                .ok_or(H::INVALID_PARAM)?;
        }
        let effect = self.effect_mut(id)?;
        effect.parameters = parameters;
        effect.initialized = initialized;
        if controls != 0x80000000 && exclusive {
            effect.downloaded = Some(effect.parameters.clone());
            effect.armed = effect.parameters.trigger.is_some();
        }
        if controls == 0x20000000 {
            effect.playing = Some(Playback32 {
                start: now,
                iterations: 1,
                triggered: false,
            });
        }
        Ok(if controls != 0x80000000 && !exclusive {
            3
        } else {
            0
        })
    }
    pub fn download(&mut self, id: u64, exclusive: bool) -> Result<u32, H> {
        if !exclusive {
            return Err(H::NOT_EXCLUSIVE);
        }
        let effect = self.effect_mut(id)?;
        if effect.initialized & REQUIRED_PARAMETERS != REQUIRED_PARAMETERS {
            return Err(H::INCOMPLETE_EFFECT);
        }
        let result = u32::from(effect.downloaded.is_some());
        effect.downloaded = Some(effect.parameters.clone());
        effect.armed = effect.parameters.trigger.is_some();
        Ok(result)
    }
    pub fn start(
        &mut self,
        id: u64,
        iterations: u32,
        flags: u32,
        exclusive: bool,
        now: u64,
    ) -> Result<(), H> {
        if iterations == 0 || flags & !0x80000001 != 0 {
            return Err(H::INVALID_PARAM);
        }
        if !exclusive {
            return Err(H::NOT_EXCLUSIVE);
        }
        let effect = self.effect(id)?;
        if effect.initialized & REQUIRED_PARAMETERS != REQUIRED_PARAMETERS {
            return Err(H::INCOMPLETE_EFFECT);
        }
        if flags & 0x80000000 != 0 && effect.downloaded.is_none() {
            return Err(H::NOT_DOWNLOADED);
        }
        let now = self.paused.unwrap_or(now);
        now.checked_add(u64::from(effect.parameters.start_delay) * 1000)
            .ok_or(H::INVALID_PARAM)?;
        if flags & 1 != 0 {
            for effect in &mut self.effects {
                effect.playing = None;
                effect.armed = false;
            }
        }
        let effect = self.effect_mut(id)?;
        if flags & 0x80000000 == 0 {
            effect.downloaded = Some(effect.parameters.clone());
        }
        effect.armed = effect.downloaded.as_ref().unwrap().trigger.is_some();
        effect.playing = Some(Playback32 {
            start: now,
            iterations,
            triggered: false,
        });
        Ok(())
    }
    pub fn stop(&mut self, id: u64) -> Result<(), H> {
        let effect = self.effect_mut(id)?;
        effect.playing = None;
        effect.armed = false;
        Ok(())
    }
    pub fn unload(&mut self, id: u64) -> Result<(), H> {
        self.stop(id)?;
        self.effect_mut(id)?.downloaded = None;
        Ok(())
    }
    pub fn lose_acquisition(&mut self) {
        for effect in &mut self.effects {
            effect.playing = None;
            effect.downloaded = None;
            effect.armed = false;
        }
    }
    pub fn command(&mut self, command: u32, exclusive: bool, now: u64) -> Result<(), H> {
        if !matches!(command, 1 | 2 | 4 | 8 | 16 | 32) {
            return Err(H::INVALID_PARAM);
        }
        if !exclusive {
            return Err(H::NOT_EXCLUSIVE);
        }
        if command == 8
            && let Some(paused) = self.paused
        {
            let delta = now.checked_sub(paused).ok_or(H::INVALID_PARAM)?;
            for effect in &self.effects {
                if let Some(playing) = effect.playing {
                    playing.start.checked_add(delta).ok_or(H::INVALID_PARAM)?;
                }
            }
        }
        match command {
            1 => {
                self.lose_acquisition();
                self.paused = None;
                self.actuators = true;
            }
            2 => {
                for effect in &mut self.effects {
                    effect.playing = None;
                    effect.armed = false;
                }
            }
            4 => {
                if self.paused.is_none() {
                    self.paused = Some(now);
                }
            }
            8 => {
                if let Some(paused) = self.paused.take() {
                    let delta = now - paused;
                    for effect in &mut self.effects {
                        if let Some(playing) = &mut effect.playing {
                            playing.start += delta;
                        }
                    }
                }
            }
            16 => self.actuators = true,
            32 => self.actuators = false,
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn effect_status(&self, id: u64, now: u64) -> Result<u32, H> {
        let effect = self.effect(id)?;
        Ok(if effect.elapsed(self.paused.unwrap_or(now)).is_some() {
            1
        } else {
            0
        })
    }
    pub fn state(&self, now: u64) -> u32 {
        let empty = self
            .effects
            .iter()
            .all(|effect| effect.downloaded.is_none());
        let stopped = self
            .effects
            .iter()
            .all(|effect| effect.elapsed(self.paused.unwrap_or(now)).is_none());
        u32::from(empty)
            | (u32::from(stopped) * 2)
            | (u32::from(self.paused.is_some()) * 4)
            | if self.actuators { 16 } else { 32 }
    }
    fn effect(&self, id: u64) -> Result<&Effect32, H> {
        self.effects
            .iter()
            .find(|effect| effect.id == id)
            .ok_or(H::OBJECT_NOT_FOUND)
    }
    fn effect_mut(&mut self, id: u64) -> Result<&mut Effect32, H> {
        self.effects
            .iter_mut()
            .find(|effect| effect.id == id)
            .ok_or(H::OBJECT_NOT_FOUND)
    }
}

impl Effect32 {
    pub fn elapsed(&self, now: u64) -> Option<u64> {
        let parameters = self.downloaded.as_ref()?;
        let playing = self.playing?;
        let elapsed = now
            .checked_sub(playing.start)?
            .checked_div(1000)?
            .checked_sub(u64::from(parameters.start_delay))?;
        if parameters.duration == u32::MAX {
            return Some(elapsed);
        }
        if parameters.duration == 0 {
            return None;
        }
        let duration = u64::from(parameters.duration);
        if playing.iterations != u32::MAX && elapsed >= duration * u64::from(playing.iterations) {
            None
        } else {
            Some(elapsed % duration)
        }
    }
}
