//! Trails (`docs/stage-12.md` phase 3): a [`Trail`] on an object leaves a
//! strip behind it as it moves, as Unity's Trail Renderer does: a ball's
//! streak, a sword's swipe. A point is added each time the object has gone
//! `min_vertex_distance` from the last, and each lives `time` seconds; the
//! strip runs from the object through them, its width and colour over its
//! length, the head (0) to the tail (1). It's drawn facing the camera,
//! with the material as particles are.
//!
//! [`World::step_particles`] moves the trails too, on the game's time. An
//! object that's hidden clears its trail. The points are a fixed ring of
//! [`MAX_TRAIL_POINTS`], the oldest dropped when it's full, so a frame
//! allocates nothing.

use std::collections::VecDeque;

use glam::Vec3;

use crate::particles::{DrawnParticle, TABLE, sample, sample_color};
use crate::world::{Result, World, WorldError};
use crate::{Color, MaterialId, ObjectId};

/// The most points a trail keeps.
pub const MAX_TRAIL_POINTS: usize = 512;

/// A trail's settings (Unity's `TrailRenderer`).
#[derive(Clone, Debug, PartialEq)]
pub struct TrailSettings {
    /// How long each point lives, in seconds.
    pub time: f32,
    /// How far, in metres, the object goes before another point is added.
    pub min_vertex_distance: f32,
    /// Its width, in metres, times `width_curve`.
    pub width: f32,
    /// Over its length, head to tail; `None` for the same all along.
    pub width_curve: Option<Box<[f32; TABLE]>>,
    /// Over its length, head to tail; `None` for white.
    pub color: Option<Box<[Color; TABLE]>>,
    /// Adding points; off, the ones it has live out their time.
    pub emitting: bool,
    /// Drawn with it, as particles are; `None` isn't drawn.
    pub material: Option<MaterialId>,
}

impl Default for TrailSettings {
    fn default() -> Self {
        Self {
            time: 5.0,
            min_vertex_distance: 0.1,
            width: 1.0,
            width_curve: None,
            color: None,
            emitting: true,
            material: None,
        }
    }
}

impl TrailSettings {
    fn refused(&self) -> Option<&'static str> {
        if !(self.time.is_finite() && self.time >= 0.0) {
            return Some("a trail's time is seconds of 0 or more");
        }
        if !(self.min_vertex_distance.is_finite() && self.min_vertex_distance >= 0.0) {
            return Some("a trail's min vertex distance is metres of 0 or more");
        }
        if !(self.width.is_finite() && self.width >= 0.0) {
            return Some("a trail's width is metres of 0 or more");
        }
        if self.width_curve.as_ref().is_some_and(|t| t.iter().any(|v| !v.is_finite())) {
            return Some("a trail's width curve has a number that isn't finite");
        }
        if self.color.as_ref().is_some_and(|t| t.iter().any(|c| !c.to_linear().iter().all(|v| v.is_finite()))) {
            return Some("a trail's colours have a number that isn't finite");
        }
        None
    }
}

/// A trail and its points.
#[derive(Debug)]
pub struct Trail {
    settings: TrailSettings,
    /// `settings.color`, linear.
    color_table: Option<Box<[[f32; 4]; TABLE]>>,
    /// Where each point is, in the world, and when it was added, the
    /// oldest first.
    points: VecDeque<(Vec3, f32)>,
    /// The last point dropped, while the oldest is still alive: the tail
    /// is drawn between the two, where a point `time` old would be, so it
    /// shortens smoothly rather than a point at a time.
    tail: Option<(Vec3, f32)>,
    /// Where the object was last step: the head.
    head: Option<Vec3>,
    clock: f32,
    shown: bool,
}

impl Trail {
    fn new(settings: TrailSettings) -> Self {
        let mut trail = Self {
            settings: TrailSettings::default(),
            color_table: None,
            points: VecDeque::with_capacity(MAX_TRAIL_POINTS),
            tail: None,
            head: None,
            clock: 0.0,
            shown: false,
        };
        trail.set(settings);
        trail
    }

    fn set(&mut self, settings: TrailSettings) {
        self.color_table = settings.color.as_ref().map(|t| Box::new(t.map(Color::to_linear)));
        self.settings = settings;
    }

    pub fn settings(&self) -> &TrailSettings {
        &self.settings
    }

    /// Its points, the head's not counted.
    pub fn count(&self) -> usize {
        self.points.len()
    }

    /// Takes every point away (Unity's `Clear`).
    pub fn clear(&mut self) {
        self.points.clear();
        self.tail = None;
    }

    /// `seconds` on, its object at `here`.
    fn step(&mut self, seconds: f32, here: Vec3) {
        self.clock += seconds;
        let s = &self.settings;
        if s.emitting {
            let far = self.points.back().is_none_or(|&(last, _)| last.distance(here) >= s.min_vertex_distance);
            if far {
                if self.points.len() == MAX_TRAIL_POINTS {
                    self.tail = self.points.pop_front();
                }
                self.points.push_back((here, self.clock));
            }
        }
        let gone = self.clock - s.time;
        while self.points.front().is_some_and(|&(_, born)| born < gone) {
            self.tail = self.points.pop_front();
        }
        if self.points.is_empty() {
            self.tail = None;
        }
        self.head = Some(here);
    }

    /// Its points as they're drawn, into `out`, the head first, each with
    /// its width (as `size`), colour, the way the strip goes there (as
    /// `velocity`) and how far along it is, 0 to 1 (as `t`). `None` when
    /// there's nothing to draw; else the box they're in, wide enough for
    /// the strip.
    pub fn drawn(&self, out: &mut Vec<DrawnParticle>) -> Option<(Vec3, Vec3)> {
        let s = &self.settings;
        let head = self.head.filter(|_| s.emitting && !self.points.is_empty());
        let tail = self.tail.zip(self.points.front()).map(|((p, born), &(next, next_born))| {
            let f = ((self.clock - s.time - born) / (next_born - born).max(1e-6)).clamp(0.0, 1.0);
            p.lerp(next, f)
        });
        let from = out.len();
        let mut add = |position: Vec3| {
            out.push(DrawnParticle {
                position,
                size: 0.0,
                color: [1.0; 4],
                rotation: 0.0,
                velocity: Vec3::ZERO,
                frame: 0,
                t: 0.0,
                random: 0.0,
            });
        };
        // The head, unless it's still on the newest point.
        if let Some(head) = head.filter(|h| self.points.back().is_none_or(|&(p, _)| p.distance_squared(*h) > 1e-10)) {
            add(head);
        }
        for &(p, _) in self.points.iter().rev() {
            add(p);
        }
        if let Some(tail) = tail {
            add(tail);
        }
        let points = &mut out[from..];
        if points.len() < 2 {
            out.truncate(from);
            return None;
        }
        // How far along each is, then what that gives it.
        let mut length = 0.0;
        for i in 1..points.len() {
            length += points[i].position.distance(points[i - 1].position);
            points[i].t = length;
        }
        let n = points.len();
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let mut widest = 0.0f32;
        for i in 0..n {
            let t = if length > 0.0 { points[i].t / length } else { i as f32 / (n - 1) as f32 };
            let along = points[(i + 1).min(n - 1)].position - points[i.saturating_sub(1)].position;
            let p = &mut points[i];
            p.t = t;
            p.size = s.width * s.width_curve.as_deref().map_or(1.0, |table| sample(table, t));
            p.color = self.color_table.as_deref().map_or([1.0; 4], |table| sample_color(table, t));
            p.velocity = along.normalize_or_zero();
            widest = widest.max(p.size);
            lo = lo.min(p.position);
            hi = hi.max(p.position);
        }
        Some((lo - Vec3::splat(widest * 0.5), hi + Vec3::splat(widest * 0.5)))
    }
}

impl World {
    /// Gives `object` a trail with `settings`, or changes the one it has
    /// (keeping its points). Refused for a missing object or material, or
    /// settings that can't be.
    pub fn set_trail(&mut self, object: ObjectId, settings: TrailSettings) -> Result<()> {
        self.entity(object)?;
        if let Some(material) = settings.material {
            self.material(material)?;
        }
        if let Some(why) = settings.refused() {
            return Err(WorldError::Particles(why));
        }
        match self.trails.get_mut(&object) {
            Some(trail) => trail.set(settings),
            None => {
                self.trails.insert(object, Trail::new(settings));
            }
        }
        Ok(())
    }

    /// Takes `object`'s trail away.
    pub fn remove_trail(&mut self, object: ObjectId) -> Result<()> {
        self.entity(object)?;
        self.trails.remove(&object).ok_or(WorldError::NoComponent { object, component: "TrailRenderer" })?;
        Ok(())
    }

    pub fn trail(&self, object: ObjectId) -> Result<&Trail> {
        self.entity(object)?;
        self.trails.get(&object).ok_or(WorldError::NoComponent { object, component: "TrailRenderer" })
    }

    /// Takes every point of `object`'s trail away.
    pub fn clear_trail(&mut self, object: ObjectId) -> Result<()> {
        self.entity(object)?;
        self.trails.get_mut(&object).ok_or(WorldError::NoComponent { object, component: "TrailRenderer" })?.clear();
        Ok(())
    }

    pub fn has_trails(&self) -> bool {
        !self.trails.is_empty()
    }

    /// Every trail, by its object, in no particular order.
    pub fn trails(&self) -> impl Iterator<Item = (ObjectId, &Trail)> + '_ {
        self.trails.iter().map(|(&id, t)| (id, t))
    }

    /// Moves every trail `dt` seconds on: a point where its object is, if
    /// it's gone far enough, and the old ones gone. A hidden object's is
    /// cleared.
    pub(crate) fn step_trails(&mut self, dt: f32) {
        if self.trails.is_empty() {
            return;
        }
        let mut trails = std::mem::take(&mut self.trails);
        for (&object, trail) in &mut trails {
            let shown = self.active_in_hierarchy(object).unwrap_or(false);
            if !shown {
                if trail.shown {
                    trail.clear();
                    trail.head = None;
                }
                trail.shown = false;
                continue;
            }
            trail.shown = true;
            let Ok(m) = self.world_matrix(object) else { continue };
            trail.step(dt, m.w_axis.truncate());
        }
        self.trails = trails;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform;

    fn moved(world: &mut World, id: ObjectId, x: f32) {
        world.insert(id, Transform { position: Vec3::new(x, 0.0, 0.0), ..Transform::default() }).unwrap();
    }

    fn drawn(world: &World, id: ObjectId) -> Vec<DrawnParticle> {
        let mut out = Vec::new();
        world.trail(id).unwrap().drawn(&mut out);
        out
    }

    fn trail(settings: TrailSettings) -> (World, ObjectId) {
        let mut world = World::new();
        let id = world.spawn();
        moved(&mut world, id, 0.0);
        world.set_trail(id, settings).unwrap();
        (world, id)
    }

    #[test]
    fn a_trail_leaves_a_point_each_min_vertex_distance_and_each_lives_its_time() {
        let (mut world, id) = trail(TrailSettings { time: 1.0, min_vertex_distance: 0.25, ..Default::default() });
        // 0.1 m a frame, 60 frames a second, for half a second.
        for k in 0..=30 {
            moved(&mut world, id, k as f32 * 0.1);
            world.step_trails(1.0 / 60.0);
        }
        let n = world.trail(id).unwrap().count();
        assert!((10..=11).contains(&n), "3 m at a point each 0.3 m: {n}");
        let points = drawn(&world, id);
        assert_eq!(points[0].position.x, 3.0, "from the head");
        assert_eq!((points[0].t, points.last().unwrap().t), (0.0, 1.0));
        assert!(points.windows(2).all(|w| w[0].position.x > w[1].position.x && w[0].t < w[1].t));
        assert!(points.iter().all(|p| p.velocity.x < -0.99), "the strip goes back the way it came");

        // Standing still, the tail shortens smoothly until none is left.
        let mut last = points.last().unwrap().position.x;
        for _ in 0..59 {
            world.step_trails(1.0 / 60.0);
            let points = drawn(&world, id);
            let tail = points.last().unwrap().position.x;
            assert!(tail >= last && tail - last < 0.2, "{last} to {tail}");
            last = tail;
        }
        for _ in 0..3 {
            world.step_trails(1.0 / 60.0);
        }
        assert_eq!(world.trail(id).unwrap().count(), 1, "where it stands, all that's left");
        assert!(drawn(&world, id).is_empty());
    }

    #[test]
    fn its_width_and_colour_go_head_to_tail_and_hiding_it_clears_it() {
        let red_to_clear: [Color; TABLE] =
            std::array::from_fn(|i| Color::rgba(1.0, 0.0, 0.0, 1.0 - i as f32 / (TABLE - 1) as f32));
        let (mut world, id) = trail(TrailSettings {
            width: 0.5,
            width_curve: Some(Box::new(std::array::from_fn(|i| 1.0 - i as f32 / (TABLE - 1) as f32))),
            color: Some(Box::new(red_to_clear)),
            min_vertex_distance: 0.0,
            ..Default::default()
        });
        for k in 0..=10 {
            moved(&mut world, id, k as f32);
            world.step_trails(0.1);
        }
        let points = drawn(&world, id);
        let (head, tail) = (points[0], *points.last().unwrap());
        assert_eq!((head.size, head.color), (0.5, [1.0, 0.0, 0.0, 1.0]));
        assert!(tail.size.abs() < 1e-5 && tail.color[3].abs() < 1e-5);
        let middle = points.iter().find(|p| p.position.x == 5.0).unwrap();
        assert!((middle.size - 0.25).abs() < 0.01, "{}", middle.size);

        let mut out = Vec::new();
        let (lo, hi) = world.trail(id).unwrap().drawn(&mut out).unwrap();
        assert_eq!((lo.x, hi.x), (-0.25, 10.25), "wide enough for the strip");

        world.set_active(id, false).unwrap();
        world.step_trails(0.1);
        world.set_active(id, true).unwrap();
        world.step_trails(0.1);
        assert_eq!(world.trail(id).unwrap().count(), 1, "a hidden object's trail is cleared");
        assert!(drawn(&world, id).is_empty());
    }

    #[test]
    fn not_emitting_adds_no_points_and_a_full_trail_drops_its_oldest() {
        let (mut world, id) =
            trail(TrailSettings { emitting: false, min_vertex_distance: 0.0, time: 100.0, ..Default::default() });
        for k in 0..10 {
            moved(&mut world, id, k as f32);
            world.step_trails(0.1);
        }
        assert_eq!(world.trail(id).unwrap().count(), 0);
        let mut settings = world.trail(id).unwrap().settings().clone();
        settings.emitting = true;
        world.set_trail(id, settings).unwrap();
        for k in 0..MAX_TRAIL_POINTS + 10 {
            moved(&mut world, id, k as f32);
            world.step_trails(0.01);
        }
        assert_eq!(world.trail(id).unwrap().count(), MAX_TRAIL_POINTS);
        assert!(world.set_trail(id, TrailSettings { time: -1.0, ..Default::default() }).is_err());
        world.despawn(id).unwrap();
        assert_eq!(world.trails().count(), 0);
    }
}
