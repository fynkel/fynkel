//! 3D sound (docs/stage-11.md §3.3): how loud a voice is in each ear, from
//! where it is around the listener. Worked out on the game's side, once a
//! frame, as a [`Mix`] the mixer ramps to over its next callback.

use std::f32::consts::FRAC_PI_4;

/// How a 3D sound fades with distance, as Unity's rolloff modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rolloff {
    /// Full within the min distance, then falling as 1 over the distance,
    /// until the max distance, where it stops falling.
    #[default]
    Logarithmic,
    /// Full within the min distance, falling in a straight line to silence
    /// at the max distance.
    Linear,
}

/// Where a voice sits between 2D and 3D, and how a 3D one fades.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spatial {
    /// 0 is 2D (the same wherever it is: music, a click), 1 is 3D, and
    /// between is a blend, as Unity's `spatialBlend`.
    pub blend: f32,
    /// Left (-1) to right (1) for the 2D part, as Unity's `panStereo`.
    pub pan: f32,
    /// Within it, full volume.
    pub min_distance: f32,
    /// See [`Rolloff`].
    pub max_distance: f32,
    pub rolloff: Rolloff,
}

impl Default for Spatial {
    /// Unity's: 2D, in the middle, 1 to 500 metres, logarithmic.
    fn default() -> Self {
        Self { blend: 0.0, pan: 0.0, min_distance: 1.0, max_distance: 500.0, rolloff: Rolloff::Logarithmic }
    }
}

impl Spatial {
    /// Fails, saying why, when a value is out of range.
    pub fn check(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.blend) {
            return Err(format!("spatial blend {} isn't between 0 and 1", self.blend));
        }
        if !(-1.0..=1.0).contains(&self.pan) {
            return Err(format!("pan {} isn't between -1 and 1", self.pan));
        }
        if !(self.min_distance.is_finite() && self.min_distance > 0.0) {
            return Err(format!("min distance {} isn't above 0", self.min_distance));
        }
        if !(self.max_distance.is_finite() && self.max_distance > self.min_distance) {
            return Err(format!(
                "max distance {} isn't above the min distance {}",
                self.max_distance, self.min_distance
            ));
        }
        Ok(())
    }
}

/// Who hears: where, and which way they face (a unit quaternion x, y, z,
/// w). As every object in Fynkel, it looks along its -Z, with +X on its
/// right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

impl Default for Listener {
    /// At the origin, looking along -Z.
    fn default() -> Self {
        Self { position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0] }
    }
}

impl Listener {
    /// Fails, saying why, when it isn't finite or its rotation is none.
    pub fn check(&self) -> Result<(), String> {
        let [x, y, z, w] = self.rotation;
        let length = (x * x + y * y + z * z + w * w).sqrt();
        if !self.position.iter().all(|v| v.is_finite()) {
            return Err(format!("the listener's position {:?} isn't finite", self.position));
        }
        if !(length.is_finite() && length > 1e-6) {
            return Err(format!("the listener's rotation {:?} isn't one", self.rotation));
        }
        Ok(())
    }

    /// Its right and its forward, as unit directions.
    fn axes(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z, w] = self.rotation;
        let n = (x * x + y * y + z * z + w * w).sqrt();
        let (x, y, z, w) = (x / n, y / n, z / n, w / n);
        // The rotation's +X, and its -Z.
        let right = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)];
        let back = [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)];
        (right, [-back[0], -back[1], -back[2]])
    }
}

/// How a voice's clip goes into the output: `gains` are left from the
/// clip's left and right, then right from them; `dull` (0 to 1) is how
/// much its highs are cut, for a sound behind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub gains: [f32; 4],
    pub dull: f32,
}

impl Mix {
    /// Each side of the clip to its own side, as it is: a 2D sound in the
    /// middle.
    pub const STRAIGHT: Self = Self { gains: [1.0, 0.0, 0.0, 1.0], dull: 0.0 };

    /// How loud it is in each ear, for a sound the same in both of the
    /// clip's sides.
    pub fn ears(&self) -> (f32, f32) {
        (self.gains[0] + self.gains[1], self.gains[2] + self.gains[3])
    }
}

/// How much quieter a sound right behind is.
const BEHIND_QUIETER: f32 = 0.3;

/// Left and right gains for `pan` (-1 to 1): equal-power, scaled so the
/// middle is as loud as a 2D sound, so the near ear stays at 1 and the far
/// one falls to 0 at the side.
fn pan_gains(pan: f32) -> (f32, f32) {
    if pan == 0.0 {
        return (1.0, 1.0);
    }
    let angle = (pan + 1.0) * FRAC_PI_4;
    let s = std::f32::consts::SQRT_2;
    ((s * angle.cos()).min(1.0), (s * angle.sin()).min(1.0))
}

/// How loud a 3D sound is at `distance`.
pub fn rolloff(spatial: &Spatial, distance: f32) -> f32 {
    let (min, max) = (spatial.min_distance, spatial.max_distance);
    match spatial.rolloff {
        Rolloff::Logarithmic => min / distance.clamp(min, max),
        Rolloff::Linear => ((max - distance) / (max - min)).clamp(0.0, 1.0),
    }
}

/// The mix of a voice with `spatial`'s settings, at `position`, heard by
/// `listener`.
pub fn mix(spatial: &Spatial, position: [f32; 3], listener: &Listener) -> Mix {
    let (pl, pr) = pan_gains(spatial.pan);
    let flat = [pl, 0.0, 0.0, pr];
    let blend = spatial.blend;
    if blend <= 0.0 {
        return Mix { gains: flat, dull: 0.0 };
    }
    let to =
        [position[0] - listener.position[0], position[1] - listener.position[1], position[2] - listener.position[2]];
    let distance = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
    let (side, behind) = if distance > 1e-4 {
        let (right, forward) = listener.axes();
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        ((dot(to, right) / distance).clamp(-1.0, 1.0), (-dot(to, forward) / distance).max(0.0))
    } else {
        (0.0, 0.0)
    };
    let gain = rolloff(spatial, distance) * (1.0 - BEHIND_QUIETER * behind);
    let (al, ar) = pan_gains(side);
    // A 3D sound is heard from one place: both of the clip's sides, half
    // each, to each ear.
    let (l, r) = (0.5 * al * gain, 0.5 * ar * gain);
    let around = [l, l, r, r];
    let mut gains = [0.0; 4];
    for i in 0..4 {
        gains[i] = flat[i] + (around[i] - flat[i]) * blend;
    }
    Mix { gains, dull: behind * blend }
}
