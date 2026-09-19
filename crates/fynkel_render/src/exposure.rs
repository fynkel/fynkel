//! Exposure in physical units (blueprint §19.2d item 4 scope rules): the
//! renderer shades in nits, the camera's EV100 decides how many nits are
//! white. Lagarde & de Rousiers (Frostbite, 2014) conventions.

use fynkel_core::{Camera, ExposureMode};

/// Illuminance the host's fallback light stands for when the scene has no
/// `DirectionalLight` (a clear-sky sun, the `DirectionalLight` default).
pub const FALLBACK_SUN_LUX: f32 = 10_000.0;

/// EV100 an incident-light meter reads for `lux` (ISO 100, calibration
/// constant C = 250): `log2(E / 2.5)`. 10 000 lux → 11.97.
pub fn ev100_from_lux(lux: f32) -> f32 {
    (lux.max(1.0) / 2.5).log2()
}

/// Automatic exposure: metered from the strongest constant illuminance in
/// the scene — the sun or the ambient — never below 1 lux (EV −1.3).
pub fn ev100_auto(sun_lux: f32, ambient_lux: f32) -> f32 {
    ev100_from_lux(sun_lux.max(ambient_lux))
}

/// The camera's EV100: its own when `exposure_mode` is manual, else
/// [`ev100_auto`].
pub fn ev100_for(camera: Option<&Camera>, sun_lux: f32, ambient_lux: f32) -> f32 {
    match camera {
        Some(c) if c.exposure_mode == ExposureMode::Manual && c.exposure.is_finite() => c.exposure,
        _ => ev100_auto(sun_lux, ambient_lux),
    }
}

/// The luminance (nits) that maps to white at `ev100`: `1.2 · 2^EV100`.
/// A white Lambertian surface facing the metered sun lands at ~0.66.
pub fn white_nits(ev100: f32) -> f32 {
    1.2 * 2f32.powf(ev100)
}

/// The factor scene radiance is multiplied by before tone mapping.
pub fn exposure_factor(ev100: f32) -> f32 {
    1.0 / white_nits(ev100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clear_sky_sun_meters_near_ev_12() {
        let ev = ev100_from_lux(FALLBACK_SUN_LUX);
        assert!((ev - 11.97).abs() < 0.01, "{ev}");
        // Every stop of light is one EV.
        assert!((ev100_from_lux(20_000.0) - ev - 1.0).abs() < 1e-5);
        assert!((ev100_from_lux(5_000.0) - ev + 1.0).abs() < 1e-5);
        // Floor: darkness does not send EV to -inf.
        assert_eq!(ev100_from_lux(0.0), ev100_from_lux(1.0));
        assert!(ev100_from_lux(0.0) > -2.0);
    }

    #[test]
    fn auto_takes_the_brighter_of_sun_and_ambient() {
        assert_eq!(ev100_auto(10_000.0, 2_000.0), ev100_from_lux(10_000.0));
        assert_eq!(ev100_auto(0.0, 2_000.0), ev100_from_lux(2_000.0));
    }

    #[test]
    fn manual_cameras_override_the_meter() {
        let auto = Camera::default();
        assert_eq!(auto.exposure_mode, ExposureMode::Auto);
        assert_eq!(
            ev100_for(Some(&auto), 10_000.0, 0.0),
            ev100_auto(10_000.0, 0.0)
        );
        let manual = Camera {
            exposure_mode: ExposureMode::Manual,
            exposure: 6.0,
            ..Camera::default()
        };
        assert_eq!(ev100_for(Some(&manual), 10_000.0, 0.0), 6.0);
        let nan = Camera {
            exposure: f32::NAN,
            ..manual
        };
        assert_eq!(
            ev100_for(Some(&nan), 10_000.0, 0.0),
            ev100_auto(10_000.0, 0.0)
        );
        assert_eq!(ev100_for(None, 10_000.0, 0.0), ev100_auto(10_000.0, 0.0));
    }

    #[test]
    fn white_point_follows_the_ev() {
        assert!((white_nits(12.0) - 4915.2).abs() < 0.1);
        assert!((white_nits(13.0) / white_nits(12.0) - 2.0).abs() < 1e-5);
        let l = 3183.0; // a white Lambertian surface under 10 000 lux
        let x = l * exposure_factor(ev100_from_lux(10_000.0));
        assert!((x - 0.66).abs() < 0.01, "{x}");
    }
}
