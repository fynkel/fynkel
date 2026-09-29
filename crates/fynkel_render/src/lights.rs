//! Which lights reach an object: the frame's pool and each object's nearest
//! eight.

use super::*;

/// An instance on its way into the buffer: its data, where it is drawn,
/// and the light list that goes into the same slot.
pub(crate) type Grouped = (Instance, Visibility, [u32; 4]);

/// The lights of `pool` that reach `sphere`, packed for an [`Instance`]:
/// up to [`MAX_LIGHTS`] pool indices one per byte (the first four in `x`,
/// the next four in `y`) and how many in `z`. Returns that list, its
/// length and whether lights were left out. When more than the budget
/// reach one object the strongest at its bounds win — the weakest are the
/// ones a viewer is least likely to miss, and the choice is stable while
/// nothing moves.
pub(crate) fn assign_lights(
    pool: &[GpuLight],
    masks: &[u32],
    layer_bit: u32,
    sphere: &Sphere,
    scratch: &mut Vec<(f32, u8)>,
) -> ([u32; 4], usize, bool) {
    scratch.clear();
    for (i, l) in pool.iter().enumerate() {
        // A light whose `culling_mask` leaves the object's layer out
        // (ADR-0020) does not light it.
        if masks[i] & layer_bit == 0 {
            continue;
        }
        let center = Vec3::new(l.position[0], l.position[1], l.position[2]);
        let range = l.position[3];
        // How far the light is from the nearest point of the bounds.
        let d = (center.distance(sphere.center) - sphere.radius).max(0.0);
        if d >= range {
            continue;
        }
        // The shader's own windowed inverse square there, on the light's
        // brightest channel: the most this object can get from it.
        let ratio = d / range;
        let window = 1.0 - ratio * ratio * ratio * ratio;
        let peak = l.color[0].max(l.color[1]).max(l.color[2]);
        scratch.push((peak * window * window / d.max(1e-2).powi(2), i as u8));
    }
    let overflow = scratch.len() > MAX_LIGHTS;
    if overflow {
        scratch.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        scratch.truncate(MAX_LIGHTS);
    }
    let mut list = [0u32; 4];
    for (n, (_, i)) in scratch.iter().enumerate() {
        list[n / 4] |= u32::from(*i) << ((n % 4) * 8);
    }
    list[2] = scratch.len() as u32;
    (list, scratch.len(), overflow)
}

/// The per-instance data for a material at `world`.
/// Where an extra world's light stands in the frame (`transform`: none for
/// the main extract's, which is left bit for bit as it is): its position,
/// its direction and its range, scaled by the transform's largest axis.
pub(crate) fn place_light(
    transform: Option<&Mat4>,
    position: Vec3,
    direction: Vec3,
    range: f32,
) -> (Vec3, Vec3, f32) {
    match transform {
        None => (position, direction, range),
        Some(t) => {
            let scale = t
                .x_axis
                .truncate()
                .length()
                .max(t.y_axis.truncate().length())
                .max(t.z_axis.truncate().length());
            (
                t.transform_point3(position),
                t.transform_vector3(direction),
                range * scale,
            )
        }
    }
}
