//! A model's animations as plain values, and sampling them
//! (`docs/stage-9.md` phase 1).
//!
//! A clip is a list of channels; each moves one node's position, rotation
//! or scale through keys, with glTF's three kinds of interpolation. What a
//! channel gives at a time is [`ChannelData::sample`]: no state, so any
//! number of copies of a model play the same clip at their own times.

/// One of a model's animations: everything it moves, and how long it is.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipData {
    /// The file's name for it, or `animation N` (its index) for none.
    pub name: String,
    /// Seconds: its last key's time, 0 for a clip with no keys.
    pub length: f32,
    pub channels: Vec<ChannelData>,
}

/// What a channel moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Property {
    Translation,
    Rotation,
    Scale,
}

/// How a channel goes from one key to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Each key's value until the next key.
    Step,
    /// In a straight line; a rotation along the shortest arc (slerp).
    Linear,
    /// A curve through the keys with tangents in the file (glTF's
    /// `CUBICSPLINE`).
    Cubic,
}

/// One node's position, rotation or scale through a clip.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelData {
    /// An index into [`crate::ModelData::nodes`].
    pub node: usize,
    pub property: Property,
    pub interpolation: Interpolation,
    /// Seconds, increasing, at least one.
    pub times: Vec<f32>,
    /// A value a key: xyz (w 0) for a translation or scale, xyzw for a
    /// rotation (a unit quaternion). For [`Interpolation::Cubic`], three a
    /// key: the in-tangent, the value and the out-tangent.
    pub values: Vec<[f32; 4]>,
}

impl ChannelData {
    /// The value at `time` seconds: the first key's before it, the last
    /// key's after the last. A rotation comes out a unit quaternion.
    pub fn sample(&self, time: f32) -> [f32; 4] {
        let times = &self.times;
        let last = times.len() - 1;
        let cubic = self.interpolation == Interpolation::Cubic;
        let value = |key: usize| if cubic { self.values[key * 3 + 1] } else { self.values[key] };
        if time <= times[0] || last == 0 {
            return value(0);
        }
        if time >= times[last] {
            return value(last);
        }
        // The key at or before `time`: times[key] <= time < times[key + 1].
        let key = times.partition_point(|&t| t <= time) - 1;
        let span = times[key + 1] - times[key];
        let s = if span > 0.0 { (time - times[key]) / span } else { 0.0 };
        let rotation = self.property == Property::Rotation;
        match self.interpolation {
            Interpolation::Step => value(key),
            Interpolation::Linear if rotation => slerp(value(key), value(key + 1), s),
            Interpolation::Linear => lerp(value(key), value(key + 1), s),
            Interpolation::Cubic => {
                // Hermite, with the tangents scaled by the keys' distance,
                // as glTF's appendix C says.
                let (p0, m0) = (self.values[key * 3 + 1], self.values[key * 3 + 2]);
                let (m1, p1) = (self.values[(key + 1) * 3], self.values[(key + 1) * 3 + 1]);
                let (s2, s3) = (s * s, s * s * s);
                let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
                let h10 = (s3 - 2.0 * s2 + s) * span;
                let h01 = -2.0 * s3 + 3.0 * s2;
                let h11 = (s3 - s2) * span;
                let v = std::array::from_fn(|i| h00 * p0[i] + h10 * m0[i] + h01 * p1[i] + h11 * m1[i]);
                if rotation { normalize(v) } else { v }
            }
        }
    }
}

fn lerp(a: [f32; 4], b: [f32; 4], s: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * s)
}

fn dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    a.iter().zip(&b).map(|(x, y)| x * y).sum()
}

/// `q` scaled to unit length; the identity for a zero one.
pub fn normalize(q: [f32; 4]) -> [f32; 4] {
    let length = dot(q, q).sqrt();
    if length > 1e-12 { q.map(|x| x / length) } else { [0.0, 0.0, 0.0, 1.0] }
}

/// From `a` to `b` along the shortest arc, `s` of the way.
pub fn slerp(a: [f32; 4], b: [f32; 4], s: f32) -> [f32; 4] {
    let mut cos = dot(a, b);
    // The same rotation the shorter way round.
    let b = if cos < 0.0 {
        cos = -cos;
        b.map(|x| -x)
    } else {
        b
    };
    // Nearly the same: a straight line is as good, and doesn't divide by
    // a sine near 0.
    if cos > 0.9995 {
        return normalize(lerp(a, b, s));
    }
    let angle = cos.acos();
    let sin = angle.sin();
    let (wa, wb) = (((1.0 - s) * angle).sin() / sin, (s * angle).sin() / sin);
    std::array::from_fn(|i| wa * a[i] + wb * b[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(property: Property, interpolation: Interpolation, times: &[f32], values: &[[f32; 4]]) -> ChannelData {
        ChannelData { node: 0, property, interpolation, times: times.to_vec(), values: values.to_vec() }
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn before_the_first_key_and_after_the_last_hold_them() {
        let c = channel(
            Property::Translation,
            Interpolation::Linear,
            &[1.0, 2.0],
            &[[1.0, 0.0, 0.0, 0.0], [3.0, 0.0, 0.0, 0.0]],
        );
        assert_eq!(c.sample(0.0), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(c.sample(5.0), [3.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn linear_is_a_straight_line_between_keys() {
        let c = channel(
            Property::Translation,
            Interpolation::Linear,
            &[0.0, 1.0, 3.0],
            &[[0.0; 4], [2.0, 4.0, 0.0, 0.0], [4.0, 0.0, 0.0, 0.0]],
        );
        assert!(close(c.sample(0.5), [1.0, 2.0, 0.0, 0.0]));
        assert!(close(c.sample(1.0), [2.0, 4.0, 0.0, 0.0]));
        assert!(close(c.sample(2.0), [3.0, 2.0, 0.0, 0.0]));
    }

    #[test]
    fn step_holds_each_key_until_the_next() {
        let c = channel(Property::Scale, Interpolation::Step, &[0.0, 1.0], &[[1.0; 4], [2.0; 4]]);
        assert_eq!(c.sample(0.99), [1.0; 4]);
        assert_eq!(c.sample(1.0), [2.0; 4]);
    }

    #[test]
    fn a_rotation_turns_at_an_even_rate_the_short_way_round() {
        // 0° to 90° about y: halfway is 45°.
        let h = std::f32::consts::FRAC_PI_4;
        let quarter = [0.0, h.sin(), 0.0, h.cos()];
        let c = channel(Property::Rotation, Interpolation::Linear, &[0.0, 1.0], &[[0.0, 0.0, 0.0, 1.0], quarter]);
        let e = std::f32::consts::FRAC_PI_8;
        assert!(close(c.sample(0.5), [0.0, e.sin(), 0.0, e.cos()]));
        // The same end given negated is the same rotation: still 45°, not
        // the long way round.
        let negated = channel(
            Property::Rotation,
            Interpolation::Linear,
            &[0.0, 1.0],
            &[[0.0, 0.0, 0.0, 1.0], quarter.map(|x| -x)],
        );
        assert!(close(negated.sample(0.5), [0.0, e.sin(), 0.0, e.cos()]));
    }

    #[test]
    fn cubic_passes_through_its_keys_and_follows_its_tangents() {
        // Keys 0 and 1 at t = 0 and 2, both tangents 0: a smooth S, halfway
        // at the middle, slower near the ends than a line.
        let c = channel(
            Property::Translation,
            Interpolation::Cubic,
            &[0.0, 2.0],
            &[[0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [1.0, 0.0, 0.0, 0.0], [0.0; 4]],
        );
        assert!(close(c.sample(0.0), [0.0; 4]));
        assert!(close(c.sample(1.0), [0.5, 0.0, 0.0, 0.0]));
        assert!(close(c.sample(2.0), [1.0, 0.0, 0.0, 0.0]));
        assert!(c.sample(0.5)[0] < 0.25);
        // A tangent of 1 a second out of key 0 and into key 1: a straight
        // line from 0 to 2 over 2 seconds.
        let line = channel(
            Property::Translation,
            Interpolation::Cubic,
            &[0.0, 2.0],
            &[[0.0; 4], [0.0; 4], [1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [2.0, 0.0, 0.0, 0.0], [0.0; 4]],
        );
        assert!(close(line.sample(0.5), [0.5, 0.0, 0.0, 0.0]));
    }

    #[test]
    fn one_key_is_that_value_always() {
        let c = channel(Property::Translation, Interpolation::Linear, &[0.5], &[[7.0, 0.0, 0.0, 0.0]]);
        assert_eq!(c.sample(0.0), [7.0, 0.0, 0.0, 0.0]);
        assert_eq!(c.sample(9.0), [7.0, 0.0, 0.0, 0.0]);
    }
}
