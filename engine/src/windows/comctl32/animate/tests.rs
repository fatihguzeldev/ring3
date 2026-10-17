use super::*;
fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(id);
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(data);
    if data.len() & 1 != 0 {
        bytes.push(0);
    }
    bytes
}
fn list(id: &[u8; 4], data: Vec<u8>) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend(data);
    chunk(b"LIST", &bytes)
}
fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub(crate) fn avi() -> Vec<u8> {
    let mut header = vec![0; 56];
    for (at, value) in [(0, 20_000), (16, 2), (24, 1), (32, 2), (36, 2)] {
        word(&mut header, at, value);
    }
    let mut stream = vec![0; 56];
    stream[..8].copy_from_slice(b"vidsDIB ");
    for (at, value) in [(20, 1), (24, 50), (32, 2)] {
        word(&mut stream, at, value);
    }
    let mut format = vec![0; 40];
    for (at, value) in [
        (0, 40),
        (4, 2),
        (8, (-2i32) as u32),
        (12, 1 | 24 << 16),
        (20, 16),
    ] {
        word(&mut format, at, value);
    }
    let mut strl = chunk(b"strh", &stream);
    strl.extend(chunk(b"strf", &format));
    let mut hdrl = chunk(b"avih", &header);
    hdrl.extend(list(b"strl", strl));
    let frame0 = [0, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0];
    let frame1 = [255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0];
    let mut movi = chunk(b"00db", &frame0);
    movi.extend(chunk(b"00db", &frame1));
    let mut body = b"AVI ".to_vec();
    body.extend(list(b"hdrl", hdrl));
    body.extend(list(b"movi", movi));
    chunk(b"RIFF", &body)
}
#[test]
fn bounded_avi_preserves_dib_rows_and_frame_order_and_rejects_audio_codec_and_truncation() {
    let bytes = avi();
    let clip = AviClip32::parse(bytes.clone(), bytes.len()).unwrap();
    assert_eq!(
        (
            clip.layout.width,
            clip.layout.height,
            clip.frames.len(),
            clip.period_ms
        ),
        (2, 2, 2, 20)
    );
    assert_eq!(
        (
            clip.color(0, 0, 0),
            clip.color(0, 1, 0),
            clip.color(1, 0, 0)
        ),
        (0xff, 0xff00, 0xff0000)
    );
    for end in 0..bytes.len() {
        assert!(AviClip32::parse(bytes[..end].to_vec(), bytes.len()).is_err());
    }
    assert_eq!(
        AviClip32::parse(bytes.clone(), bytes.len() - 1),
        Err(E::Capacity)
    );
    for (marker, replacement) in [
        (b"vids".as_slice(), b"auds".as_slice()),
        (b"DIB ".as_slice(), b"XVID".as_slice()),
    ] {
        let mut malformed = bytes.clone();
        let at = malformed.windows(4).position(|p| p == marker).unwrap();
        malformed[at..at + 4].copy_from_slice(replacement);
        assert_eq!(AviClip32::parse(malformed, bytes.len()), Err(E::Win32(50)));
    }
}
#[test]
fn animation_repeat_stop_and_seek_bounds_keep_the_last_visible_frame() {
    let clip = AviClip32::parse(avi(), 4096).unwrap();
    let mut model = Animate32 {
        clip: Some(clip),
        playing: true,
        last: 1,
        remaining: 2,
        ..Default::default()
    };
    assert_eq!(model.play_range(0, u16::MAX), Ok((0, 1)));
    assert_eq!(model.play_range(1, 0), Err(E::Win32(87)));
    assert!(!model.advance());
    assert_eq!(model.current, 1);
    assert!(!model.advance());
    assert_eq!(model.current, 0);
    assert!(!model.advance());
    assert_eq!(model.current, 1);
    assert!(model.advance());
    assert!(!model.playing);
    assert_eq!(model.current, 1);
    assert!(!model.advance());
}
