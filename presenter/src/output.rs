//! Finding the glasses' output: by the EDID the kernel exposes for each connector, not by a connector name that happens to be
//! right on one machine. The glasses identify as manufacturer `MRG`, product `0x4102` in every display mode.

use std::path::Path;

pub const GLASSES_MANUFACTURER: &str = "MRG";
pub const GLASSES_PRODUCT: u16 = 0x4102;

/// The three-letter PNP manufacturer id and the product code from the base EDID block, or None if it is not an EDID.
pub fn parse_edid(edid: &[u8]) -> Option<(String, u16)> {
    const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
    if edid.len() < 12 || edid[..8] != HEADER {
        return None;
    }
    let packed = u16::from_be_bytes([edid[8], edid[9]]);
    let letter = |shift: u32| {
        let v = ((packed >> shift) & 0x1f) as u8;
        (1..=26).contains(&v).then(|| (b'A' + v - 1) as char)
    };
    let manufacturer: String = [letter(10)?, letter(5)?, letter(0)?].into_iter().collect();
    Some((manufacturer, u16::from_le_bytes([edid[10], edid[11]])))
}

pub fn is_glasses(edid: &[u8]) -> bool {
    parse_edid(edid).is_some_and(|(m, p)| m == GLASSES_MANUFACTURER && p == GLASSES_PRODUCT)
}

/// The kernel connector name (`DP-1`) of a connected output whose EDID is the glasses', looked up under `drm_dir` (`/sys/class/drm`).
/// On a Wayland compositor such as Plasma this is also the output's name.
pub fn find_glasses_connector(drm_dir: &Path) -> Option<String> {
    let mut found: Vec<String> = std::fs::read_dir(drm_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let connector = name.strip_prefix("card")?.split_once('-')?.1.to_string();
            let connected = std::fs::read_to_string(dir.join("status")).ok()?.trim() == "connected";
            (connected && is_glasses(&std::fs::read(dir.join("edid")).ok()?)).then_some(connector)
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// First bytes of the glasses' EDID as read from the Deck (header, `36 47` manufacturer, `02 41` product).
    const GLASSES: [u8; 16] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x36, 0x47, 0x02, 0x41, 0, 0, 0, 0];
    /// The Deck's internal panel, `59 96` manufacturer, `01 30` product.
    const PANEL: [u8; 16] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x59, 0x96, 0x01, 0x30, 0x01, 0, 0, 0];

    #[test]
    fn the_glasses_edid_is_recognised() {
        assert_eq!(parse_edid(&GLASSES), Some(("MRG".to_string(), 0x4102)));
        assert!(is_glasses(&GLASSES));
    }

    #[test]
    fn other_displays_and_junk_are_not() {
        assert_eq!(parse_edid(&PANEL), Some(("VLV".to_string(), 0x3001)));
        assert!(!is_glasses(&PANEL));
        let mut other_product = GLASSES;
        other_product[10] = 0x03;
        assert!(!is_glasses(&other_product));
        assert!(!is_glasses(&[]));
        assert!(!is_glasses(&[0u8; 16]));
        assert_eq!(parse_edid(&GLASSES[..10]), None);
    }

    #[test]
    fn the_connector_is_found_among_several_outputs() {
        let dir = std::env::temp_dir().join(format!("xreal-drm-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, status, edid) in [("card1-eDP-1", "connected", &PANEL), ("card1-DP-2", "connected", &PANEL), ("card1-DP-3", "connected", &GLASSES), ("card1-DP-4", "disconnected", &GLASSES)] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(dir.join(name).join("status"), format!("{status}\n")).unwrap();
            std::fs::write(dir.join(name).join("edid"), edid).unwrap();
        }
        std::fs::create_dir_all(dir.join("renderD128")).unwrap();
        assert_eq!(find_glasses_connector(&dir), Some("DP-3".to_string()), "a disconnected output with the glasses' EDID does not count");
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(find_glasses_connector(&dir), None);
    }
}
