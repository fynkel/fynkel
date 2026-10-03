//! Spatial forward lighting with a bounded strongest-contribution list.

use super::*;

/// Supported forward-lighting budget. The shader layout bounds these limits;
/// larger requests are rejected rather than silently truncated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightBudget {
    pub pool: usize,
    pub per_object: usize,
}
impl Default for LightBudget {
    fn default() -> Self {
        Self {
            pool: LIGHT_POOL,
            per_object: MAX_LIGHTS,
        }
    }
}
impl LightBudget {
    pub fn validate(self) -> Result<Self, String> {
        if self.pool > MAX_LIGHT_POOL || self.per_object > MAX_LIGHTS || self.per_object > self.pool
        {
            return Err(format!("lighting supports at most {MAX_LIGHT_POOL} visible local lights and {MAX_LIGHTS} per object; per-object budget must fit the pool"));
        }
        Ok(self)
    }
}

/// An instance on its way into the buffer: its data, where it is drawn,
/// and the light list that goes into the same slot.
pub(crate) type Grouped = (Instance, Visibility, [u32; 12]);

/// The lights of `pool` that reach `sphere`, packed for an [`Instance`]:
/// eight full-width indices, followed by a count and three alignment words.
/// The GPU reads three vec4<u32> records per instance. Returns that list, its
/// length and whether lights were left out. When more than the budget
/// reach one object the strongest at its bounds win — the weakest are the
/// ones a viewer is least likely to miss, and the choice is stable while
/// nothing moves.
pub(crate) fn assign_lights(
    pool: &[GpuLight],
    masks: &[u32],
    layer_bit: u32,
    sphere: &Sphere,
    scratch: &mut Vec<(f32, u32)>,
    limit: usize,
    candidates: &[usize],
) -> ([u32; 12], usize, bool) {
    scratch.clear();
    for &i in candidates {
        let l = &pool[i];
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
        scratch.push((peak * window * window / d.max(1e-2).powi(2), i as u32));
    }
    let overflow = scratch.len() > limit;
    if overflow {
        scratch.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scratch.truncate(limit);
    }
    let mut list = [0u32; 12];
    for (n, (_, i)) in scratch.iter().enumerate() {
        list[n] = *i;
    }
    list[8] = scratch.len() as u32;
    (list, scratch.len(), overflow)
}

/// The per-instance data for a material at `world`.
/// Where an extra world's light stands in the frame (`transform`: none for
/// the main extract's, which is left bit for bit as it is): its position,
/// its direction and a conservative range under scale, reflection and shear.
pub(crate) fn place_light(
    transform: Option<&Mat4>,
    position: Vec3,
    direction: Vec3,
    range: f32,
) -> (Vec3, Vec3, f32) {
    match transform {
        None => (position, direction, range),
        Some(t) => {
            let influence = Sphere {
                center: position,
                radius: range,
            }
            .transformed(t);
            (
                influence.center,
                t.transform_vector3(direction),
                influence.radius,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_selection_is_stable_respects_masks_and_reports_overflow() {
        assert!(LightBudget {
            pool: MAX_LIGHT_POOL + 1,
            per_object: 8
        }
        .validate()
        .is_err());
        assert!(LightBudget {
            pool: 4,
            per_object: 5
        }
        .validate()
        .is_err());
        let light = GpuLight {
            position: [0.0, 0.0, 0.0, 10.0],
            color: [1.0; 4],
            ..Default::default()
        };
        let pool = [light; 20];
        let mut masks = [1; 20];
        masks[0] = 2;
        let mut scratch = Vec::new();
        let sphere = Sphere {
            center: Vec3::ZERO,
            radius: 1.0,
        };
        let expected = assign_lights(
            &pool,
            &masks,
            1,
            &sphere,
            &mut scratch,
            4,
            &(0..20).collect::<Vec<_>>(),
        );
        assert_eq!(expected.1, 4);
        assert!(expected.2);
        assert_eq!(&expected.0[..4], &[1, 2, 3, 4]);
        for _ in 0..100 {
            assert_eq!(
                assign_lights(
                    &pool,
                    &masks,
                    1,
                    &sphere,
                    &mut scratch,
                    4,
                    &(0..20).collect::<Vec<_>>()
                ),
                expected
            );
        }
        assert_eq!(
            assign_lights(
                &pool,
                &masks,
                4,
                &sphere,
                &mut scratch,
                4,
                &(0..20).collect::<Vec<_>>()
            )
            .1,
            0
        );
    }
}

#[derive(Default)]
pub(crate) struct LightFrameScratch {
    pub lights: Vec<(f32, GpuLight, u32)>,
    pub packed: Vec<GpuLight>,
    pub masks: Vec<u32>,
    pub bounds: Vec<Sphere>,
}
