use crate::{audio::effects::EffectKind as K, windows::com::InterfaceId};

const fn id(a: u32, b: u16, c: u16, d: u64) -> InterfaceId {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    let d = d.to_be_bytes();
    InterfaceId([
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6],
        d[7],
    ])
}

pub const ALL_OBJECTS: InterfaceId = id(0xaa114de5, 0xc262, 0x4169, 0xa1c823d698cc73b5);

pub const fn interface_id(kind: K) -> InterfaceId {
    match kind {
        K::Chorus => id(0x880842e3, 0x145f, 0x43e6, 0xa934a71806e50547),
        K::Compressor => id(0x4bbd1154, 0x62f6, 0x4e2c, 0xa15cd3b6c417f7a0),
        K::Distortion => id(0x8ecf4326, 0x455f, 0x4d8b, 0xbda98d5d3e9e3e0b),
        K::Echo => id(0x8bd28edf, 0x50db, 0x4e92, 0xa2bd445488d1ed42),
        K::Flanger => id(0x903e9878, 0x2c92, 0x4072, 0x9b2cea68f5396783),
        K::Gargle => id(0xd616f352, 0xd622, 0x11ce, 0xaac50020af0b99a3),
        K::ParametricEq => id(0xc03ca9fe, 0xfe90, 0x4204, 0x807882334cd177da),
        K::I3dl2Reverb => id(0x4b166a6a, 0x0d66, 0x43f3, 0x80e3ee6280dee1a4),
        K::WavesReverb => id(0x46858c3a, 0x0dc6, 0x45e3, 0xb760d4eef16cb325),
    }
}

pub const fn class_id(kind: K) -> InterfaceId {
    match kind {
        K::Chorus => id(0xefe6629c, 0x81f7, 0x4281, 0xbd91c9d604a95af6),
        K::Compressor => id(0xef011f79, 0x4000, 0x406d, 0x87afbffb3fc39d57),
        K::Distortion => id(0xef114c90, 0xcd1d, 0x484e, 0x96e509cfaf912a21),
        K::Echo => id(0xef3e932c, 0xd40b, 0x4f51, 0x8ccf3f98f1b29d5d),
        K::Flanger => id(0xefca3d92, 0xdfd8, 0x4672, 0xa6037420894bad98),
        K::Gargle => id(0xdafd8210, 0x5711, 0x4b91, 0x9fe3f75b7ae279bf),
        K::ParametricEq => id(0x120ced89, 0x3bf4, 0x4173, 0xa1323cb406cf3231),
        K::I3dl2Reverb => id(0xef985e71, 0xd5c7, 0x42d4, 0xba4d2d073e2e96f4),
        K::WavesReverb => id(0x87fc0268, 0x9a55, 0x4360, 0x95aa004a1d9de26c),
    }
}
