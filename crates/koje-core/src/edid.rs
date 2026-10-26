//! A screen's identity from its EDID, as Mutter derives it (ADR 0046):
//! vendor, product and serial are what `monitors.xml` keys its configurations
//! by, together with the connector. Read before Hyprland starts, from
//! `/sys/class/drm/*/edid`.

/// Vendor, product and serial as Mutter writes them into `monitors.xml`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identity {
    /// The manufacturer's three-letter PNP code, e.g. `DEL`.
    pub vendor: String,
    /// The monitor name descriptor, else the product code as `0x%04x`.
    pub product: String,
    /// The serial descriptor, else the serial number as `0x%08x`.
    pub serial: String,
}

const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];

/// The identity in an EDID's base block; `None` when it is none.
pub fn identity(edid: &[u8]) -> Option<Identity> {
    if edid.len() < 128 || edid[..8] != HEADER {
        return None;
    }
    // Three letters, five bits each, 1 = 'A'.
    let m = u16::from_be_bytes([edid[8], edid[9]]);
    let letter = |shift: u16| {
        let v = u8::try_from((m >> shift) & 0x1f).unwrap_or(0);
        (b'A' - 1 + v) as char
    };
    let vendor: String = [letter(10), letter(5), letter(0)].iter().collect();
    let product_code = u16::from_le_bytes([edid[10], edid[11]]);
    let serial_number = u32::from_le_bytes([edid[12], edid[13], edid[14], edid[15]]);

    // Four 18-byte descriptors; text ones are tagged 0xFC (name) and 0xFF
    // (serial), up to 13 characters ended by a newline and padded.
    let mut name = None;
    let mut serial = None;
    for d in edid[54..126].chunks(18) {
        if d[0] != 0 || d[1] != 0 {
            continue; // a detailed timing, not a descriptor
        }
        let text = || {
            let raw: Vec<u8> = d[5..18]
                .iter()
                .copied()
                .take_while(|&b| b != b'\n' && b != 0)
                .collect();
            let t = String::from_utf8_lossy(&raw).trim_end().to_owned();
            (!t.is_empty()).then_some(t)
        };
        match d[3] {
            0xfc => name = name.or_else(text),
            0xff => serial = serial.or_else(text),
            _ => {}
        }
    }
    Some(Identity {
        vendor,
        product: name.unwrap_or_else(|| format!("0x{product_code:04x}")),
        serial: serial.unwrap_or_else(|| format!("0x{serial_number:08x}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base block with `vendor` (packed), codes and text descriptors.
    fn edid(name: Option<&str>, serial: Option<&str>) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        e[..8].copy_from_slice(&HEADER);
        // "DEL": D=4, E=5, L=12.
        let m: u16 = 0b0_00100_00101_01100;
        e[8..10].copy_from_slice(&m.to_be_bytes());
        e[10..12].copy_from_slice(&0xa0c2u16.to_le_bytes());
        e[12..16].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        // First descriptor slot: a detailed timing (non-zero clock).
        e[54] = 0x4d;
        e[55] = 0xd0;
        let mut put = |slot: usize, tag: u8, text: &str| {
            let at = 54 + slot * 18;
            e[at + 3] = tag;
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(b'\n');
            bytes.resize(13, b' ');
            e[at + 5..at + 18].copy_from_slice(&bytes);
        };
        if let Some(n) = name {
            put(1, 0xfc, n);
        }
        if let Some(s) = serial {
            put(2, 0xff, s);
        }
        e
    }

    #[test]
    fn identity_from_descriptors() {
        assert_eq!(
            identity(&edid(Some("DELL U4025QW"), Some("7XYZ123"))),
            Some(Identity {
                vendor: "DEL".into(),
                product: "DELL U4025QW".into(),
                serial: "7XYZ123".into(),
            })
        );
    }

    #[test]
    fn codes_when_there_is_no_text() {
        let id = identity(&edid(None, None)).unwrap();
        assert_eq!(
            (id.product.as_str(), id.serial.as_str()),
            ("0xa0c2", "0x12345678")
        );
    }

    #[test]
    fn not_an_edid() {
        assert_eq!(identity(&[0u8; 64]), None);
        assert_eq!(identity(&[0u8; 128]), None);
    }
}
