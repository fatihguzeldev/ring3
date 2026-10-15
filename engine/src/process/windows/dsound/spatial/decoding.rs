use crate::audio::dsound::{Buffer3D, HResult, Listener3D, Vector3};
pub(in crate::process::windows::dsound) fn vector(words: &[u32]) -> Vector3 {
    Vector3 {
        x: f32::from_bits(words[0]),
        y: f32::from_bits(words[1]),
        z: f32::from_bits(words[2]),
    }
}
pub(in crate::process::windows::dsound) fn bits(value: Vector3) -> [u32; 3] {
    [value.x.to_bits(), value.y.to_bits(), value.z.to_bits()]
}
pub(in crate::process::windows::dsound) fn listener(
    words: [u32; 16],
) -> Result<Listener3D, HResult> {
    if words[0] != 64 {
        return Err(HResult::INVALID_PARAM);
    }
    Listener3D {
        position: vector(&words[1..4]),
        velocity: vector(&words[4..7]),
        front: vector(&words[7..10]),
        top: vector(&words[10..13]),
        distance_factor: f32::from_bits(words[13]),
        rolloff_factor: f32::from_bits(words[14]),
        doppler_factor: f32::from_bits(words[15]),
    }
    .validate()
}
pub(in crate::process::windows::dsound) fn listener_words(value: Listener3D) -> [u32; 16] {
    let mut words = [0; 16];
    words[0] = 64;
    words[1..4].copy_from_slice(&bits(value.position));
    words[4..7].copy_from_slice(&bits(value.velocity));
    words[7..10].copy_from_slice(&bits(value.front));
    words[10..13].copy_from_slice(&bits(value.top));
    words[13..].copy_from_slice(&[
        value.distance_factor.to_bits(),
        value.rolloff_factor.to_bits(),
        value.doppler_factor.to_bits(),
    ]);
    words
}
pub(in crate::process::windows::dsound) fn buffer(words: [u32; 16]) -> Result<Buffer3D, HResult> {
    if words[0] != 64 {
        return Err(HResult::INVALID_PARAM);
    }
    Buffer3D {
        position: vector(&words[1..4]),
        velocity: vector(&words[4..7]),
        inside_cone: words[7],
        outside_cone: words[8],
        cone_orientation: vector(&words[9..12]),
        outside_volume: words[12] as i32,
        min_distance: f32::from_bits(words[13]),
        max_distance: f32::from_bits(words[14]),
        mode: words[15],
    }
    .validate()
}
pub(in crate::process::windows::dsound) fn buffer_words(value: Buffer3D) -> [u32; 16] {
    let mut words = [0; 16];
    words[0] = 64;
    words[1..4].copy_from_slice(&bits(value.position));
    words[4..7].copy_from_slice(&bits(value.velocity));
    words[7..9].copy_from_slice(&[value.inside_cone, value.outside_cone]);
    words[9..12].copy_from_slice(&bits(value.cone_orientation));
    words[12..].copy_from_slice(&[
        value.outside_volume as u32,
        value.min_distance.to_bits(),
        value.max_distance.to_bits(),
        value.mode,
    ]);
    words
}
