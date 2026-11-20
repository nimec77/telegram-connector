//! Synthetic metadata-heavy media shared by portable probe and Unix staging tests.

pub fn metadata_jpeg() -> Vec<u8> {
    let jpeg = crate::test_helpers::create_test_jpeg(16, 16);
    let mut bytes = jpeg[..2].to_vec();
    for marker in [0xe1, 0xfe] {
        bytes.extend_from_slice(&[0xff, marker, 0xff, 0xff]);
        bytes.resize(bytes.len() + 65_533, 0);
    }
    bytes.extend_from_slice(&jpeg[2..]);
    bytes
}

pub fn tagged_mp3(version: u8, flags: u8, body: &[u8]) -> Vec<u8> {
    let size = body.len();
    assert!(size < (1 << 28));
    let header = [
        b'I',
        b'D',
        b'3',
        version,
        0,
        flags,
        ((size >> 21) & 127) as u8,
        ((size >> 14) & 127) as u8,
        ((size >> 7) & 127) as u8,
        (size & 127) as u8,
    ];
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(body);
    if version == 4 && flags & 0x10 != 0 {
        bytes.extend_from_slice(b"3DI");
        bytes.extend_from_slice(&header[3..]);
    }
    bytes.extend_from_slice(include_bytes!("fixtures/tone.mp3"));
    bytes
}
