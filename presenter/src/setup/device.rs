//! The device and capability report: which XREAL device is connected, what a front-end may offer for it, the runtime version and
//! the user units' state — as structured data (`status --json`). The capability table is the seam for other XREAL models and future
//! controls; the 1S is the only entry now.
//!
//! Producing the report reads only: the EDID the kernel exposes, the `VERSION` files and `systemctl --user` state. It sends nothing to
//! the glasses, so the display mode is unchanged.

use super::env::Env;
use super::units;
use crate::output;
use serde_json::{json, Value};
use std::path::Path;

/// The control identifiers a front-end renders and the mode selector maps onto units.
pub const HEAD_MOUSE: &str = "head_mouse";
pub const STEAMVR: &str = "steamvr";

/// What one XREAL model supports. One entry per model.
pub struct Profile {
    /// Stable id used in the report and by a front-end.
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// The product code this model reports in its EDID (with manufacturer `MRG`).
    pub product: u16,
    /// The controls this model supports.
    pub controls: &'static [&'static str],
}

/// The models this build knows. The 1S is the only one implemented.
pub const PROFILES: &[Profile] = &[Profile {
    id: "1s",
    name: "XREAL 1S",
    product: crate::output::GLASSES_PRODUCT,
    controls: &[HEAD_MOUSE, STEAMVR],
}];

/// The profile for the product code in a device's EDID, or `None` when this build does not know the model.
pub fn profile_for_product(product: u16) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.product == product)
}

/// The facts the report is rendered from, so the rendering is tested on synthetic states.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// The connector and product code of the connected XREAL output, if any.
    pub xreal: Option<(String, u16)>,
    /// The installed runtime's commit and build time, if a `VERSION` file is found.
    pub installed: Option<String>,
    pub bundled: Option<String>,
    /// `(unit, is-enabled, is-active)` for each user unit.
    pub units: Vec<(String, String, String)>,
}

/// Whether an XREAL device is connected at all. The report still prints (with `connected: false`) and `status --json` exits non-zero.
pub fn connected(f: &Facts) -> bool {
    f.xreal.is_some()
}

/// Render the report. `identified_by` is `null` when no device is connected, `"edid"` when the model is known, and
/// `"edid:unsupported"` when an XREAL display is connected but this build has no profile for its product code.
pub fn render(f: &Facts) -> Value {
    let device = match &f.xreal {
        None => json!({
            "connected": false,
            "identified_by": Value::Null,
            "model": Value::Null,
            "name": Value::Null,
            "supported": false,
            "controls": [],
        }),
        Some((connector, product)) => match profile_for_product(*product) {
            Some(p) => json!({
                "connected": true,
                "identified_by": "edid",
                "connector": connector,
                "model": p.id,
                "name": p.name,
                "supported": true,
                "controls": p.controls,
            }),
            None => json!({
                "connected": true,
                "identified_by": "edid:unsupported",
                "connector": connector,
                "model": Value::Null,
                "name": Value::Null,
                "supported": false,
                "controls": [],
            }),
        },
    };
    let units: serde_json::Map<String, Value> = f
        .units
        .iter()
        .map(|(name, enabled, active)| (name.clone(), json!({"enabled": enabled, "active": active})))
        .collect();
    json!({
        "device": device,
        "runtime": {
            "installed": f.installed.is_some(),
            "version": f.installed,
            "bundled": f.bundled,
        },
        "units": Value::Object(units),
    })
}

/// Gather the report facts from read-only sources: the EDID, the `VERSION` files and the user units' state.
pub fn gather(env: &Env) -> Facts {
    let xreal = output::find_xreal_output(Path::new("/sys/class/drm"));
    let u = |verb, unit| units::state(verb, unit);
    Facts {
        xreal,
        installed: env.installed_version().map(|v| v.commit),
        bundled: env.bundled_version().map(|v| v.commit),
        units: vec![
            (units::SOCKET.to_string(), u("is-enabled", units::SOCKET), u("is-active", units::SOCKET)),
            (units::SERVICE.to_string(), String::new(), u("is-active", units::SERVICE)),
            (units::HEADLOOK.to_string(), u("is-enabled", units::HEADLOOK), u("is-active", units::HEADLOOK)),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            xreal: Some(("DP-1".to_string(), crate::output::GLASSES_PRODUCT)),
            installed: Some("abc1234".to_string()),
            bundled: Some("abc1234".to_string()),
            units: vec![
                (units::SOCKET.to_string(), "enabled".to_string(), "active".to_string()),
                (units::SERVICE.to_string(), String::new(), "inactive".to_string()),
                (units::HEADLOOK.to_string(), "disabled".to_string(), "inactive".to_string()),
            ],
        }
    }

    #[test]
    fn a_supported_1s_is_named_with_its_controls() {
        let v = render(&facts());
        assert_eq!(v["device"]["connected"], true);
        assert_eq!(v["device"]["identified_by"], "edid");
        assert_eq!(v["device"]["model"], "1s");
        assert_eq!(v["device"]["name"], "XREAL 1S");
        assert_eq!(v["device"]["supported"], true);
        assert_eq!(v["device"]["controls"], json!(["head_mouse", "steamvr"]));
        assert_eq!(v["runtime"]["installed"], true);
        assert_eq!(v["runtime"]["version"], "abc1234");
        assert_eq!(v["units"]["xreal-linux.socket"]["enabled"], "enabled");
        assert_eq!(v["units"]["xreal-linux.socket"]["active"], "active");
        assert_eq!(v["units"]["xreal-linux.service"]["active"], "inactive");
    }

    #[test]
    fn no_device_is_reported_clearly_and_is_not_connected() {
        let f = Facts { xreal: None, ..Facts::default() };
        let v = render(&f);
        assert_eq!(v["device"]["connected"], false);
        assert_eq!(v["device"]["model"], Value::Null);
        assert_eq!(v["device"]["controls"], json!([]));
        assert_eq!(v["runtime"]["installed"], false);
        assert!(!connected(&f));
    }

    #[test]
    fn a_recognized_but_unsupported_model_gets_no_invented_controls() {
        let f = Facts { xreal: Some(("DP-2".to_string(), 0x4103)), ..Facts::default() };
        let v = render(&f);
        assert_eq!(v["device"]["connected"], true);
        assert_eq!(v["device"]["identified_by"], "edid:unsupported");
        assert_eq!(v["device"]["supported"], false);
        assert_eq!(v["device"]["model"], Value::Null);
        assert_eq!(v["device"]["controls"], json!([]));
        assert!(connected(&f), "a device is present even when unsupported");
    }

    #[test]
    fn the_capability_table_only_knows_the_1s_for_now() {
        let p = profile_for_product(crate::output::GLASSES_PRODUCT).unwrap();
        assert_eq!(p.id, "1s");
        assert_eq!(p.controls, &[HEAD_MOUSE, STEAMVR]);
        assert!(profile_for_product(0x4103).is_none());
    }

    #[test]
    fn the_report_parses_and_carries_every_field_a_front_end_needs() {
        let text = serde_json::to_string(&render(&facts())).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        for path in ["device/connected", "device/identified_by", "device/model", "device/name", "device/supported", "device/controls", "runtime/installed", "runtime/version", "units/xreal-linux.socket", "units/xreal-linux.service"] {
            let (mut cur, mut ok) = (v.clone(), true);
            for key in path.split('/') {
                match cur.get(key) {
                    Some(next) => cur = next.clone(),
                    None => ok = false,
                }
            }
            assert!(ok, "missing field {path} in {v}");
        }
    }
}
