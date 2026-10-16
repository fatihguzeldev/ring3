use super::{AecParameters, CaptureEffectKind, samples};
use crate::audio::dsound::{HResult, WaveFormat};
use crate::math::spectral::{Buffer32, Complex32, Plan32, Workspace32};

const BLOCK: usize = 128;
const FFT: usize = BLOCK * 2;

pub struct CaptureProcessor {
    format: WaveFormat,
    kinds: [Option<CaptureEffectKind>; 2],
    channels: Vec<Channel>,
    bytes: u32,
    active: bool,
}
struct Channel {
    plan: Plan32,
    history: Buffer32,
    weights: Buffer32,
    scratch: Workspace32,
    work: Buffer32,
    error: Buffer32,
    power: [f32; FFT],
    noise: [f32; FFT],
    noise_initialized: bool,
    gain: [f32; FFT],
    previous: [f32; BLOCK],
    reference: [f32; BLOCK],
    microphone: [f32; BLOCK],
    output: [f32; BLOCK],
    cursor: usize,
    partition: usize,
    constraint: usize,
    energy: f32,
    status: u32,
    converged: u32,
    random: u32,
}
impl CaptureProcessor {
    pub fn new(
        format: WaveFormat,
        kinds: &[CaptureEffectKind],
        budget: u32,
    ) -> Result<Self, HResult> {
        let format = format.validate()?;
        if kinds.is_empty()
            || kinds.len() > 2
            || !kinds.contains(&CaptureEffectKind::Aec)
            || kinds.len() == 2 && kinds[0] == kinds[1]
        {
            return Err(HResult::INVALID_PARAM);
        }
        let partitions =
            (u64::from(format.pcm.sample_rate) * 128).div_ceil(1000 * BLOCK as u64) as usize;
        let values = partitions.checked_mul(FFT).ok_or(HResult::OUT_OF_MEMORY)?;
        let bytes = ((values * 2 + FFT * 3) * Buffer32::VALUE_BYTES
            + core::mem::size_of::<Channel>()
            + 16384)
            .checked_mul(format.pcm.channels as usize)
            .and_then(|bytes| u32::try_from(bytes).ok())
            .ok_or(HResult::OUT_OF_MEMORY)?;
        if bytes > budget {
            return Err(HResult::OUT_OF_MEMORY);
        }
        let plan = Plan32::new(FFT);
        let mut channels = Vec::new();
        channels
            .try_reserve_exact(format.pcm.channels as usize)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for _ in 0..format.pcm.channels {
            channels.push(Channel {
                plan: plan.clone(),
                history: allocated(values)?,
                weights: allocated(values)?,
                scratch: plan.workspace().map_err(|_| HResult::OUT_OF_MEMORY)?,
                work: allocated(FFT)?,
                error: allocated(FFT)?,
                power: [1e-6; FFT],
                noise: [1e-6; FFT],
                noise_initialized: false,
                gain: [1.; FFT],
                previous: [0.; BLOCK],
                reference: [0.; BLOCK],
                microphone: [0.; BLOCK],
                output: [0.; BLOCK],
                cursor: 0,
                partition: 0,
                constraint: 0,
                energy: 0.,
                status: 0,
                converged: 0,
                random: 0x52433341,
            });
        }
        let mut selected = [None; 2];
        for (out, kind) in selected.iter_mut().zip(kinds) {
            *out = Some(*kind)
        }
        Ok(Self {
            format,
            kinds: selected,
            channels,
            bytes,
            active: false,
        })
    }
    pub fn bytes(&self) -> u32 {
        self.bytes
    }
    pub fn kinds(&self) -> impl Iterator<Item = CaptureEffectKind> + '_ {
        self.kinds.into_iter().flatten()
    }
    pub fn mode(&self) -> u32 {
        if self.active { 2 } else { 0 }
    }
    pub fn status(&self) -> u32 {
        self.channels
            .iter()
            .fold(9, |status, channel| status & channel.status)
            | self
                .channels
                .iter()
                .fold(0, |status, channel| status | channel.status & 2)
    }
    pub fn reset(&mut self) {
        self.active = false;
        for channel in &mut self.channels {
            channel.reset()
        }
    }
    pub fn reset_noise(&mut self) {
        for channel in &mut self.channels {
            channel.noise.fill(1e-6);
            channel.noise_initialized = false;
            channel.gain.fill(1.);
        }
    }
    pub fn process(
        &mut self,
        bytes: &mut [u8],
        parameters: AecParameters,
        noise: bool,
        mut reference: impl FnMut(usize) -> Option<[f32; 2]>,
    ) {
        let format = self.format;
        let width = format.pcm.bits_per_sample as usize / 8;
        for (frame, pcm) in bytes
            .chunks_exact_mut(format.pcm.frame_bytes() as usize)
            .enumerate()
        {
            let source = reference(frame);
            let active = parameters.enabled && source.is_some();
            if active != self.active {
                self.reset();
                self.active = active
            }
            if !active {
                continue;
            }
            let source = source.unwrap();
            let noise = noise && self.kinds.contains(&Some(CaptureEffectKind::NoiseSuppress));
            for (index, (channel, sample)) in self
                .channels
                .iter_mut()
                .zip(pcm.chunks_exact_mut(width))
                .enumerate()
            {
                let reference = if format.pcm.channels == 1 {
                    (source[0] + source[1]) * 0.5
                } else {
                    source[index]
                };
                let value = channel.sample(
                    samples::decode(format, sample),
                    reference,
                    noise,
                    parameters.noise_fill,
                );
                samples::encode(format, value, sample);
            }
        }
    }
}
impl Channel {
    fn reset(&mut self) {
        self.history.clear();
        self.weights.clear();
        self.power.fill(1e-6);
        self.noise.fill(1e-6);
        self.noise_initialized = false;
        self.gain.fill(1.);
        self.previous.fill(0.);
        self.reference.fill(0.);
        self.microphone.fill(0.);
        self.output.fill(0.);
        self.cursor = 0;
        self.partition = 0;
        self.constraint = 0;
        self.energy = 0.;
        self.status = 0;
        self.converged = 0;
    }
    fn sample(&mut self, microphone: f32, reference: f32, noise: bool, fill: bool) -> f32 {
        let value = self.output[self.cursor];
        self.microphone[self.cursor] = microphone;
        self.reference[self.cursor] = reference;
        self.cursor += 1;
        if self.cursor == BLOCK {
            self.block(noise, fill);
            self.cursor = 0
        }
        value
    }
    fn block(&mut self, noise: bool, fill: bool) {
        let partitions = self.history.len() / FFT;
        let row = self.partition * FFT;
        for index in 0..BLOCK {
            self.history
                .set(row + index, Complex32::new(self.previous[index], 0.));
            self.history.set(
                row + BLOCK + index,
                Complex32::new(self.reference[index], 0.),
            );
        }
        self.plan
            .forward(&mut self.history, row..row + FFT, &mut self.scratch);
        self.previous = self.reference;
        self.work.clear();
        for partition in 0..partitions {
            let row = (self.partition + partitions - partition) % partitions * FFT;
            for bin in 0..FFT {
                self.work.add(
                    bin,
                    self.weights.get(partition * FFT + bin) * self.history.get(row + bin),
                );
            }
        }
        self.plan.inverse(&mut self.work, 0..FFT, &mut self.scratch);
        let mut residual = 0.;
        let mut reference_energy = 0.;
        let mut microphone_energy = 0.;
        self.error.clear();
        for index in 0..BLOCK {
            let value = self.microphone[index] - self.work.get(BLOCK + index).re / FFT as f32;
            self.error.set(BLOCK + index, Complex32::new(value, 0.));
            residual += value * value;
            reference_energy += self.reference[index] * self.reference[index];
            microphone_energy += self.microphone[index] * self.microphone[index];
        }
        self.plan
            .forward(&mut self.error, 0..FFT, &mut self.scratch);
        for bin in 0..FFT {
            let power = (0..partitions)
                .map(|partition| self.history.get(partition * FFT + bin).norm_sqr())
                .sum::<f32>();
            self.power[bin] = 0.8 * self.power[bin] + 0.2 * power;
        }
        if reference_energy > 1e-6 && microphone_energy < reference_energy * 4. {
            for partition in 0..partitions {
                let row = (self.partition + partitions - partition) % partitions * FFT;
                for bin in 0..FFT {
                    self.weights.add(
                        partition * FFT + bin,
                        self.history.get(row + bin).conj()
                            * self.error.get(bin)
                            * (0.25 / (self.power[bin] + 1e-6)),
                    );
                }
            }
            let row = self.constraint * FFT;
            self.plan
                .inverse(&mut self.weights, row..row + FFT, &mut self.scratch);
            self.weights.divide(row..row + BLOCK, FFT as f32);
            self.weights.clear_range(row + BLOCK..row + FFT);
            self.plan
                .forward(&mut self.weights, row..row + FFT, &mut self.scratch);
            self.constraint = (self.constraint + 1) % partitions;
        }
        let converged = reference_energy > 1e-6 && residual < reference_energy * 0.1;
        self.converged = if converged {
            self.converged.saturating_add(1)
        } else {
            0
        };
        if self.converged >= 4 {
            self.status |= 9
        } else if self.status & 8 != 0 {
            self.status = (self.status & !9) | 2
        }
        self.energy = 0.99 * self.energy + 0.01 * (microphone_energy / BLOCK as f32);
        if noise {
            for bin in 0..FFT {
                let power = self.error.get(bin).norm_sqr();
                if !self.noise_initialized {
                    self.noise[bin] = power.max(1e-9)
                }
                let rate = if power < self.noise[bin] { 0.05 } else { 0.001 };
                self.noise[bin] += rate * (power - self.noise[bin]);
                let target = ((power - 1.5 * self.noise[bin]) / (power + 1e-9))
                    .clamp(0.0064, 1.)
                    .sqrt();
                self.gain[bin] = 0.7 * self.gain[bin] + 0.3 * target;
                self.error.scale(bin, self.gain[bin]);
            }
            self.plan
                .inverse(&mut self.error, 0..FFT, &mut self.scratch);
            for index in 0..BLOCK {
                self.output[index] = self.error.get(BLOCK + index).re / FFT as f32
            }
            self.noise_initialized = true;
        } else {
            for index in 0..BLOCK {
                self.output[index] =
                    self.microphone[index] - self.work.get(BLOCK + index).re / FFT as f32
            }
        }
        if fill {
            for value in &mut self.output {
                self.random = self.random.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (self.random as i32 as f32 / i32::MAX as f32)
                    * self.energy.sqrt().min(0.003)
                    * 0.02;
                *value += noise;
            }
        }
        for value in &mut self.output {
            if !value.is_finite() {
                *value = 0.
            }
        }
        self.partition = (self.partition + 1) % partitions;
    }
}
fn allocated(count: usize) -> Result<Buffer32, HResult> {
    Buffer32::zeroed(count).map_err(|_| HResult::OUT_OF_MEMORY)
}
