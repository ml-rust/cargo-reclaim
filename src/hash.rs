pub(crate) fn format_sha256(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut formatted = String::with_capacity("sha256:".len() + digest.len() * 2);
    formatted.push_str("sha256:");
    for &byte in digest {
        formatted.push(char::from(HEX[usize::from(byte >> 4)]));
        formatted.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    formatted
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::format_sha256;

    #[test]
    fn sha256_format_preserves_known_digests() {
        for (input, expected) in [
            (
                "",
                "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "abc",
                "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
        ] {
            assert_eq!(format_sha256(&Sha256::digest(input)), expected);
        }
    }
}
