use data_encoding::BASE32_NOPAD;

pub fn cid_of(bytes: &[u8]) -> String {
    BASE32_NOPAD
        .encode(blake3::hash(bytes).as_bytes())
        .to_lowercase()
}
