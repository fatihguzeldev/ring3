use crate::audio::{SampleEncoding32, dsound::WaveFormat};

pub(super) fn decode(format: WaveFormat, bytes: &[u8]) -> f32 {
    match (format.encoding, format.pcm.bits_per_sample) {
        (SampleEncoding32::Integer, 8) => (f32::from(bytes[0]) - 128.) / 128.,
        (SampleEncoding32::Integer, 16) => {
            f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.
        }
        (SampleEncoding32::Integer, 24) => {
            i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) as f32 / 2147483648.
        }
        (SampleEncoding32::Integer, 32) => {
            i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2147483648.
        }
        (SampleEncoding32::Float, 32) => {
            let value = f32::from_le_bytes(bytes.try_into().unwrap());
            if value.is_finite() { value } else { 0. }
        }
        _ => unreachable!(),
    }
}
pub(super) fn encode(format: WaveFormat, value: f32, bytes: &mut [u8]) {
    let value = value.clamp(-1., 1.);
    match (format.encoding, format.pcm.bits_per_sample) {
        (SampleEncoding32::Integer, 8) => {
            bytes[0] = (value * 128. + 128.).round().clamp(0., 255.) as u8
        }
        (SampleEncoding32::Integer, 16) => bytes.copy_from_slice(
            &((value * 32768.).round().clamp(-32768., 32767.) as i16).to_le_bytes(),
        ),
        (SampleEncoding32::Integer, 24) => {
            let sample = (f64::from(value) * 8388608.)
                .round()
                .clamp(-8388608., 8388607.) as i32;
            bytes.copy_from_slice(&sample.to_le_bytes()[..3])
        }
        (SampleEncoding32::Integer, 32) => bytes.copy_from_slice(
            &((f64::from(value) * 2147483648.)
                .round()
                .clamp(-2147483648., 2147483647.) as i32)
                .to_le_bytes(),
        ),
        (SampleEncoding32::Float, 32) => bytes.copy_from_slice(&value.to_le_bytes()),
        _ => unreachable!(),
    }
}
