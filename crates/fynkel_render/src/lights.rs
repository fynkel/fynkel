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

/// One record's light list and what it was computed from.
#[derive(Clone, Copy)]
pub(crate) struct CachedLights {
    pub list: [u32; 12],
    pub count: u32,
    pub overflow: bool,
    pub candidates: u32,
}

/// What is kept for one record: the pool's lights that can reach a sphere
/// a little larger than its bounds (the spatial lookup, the expensive part),
/// and the list last chosen from them for exact bounds and a layer.
#[derive(Default)]
struct Kept {
    seen: u64,
    query: Option<Sphere>,
    candidates: Vec<usize>,
    chosen: Option<(Sphere, u32, CachedLights)>,
}

/// Light lists kept across frames. A list is a function of the pool, the
/// per-object budget, the object's bounds and its layer alone. Kept work is
/// trusted only from the frame before, and only where no light that changed
/// since then reaches it, before or after the change; the lookup's
/// candidates hold while the bounds stay inside the sphere they were found
/// for, and the choice among them is made again unless the bounds and layer
/// are exactly the same.
#[derive(Default)]
pub(crate) struct LightCache {
    entries: Vec<Kept>,
    pool: Vec<GpuLight>,
    masks: Vec<u32>,
    limit: usize,
    frame: u64,
    /// Where the lights that changed this frame reached, before and after.
    changed: Vec<Sphere>,
}

/// Beyond this many changed lights a frame starts over instead.
const CHANGED_LIGHTS: usize = 32;
/// How much larger than the bounds the kept lookup is: a quarter of the
/// radius and a quarter metre, so a turning or slowly moving object keeps it.
const LOOKUP_SLACK: (f32, f32) = (1.25, 0.25);

fn reach(l: &GpuLight) -> Sphere {
    Sphere {
        center: Vec3::new(l.position[0], l.position[1], l.position[2]),
        radius: l.position[3],
    }
}

fn touches(a: &Sphere, b: &Sphere) -> bool {
    a.center.distance(b.center) <= a.radius + b.radius
}

impl LightCache {
    /// Compares this frame's pool and budget with the last frame's.
    pub fn begin(&mut self, pool: &[GpuLight], masks: &[u32], limit: usize) {
        self.frame += 1;
        self.changed.clear();
        let comparable =
            limit == self.limit && pool.len() == self.pool.len() && masks == self.masks.as_slice();
        let mut fresh = !comparable;
        if comparable {
            for (old, new) in self.pool.iter().zip(pool) {
                if bytemuck::bytes_of(old) != bytemuck::bytes_of(new) {
                    if self.changed.len() >= 2 * CHANGED_LIGHTS {
                        fresh = true;
                        break;
                    }
                    self.changed.push(reach(old));
                    self.changed.push(reach(new));
                }
            }
        }
        if fresh {
            // Nothing kept from an earlier frame is trusted (`seen`).
            self.frame += 1;
            self.changed.clear();
            self.masks.clear();
            self.masks.extend_from_slice(masks);
            self.limit = limit;
        }
        self.pool.clear();
        self.pool.extend_from_slice(pool);
    }

    /// `slot`'s light list for `sphere` and `layer`: kept, chosen again from
    /// kept candidates, or looked up in `index` and chosen.
    #[allow(clippy::too_many_arguments)]
    pub fn lights(
        &mut self,
        slot: usize,
        sphere: &Sphere,
        layer: u32,
        index: &mut spatial::SpatialIndex,
        pool: &[GpuLight],
        masks: &[u32],
        scratch: &mut Vec<(f32, u32)>,
    ) -> CachedLights {
        if self.entries.len() <= slot {
            self.entries.resize_with(slot + 1, Kept::default);
        }
        let frame = self.frame;
        let kept = &mut self.entries[slot];
        let current = kept.seen + 1 == frame || kept.seen == frame;
        let holds = kept.query.is_some_and(|q| {
            current
                && q.center.distance(sphere.center) + sphere.radius <= q.radius
                && !self.changed.iter().any(|c| touches(c, &q))
        });
        kept.seen = frame;
        if !holds {
            let query = Sphere {
                center: sphere.center,
                radius: sphere.radius * LOOKUP_SLACK.0 + LOOKUP_SLACK.1,
            };
            index.overlaps(&query, &mut kept.candidates);
            kept.query = Some(query).filter(|q| q.is_finite());
            return choose(kept, sphere, layer, pool, masks, scratch, self.limit);
        }
        match kept.chosen {
            Some((s, l, chosen)) if s == *sphere && l == layer => chosen,
            _ => choose(kept, sphere, layer, pool, masks, scratch, self.limit),
        }
    }
}

fn choose(
    kept: &mut Kept,
    sphere: &Sphere,
    layer: u32,
    pool: &[GpuLight],
    masks: &[u32],
    scratch: &mut Vec<(f32, u32)>,
    limit: usize,
) -> CachedLights {
    let (list, count, overflow) =
        assign_lights(pool, masks, layer, sphere, scratch, limit, &kept.candidates);
    let chosen = CachedLights {
        list,
        count: count as u32,
        overflow,
        candidates: kept.candidates.len() as u32,
    };
    kept.chosen = Some((*sphere, layer, chosen));
    chosen
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

    /// Whatever moves, kept lists are the lists a fresh assignment over
    /// the whole pool gives.
    #[test]
    fn kept_light_lists_match_a_fresh_assignment_frame_after_frame() {
        let mut cache = LightCache::default();
        let mut index = spatial::SpatialIndex::default();
        let mut scratch = Vec::new();
        let mut fresh_scratch = Vec::new();
        let mut seed = 7u32;
        let mut rand = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut pool: Vec<GpuLight> = (0..40)
            .map(|_| GpuLight {
                position: [rand() * 40.0, 3.0, rand() * 40.0, 4.0 + rand() * 8.0],
                color: [rand() * 300.0, rand() * 300.0, rand() * 300.0, 0.0],
                ..Default::default()
            })
            .collect();
        let masks: Vec<u32> = (0..40)
            .map(|i| if i % 7 == 0 { 2 } else { u32::MAX })
            .collect();
        let mut objects: Vec<(Sphere, u32)> = (0..120)
            .map(|i| {
                (
                    Sphere {
                        center: Vec3::new(rand() * 40.0, rand() * 2.0, rand() * 40.0),
                        radius: 0.3 + rand(),
                    },
                    if i % 5 == 0 { 2 } else { 1 },
                )
            })
            .collect();
        let mut hits = 0;
        for frame in 0..60 {
            // Objects drift (a few by more than the slack), one light walks,
            // now and then a layer changes and once the pool shrinks.
            for (n, (s, layer)) in objects.iter_mut().enumerate() {
                if n % 3 == frame % 3 {
                    s.center.x += if n % 11 == 0 { 0.9 } else { 0.01 };
                }
                if frame == 30 && n % 13 == 0 {
                    *layer ^= 3;
                }
            }
            pool[5].position[0] += 0.5;
            if frame == 45 {
                pool.truncate(35);
            }
            let n = pool.len();
            let bounds: Vec<Sphere> = pool.iter().map(reach).collect();
            index.update(&bounds);
            cache.begin(&pool, &masks[..n], 4);
            let all: Vec<usize> = (0..n).collect();
            for (slot, (sphere, layer)) in objects.iter().enumerate() {
                let kept = cache.lights(
                    slot,
                    sphere,
                    *layer,
                    &mut index,
                    &pool,
                    &masks[..n],
                    &mut scratch,
                );
                let fresh = assign_lights(
                    &pool,
                    &masks[..n],
                    *layer,
                    sphere,
                    &mut fresh_scratch,
                    4,
                    &all,
                );
                assert_eq!(
                    (kept.list, kept.count as usize, kept.overflow),
                    fresh,
                    "frame {frame} slot {slot}"
                );
                hits += usize::from(kept.candidates as usize != n);
            }
        }
        assert!(hits > 0);
    }
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
