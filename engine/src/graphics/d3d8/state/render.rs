use super::StateError;

pub(super) const RENDER_COUNT: usize = 174;
pub(super) const STAGE_COUNT: usize = 29;
pub(super) const TRANSFORM_COUNT: usize = 266;

pub(super) fn render_index(state: u32) -> Result<usize, StateError> {
    if matches!(state, 7..=10 | 14..=16 | 19..=20 | 22..=30 | 34..=38 | 40 | 47..=48
        | 52..=60 | 128..=137 | 139..=143 | 145..=148 | 151..=168 | 170..=173)
    {
        Ok(state as usize)
    } else {
        Err(StateError::InvalidState)
    }
}

pub(super) fn stage_index(state: u32) -> Result<usize, StateError> {
    if matches!(state, 1..=11 | 13..=28) {
        Ok(state as usize)
    } else {
        Err(StateError::InvalidState)
    }
}

pub(super) fn transform_index(state: u32) -> Result<usize, StateError> {
    match state {
        2 => Ok(0),
        3 => Ok(1),
        16..=23 => Ok(2 + (state - 16) as usize),
        256..=511 => Ok(10 + (state - 256) as usize),
        _ => Err(StateError::InvalidState),
    }
}

pub(super) fn render_defaults(auto_depth: bool) -> [u32; RENDER_COUNT] {
    let mut values = [0; RENDER_COUNT];
    for (state, value) in [
        (7, u32::from(auto_depth)),
        (8, 3),
        (9, 2),
        (14, 1),
        (16, 1),
        (19, 2),
        (20, 1),
        (22, 3),
        (23, 4),
        (25, 8),
        (37, 0x3f80_0000),
        (38, 0x3f80_0000),
        (53, 1),
        (54, 1),
        (55, 1),
        (56, 8),
        (58, u32::MAX),
        (59, u32::MAX),
        (60, u32::MAX),
        (136, 1),
        (137, 1),
        (141, 1),
        (142, 1),
        (145, 1),
        (146, 2),
        (154, 0x3f80_0000),
        (155, 0x3f80_0000),
        (158, 0x3f80_0000),
        (161, 1),
        (162, u32::MAX),
        (164, 0x3f80_0000),
        (166, 0x4280_0000),
        (168, 15),
        (171, 1),
        (172, 3),
        (173, 1),
    ] {
        values[state] = value;
    }
    values
}

pub(super) fn stage_defaults(stage: usize) -> [u32; STAGE_COUNT] {
    let mut values = [0; STAGE_COUNT];
    for (state, value) in [
        (1, if stage == 0 { 4 } else { 1 }),
        (2, 2),
        (3, 1),
        (4, if stage == 0 { 2 } else { 1 }),
        (5, 2),
        (6, 1),
        (11, stage as u32),
        (13, 1),
        (14, 1),
        (16, 1),
        (17, 1),
        (21, 1),
        (25, 1),
        (26, 1),
        (27, 1),
        (28, 1),
    ] {
        values[state] = value;
    }
    values
}

pub(super) fn vertex_render(state: usize) -> bool {
    matches!(state, 9 | 22 | 28 | 29 | 34..=38 | 48 | 136..=137 | 139..=143
        | 145..=148 | 151..=163 | 166..=167 | 170 | 172..=173)
}

pub(super) fn pixel_render(state: usize) -> bool {
    matches!(state, 7..=10 | 14..=16 | 19..=20 | 23..=27 | 36..=38 | 40 | 47
        | 52..=60 | 128..=135 | 168 | 171)
}

pub(super) fn vertex_stage(state: usize) -> bool {
    matches!(state, 11 | 24)
}
