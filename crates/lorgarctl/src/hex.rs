use lorgar_core::{LorgarError, Result};

pub fn format_hex_bytes(slice: &[u8]) -> String {
    slice
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_hex_payload(inputs: &[String]) -> Result<[u8; 8]> {
    let joined = inputs.join(" ").replace("0x", "").replace(',', "");
    let hex_chars: String = joined.chars().filter(|c| !c.is_whitespace()).collect();

    if hex_chars.len() & 1 != 0 || !hex_chars.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(LorgarError::HexParseError(
            "Odd number of hex characters".into(),
        ));
    }

    let mut bytes = Vec::new();
    for i in (0..hex_chars.len()).step_by(2) {
        let byte_str = &hex_chars[i..i + 2];
        let byte = u8::from_str_radix(byte_str, 16).map_err(|e| {
            LorgarError::HexParseError(format!("Invalid hex byte '{byte_str}': {e}"))
        })?;
        bytes.push(byte);
    }

    if bytes.len() != 8 {
        return Err(LorgarError::InvalidFeatureLength { len: bytes.len() });
    }

    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_odd_hex_without_panicking() {
        assert!(parse_hex_payload(&["abc".to_string()]).is_err());
    }

    #[test]
    fn accepts_spaced_hex() {
        let out = parse_hex_payload(&["82 00 00 00 00 00 00 00".to_string()]).unwrap();
        assert_eq!(out[0], 0x82);
    }
}
