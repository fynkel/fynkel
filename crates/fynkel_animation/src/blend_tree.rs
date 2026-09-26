//! Blend trees (ADR-0033 § 1): a state's motion as several motions mixed
//! by parameters — Unity's five types. This module is the data a
//! controller carries ([`TreeDef`]) and the weights ([`weights`]); the
//! world samples the children and blends their poses.
//!
//! The weights follow Unity's published methods, so a tree set up for
//! Unity behaves the same:
//! - `OneD`: linear between the two neighbours along one parameter,
//!   clamped at the ends;
//! - `SimpleDirectional`: barycentric in the wedge of the two directions
//!   around the input and the centre child (one child per direction);
//! - `FreeformCartesian`: gradient-band interpolation (Johansen 2009);
//! - `FreeformDirectional`: gradient-band interpolation in polar space
//!   (magnitude and angle, Johansen 2009), for several children per
//!   direction;
//! - `Direct`: each child's own parameter is its weight.
//!
//! Every result is normalized to sum to 1; a weight below
//! [`MIN_WEIGHT`] is dropped (the child is not sampled) and the rest
//! scaled up.

use fynkel_contracts::AssetRef;

/// A weight below this is dropped: the child is not sampled.
pub const MIN_WEIGHT: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    OneD,
    SimpleDirectional,
    FreeformDirectional,
    FreeformCartesian,
    Direct,
}

impl TreeKind {
    pub fn name(self) -> &'static str {
        match self {
            TreeKind::OneD => "1d",
            TreeKind::SimpleDirectional => "simpleDirectional",
            TreeKind::FreeformDirectional => "freeformDirectional",
            TreeKind::FreeformCartesian => "freeformCartesian",
            TreeKind::Direct => "direct",
        }
    }

    pub fn parse(s: &str) -> Option<TreeKind> {
        Some(match s {
            "1d" => TreeKind::OneD,
            "simpleDirectional" => TreeKind::SimpleDirectional,
            "freeformDirectional" => TreeKind::FreeformDirectional,
            "freeformCartesian" => TreeKind::FreeformCartesian,
            "direct" => TreeKind::Direct,
            _ => return None,
        })
    }

    /// How many parameters the tree reads: 1, 2, or none (a direct
    /// tree's children name their own).
    pub fn parameter_count(self) -> usize {
        match self {
            TreeKind::OneD => 1,
            TreeKind::Direct => 0,
            _ => 2,
        }
    }

    pub fn is_2d(self) -> bool {
        self.parameter_count() == 2
    }
}

/// What a state or a tree's child plays.
#[derive(Clone, Debug, PartialEq)]
pub enum MotionDef {
    /// A clip or a blend space document.
    Asset(AssetRef),
    Tree(Box<TreeDef>),
}

impl MotionDef {
    /// Every asset it plays, depth first.
    pub fn assets<'a>(&'a self, out: &mut Vec<&'a AssetRef>) {
        match self {
            MotionDef::Asset(a) => out.push(a),
            MotionDef::Tree(t) => {
                for c in &t.children {
                    c.motion.assets(out);
                }
            }
        }
    }

    pub fn as_asset(&self) -> Option<&AssetRef> {
        match self {
            MotionDef::Asset(a) => Some(a),
            MotionDef::Tree(_) => None,
        }
    }
}

impl From<AssetRef> for MotionDef {
    fn from(a: AssetRef) -> MotionDef {
        MotionDef::Asset(a)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildDef {
    pub motion: MotionDef,
    /// A 1D tree's threshold in `[0]`; a 2D tree's position; unused in a
    /// direct tree.
    pub position: [f32; 2],
    /// Scales the child's length in the tree's (its clock runs this much
    /// faster).
    pub time_scale: f32,
    /// A direct tree's: the float parameter that is this child's weight.
    pub parameter: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeDef {
    pub kind: TreeKind,
    /// Indices of the float parameters the tree reads (see
    /// [`TreeKind::parameter_count`]).
    pub parameters: Vec<usize>,
    pub children: Vec<ChildDef>,
}

impl TreeDef {
    /// The limits the schema and the cook hold (ADR-0033 § 1).
    pub const MAX_CHILDREN: usize = 32;
    pub const MAX_DEPTH: usize = 4;
    pub const MAX_LEAVES: usize = 64;

    /// The motions at the bottom of the tree, counting a nested tree's.
    pub fn leaf_count(&self) -> usize {
        self.children
            .iter()
            .map(|c| match &c.motion {
                MotionDef::Asset(_) => 1,
                MotionDef::Tree(t) => t.leaf_count(),
            })
            .sum()
    }

    /// How deep the tree is: 1 without nested trees.
    pub fn depth(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(|c| match &c.motion {
                MotionDef::Asset(_) => 0,
                MotionDef::Tree(t) => t.depth(),
            })
            .max()
            .unwrap_or(0)
    }

    /// The shape the weights rely on: the parameter count, finite
    /// positions and time scales, no two children at one position, the
    /// limits. `is_float(i)` says whether parameter `i` exists and is a
    /// float.
    pub fn validate(&self, is_float: &dyn Fn(usize) -> bool) -> Result<(), String> {
        let kind = self.kind.name();
        if self.parameters.len() != self.kind.parameter_count() {
            return Err(format!(
                "a {kind} tree reads {} parameter(s), not {}",
                self.kind.parameter_count(),
                self.parameters.len()
            ));
        }
        if self.parameters.iter().any(|&p| !is_float(p)) {
            return Err(format!("a {kind} tree's parameters must be floats"));
        }
        if self.children.is_empty() || self.children.len() > Self::MAX_CHILDREN {
            return Err(format!(
                "a blend tree has 1 to {} children",
                Self::MAX_CHILDREN
            ));
        }
        if self.depth() > Self::MAX_DEPTH {
            return Err(format!("blend trees nest at most {} deep", Self::MAX_DEPTH));
        }
        if self.leaf_count() > Self::MAX_LEAVES {
            return Err(format!(
                "a blend tree plays at most {} motions",
                Self::MAX_LEAVES
            ));
        }
        for (i, c) in self.children.iter().enumerate() {
            if !c.position.iter().all(|v| v.is_finite()) {
                return Err(format!("child {i}: the position is not finite"));
            }
            if !c.time_scale.is_finite() || c.time_scale <= 0.0 {
                return Err(format!("child {i}: the time scale must be above 0"));
            }
            match (self.kind, c.parameter) {
                (TreeKind::Direct, None) => {
                    return Err(format!(
                        "child {i}: a direct tree's child names a parameter"
                    ))
                }
                (TreeKind::Direct, Some(p)) if !is_float(p) => {
                    return Err(format!("child {i}: its parameter must be a float"))
                }
                _ => {}
            }
            if self.kind != TreeKind::Direct {
                let same = self.children[..i].iter().position(|o| match self.kind {
                    TreeKind::OneD => o.position[0] == c.position[0],
                    _ => o.position == c.position,
                });
                if let Some(j) = same {
                    return Err(format!("children {j} and {i} are at the same position"));
                }
            }
            if let MotionDef::Tree(t) = &c.motion {
                t.validate(is_float)
                    .map_err(|e| format!("child {i}: {e}"))?;
            }
        }
        Ok(())
    }
}

/// The children's weights for `input` (a 1D tree reads `input[0]`), one
/// per position, into `out` (as long as `positions`); normalized, the
/// ones below [`MIN_WEIGHT`] zero. `kind` is not `Direct` (see
/// [`direct_weights`]).
pub fn weights(kind: TreeKind, positions: &[[f32; 2]], input: [f32; 2], out: &mut [f32]) {
    assert_eq!(positions.len(), out.len(), "one weight per position");
    out.fill(0.0);
    if positions.is_empty() {
        return;
    }
    if positions.len() == 1 {
        out[0] = 1.0;
        return;
    }
    match kind {
        TreeKind::OneD => one_d(positions, input[0], out),
        TreeKind::SimpleDirectional => {
            if !simple_directional(positions, input, out) {
                gradient_band(positions, input, out);
            }
        }
        TreeKind::FreeformCartesian => gradient_band(positions, input, out),
        TreeKind::FreeformDirectional => polar_gradient_band(positions, input, out),
        TreeKind::Direct => {
            // Callers use `direct_weights`; equal shares keep this total.
            out.fill(1.0);
        }
    }
    finish(positions, input, out);
}

/// A direct tree's weights: each child's parameter value (negative
/// counts as 0), normalized, into `out` (as long as `values`); all zero
/// gives the first child.
pub fn direct_weights(values: &[f32], out: &mut [f32]) {
    assert_eq!(values.len(), out.len(), "one weight per value");
    for (w, v) in out.iter_mut().zip(values) {
        *w = if v.is_finite() { v.max(0.0) } else { 0.0 };
    }
    let sum: f32 = out.iter().sum();
    if sum <= 0.0 {
        if let Some(first) = out.first_mut() {
            *first = 1.0;
        }
        return;
    }
    for w in out.iter_mut() {
        *w /= sum;
    }
    drop_small(out);
}

fn one_d(positions: &[[f32; 2]], x: f32, out: &mut [f32]) {
    // The nearest child on each side.
    let mut below: Option<usize> = None;
    let mut above: Option<usize> = None;
    for (i, p) in positions.iter().enumerate() {
        let t = p[0];
        if t <= x && below.is_none_or(|b| t > positions[b][0]) {
            below = Some(i);
        }
        if t >= x && above.is_none_or(|a| t < positions[a][0]) {
            above = Some(i);
        }
    }
    match (below, above) {
        (Some(b), Some(a)) if a != b => {
            let (tb, ta) = (positions[b][0], positions[a][0]);
            let f = (x - tb) / (ta - tb);
            out[b] = 1.0 - f;
            out[a] = f;
        }
        (Some(i), _) | (None, Some(i)) => out[i] = 1.0,
        (None, None) => out[0] = 1.0,
    }
}

const EPS: f32 = 1e-6;

fn len(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// The signed angle from `a` to `b`, radians in `(-π, π]`.
fn signed_angle(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] * b[1] - a[1] * b[0]).atan2(dot(a, b))
}

/// Unity's Simple Directional: the two directions around the input and
/// the centre child. False when the layout does not allow it (no two
/// directions within 180° of each other around the input): the caller
/// falls back to the cartesian gradient band.
fn simple_directional(positions: &[[f32; 2]], p: [f32; 2], out: &mut [f32]) -> bool {
    let center = positions.iter().position(|q| len(*q) < EPS);
    if len(p) < EPS {
        match center {
            Some(c) => {
                out[c] = 1.0;
                return true;
            }
            None => return false,
        }
    }
    let angle = p[1].atan2(p[0]);
    // The nearest direction counter-clockwise (b) and clockwise (a).
    let mut a: Option<(usize, f32)> = None;
    let mut b: Option<(usize, f32)> = None;
    for (i, q) in positions.iter().enumerate() {
        if Some(i) == center {
            continue;
        }
        let d = (q[1].atan2(q[0]) - angle).rem_euclid(std::f32::consts::TAU);
        // d = 0 is exactly on this direction: it is `a` (clockwise, 0).
        if a.is_none_or(|(_, best)| (std::f32::consts::TAU - d) % std::f32::consts::TAU < best) {
            a = Some((i, (std::f32::consts::TAU - d) % std::f32::consts::TAU));
        }
        if d > 0.0 && b.is_none_or(|(_, best)| d < best) {
            b = Some((i, d));
        }
    }
    let Some((ia, da)) = a else {
        return false;
    };
    let qa = positions[ia];
    let (alpha, beta, ib) = match b {
        Some((ib, db)) if ib != ia && da + db < std::f32::consts::PI - EPS => {
            let qb = positions[ib];
            let det = qa[0] * qb[1] - qa[1] * qb[0];
            if det.abs() < EPS {
                return false;
            }
            let alpha = (p[0] * qb[1] - p[1] * qb[0]) / det;
            let beta = (qa[0] * p[1] - qa[1] * p[0]) / det;
            (alpha.max(0.0), beta.max(0.0), Some(ib))
        }
        _ if da < EPS => {
            // On one direction's line, no neighbour within 180°.
            (dot(p, qa) / dot(qa, qa), 0.0, None)
        }
        _ => return false,
    };
    let sum = alpha + beta;
    let (scale, rest) = match center {
        Some(c) if sum < 1.0 => (1.0, Some(c)),
        _ => (1.0 / sum, None),
    };
    out[ia] = alpha * scale;
    if let Some(ib) = ib {
        out[ib] = beta * scale;
    }
    if let Some(c) = rest {
        out[c] = 1.0 - sum;
    }
    true
}

/// Cartesian gradient-band interpolation: each child's influence is the
/// smallest, over the other children, of how far the input is from it
/// towards that other child (1 at the child, 0 at the other).
fn gradient_band(positions: &[[f32; 2]], p: [f32; 2], out: &mut [f32]) {
    for (i, pi) in positions.iter().enumerate() {
        let mut h = f32::INFINITY;
        for (j, pj) in positions.iter().enumerate() {
            if i == j {
                continue;
            }
            let ij = sub(*pj, *pi);
            let ip = sub(p, *pi);
            let lsq = dot(ij, ij);
            if lsq < EPS * EPS {
                continue;
            }
            h = h.min(1.0 - dot(ip, ij) / lsq);
        }
        out[i] = if h.is_finite() { h.max(0.0) } else { 1.0 };
    }
}

/// How much an angle weighs against a relative change of magnitude in
/// the polar gradient band (Johansen's constant).
const DIRECTION_SCALE: f32 = 2.0;

/// Polar gradient-band interpolation: as [`gradient_band`], in a space
/// of (relative magnitude, angle), so a walk and a run in one direction
/// blend by speed and neighbouring directions by angle. A child at the
/// centre has no direction: it takes the other point's.
fn polar_gradient_band(positions: &[[f32; 2]], p: [f32; 2], out: &mut [f32]) {
    let pm = len(p);
    for (i, pi) in positions.iter().enumerate() {
        let im = len(*pi);
        let mut h = f32::INFINITY;
        for (j, pj) in positions.iter().enumerate() {
            if i == j {
                continue;
            }
            let jm = len(*pj);
            let avg = (im + jm) * 0.5;
            if avg < EPS {
                continue;
            }
            let (mut angle_ij, mut angle_ip) = (signed_angle(*pi, *pj), signed_angle(*pi, p));
            if im < EPS {
                // The centre: its direction is the input's.
                angle_ip = 0.0;
                angle_ij = if pm < EPS { 0.0 } else { signed_angle(p, *pj) };
            } else if jm < EPS {
                angle_ij = 0.0;
            }
            if pm < EPS {
                angle_ip = 0.0;
            }
            let ij = [(jm - im) / avg, angle_ij * DIRECTION_SCALE];
            let ip = [(pm - im) / avg, angle_ip * DIRECTION_SCALE];
            let lsq = dot(ij, ij);
            if lsq < EPS * EPS {
                continue;
            }
            h = h.min(1.0 - dot(ip, ij) / lsq);
        }
        out[i] = if h.is_finite() { h.max(0.0) } else { 1.0 };
    }
}

/// Normalizes; all zero (the input far outside) gives the nearest child.
fn finish(positions: &[[f32; 2]], p: [f32; 2], out: &mut [f32]) {
    let sum: f32 = out.iter().sum();
    if sum <= 0.0 || !sum.is_finite() {
        out.fill(0.0);
        let nearest = (0..positions.len())
            .min_by(|&a, &b| len(sub(positions[a], p)).total_cmp(&len(sub(positions[b], p))))
            .unwrap_or(0);
        out[nearest] = 1.0;
        return;
    }
    for w in out.iter_mut() {
        *w /= sum;
    }
    drop_small(out);
}

fn drop_small(out: &mut [f32]) {
    let mut sum = 0.0;
    for w in out.iter_mut() {
        if *w < MIN_WEIGHT {
            *w = 0.0;
        }
        sum += *w;
    }
    if sum > 0.0 && sum != 1.0 {
        for w in out.iter_mut() {
            *w /= sum;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(kind: TreeKind, positions: &[[f32; 2]], p: [f32; 2]) -> Vec<f32> {
        let mut out = vec![0.0; positions.len()];
        weights(kind, positions, p, &mut out);
        out
    }

    fn close(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-4, "{a:?} vs {b:?}");
        }
    }

    fn sums_to_one(ws: &[f32]) {
        let s: f32 = ws.iter().sum();
        assert!((s - 1.0).abs() < 1e-5, "{ws:?} sums to {s}");
        assert!(ws.iter().all(|w| *w >= 0.0), "{ws:?}");
    }

    /// Idle at the centre, a walk at 2 m/s in each of the four
    /// directions (the ADR's strafe tree).
    const FOUR: [[f32; 2]; 5] = [[0.0, 0.0], [0.0, 2.0], [0.0, -2.0], [-2.0, 0.0], [2.0, 0.0]];

    #[test]
    fn one_d_is_linear_between_neighbours_and_clamped_in_any_order() {
        // Out of order on purpose: the tree sorts nothing, the weights do.
        let t = [[4.0, 0.0], [0.0, 0.0], [7.0, 0.0]];
        close(&w(TreeKind::OneD, &t, [2.0, 0.0]), &[0.5, 0.5, 0.0]);
        close(&w(TreeKind::OneD, &t, [5.5, 0.0]), &[0.5, 0.0, 0.5]);
        close(&w(TreeKind::OneD, &t, [4.0, 0.0]), &[1.0, 0.0, 0.0]);
        close(&w(TreeKind::OneD, &t, [-3.0, 0.0]), &[0.0, 1.0, 0.0]);
        close(&w(TreeKind::OneD, &t, [9.0, 0.0]), &[0.0, 0.0, 1.0]);
        // 0.0005 of the way: the small weight is dropped.
        close(&w(TreeKind::OneD, &t, [4.0015, 0.0]), &[1.0, 0.0, 0.0]);
    }

    #[test]
    fn every_2d_type_gives_a_child_all_the_weight_at_its_position() {
        for kind in [
            TreeKind::SimpleDirectional,
            TreeKind::FreeformDirectional,
            TreeKind::FreeformCartesian,
        ] {
            for (i, p) in FOUR.iter().enumerate() {
                let ws = w(kind, &FOUR, *p);
                let mut want = [0.0; 5];
                want[i] = 1.0;
                close(&ws, &want);
            }
        }
    }

    #[test]
    fn every_2d_type_sums_to_one_everywhere() {
        for kind in [
            TreeKind::SimpleDirectional,
            TreeKind::FreeformDirectional,
            TreeKind::FreeformCartesian,
        ] {
            for x in -6..=6 {
                for y in -6..=6 {
                    let ws = w(kind, &FOUR, [x as f32 * 0.5, y as f32 * 0.5]);
                    sums_to_one(&ws);
                }
            }
        }
    }

    #[test]
    fn simple_directional_is_barycentric_in_the_wedge() {
        // Halfway to forward: half idle, half forward.
        close(
            &w(TreeKind::SimpleDirectional, &FOUR, [0.0, 1.0]),
            &[0.5, 0.5, 0.0, 0.0, 0.0],
        );
        // (0.5, 0.5): a quarter each of forward and right, half idle.
        close(
            &w(TreeKind::SimpleDirectional, &FOUR, [0.5, 0.5]),
            &[0.5, 0.25, 0.0, 0.0, 0.25],
        );
        // (1, 1) is on the edge between forward and right: no idle.
        close(
            &w(TreeKind::SimpleDirectional, &FOUR, [1.0, 1.0]),
            &[0.0, 0.5, 0.0, 0.0, 0.5],
        );
        // Past the edge (2, 2): forward and right only, evenly.
        close(
            &w(TreeKind::SimpleDirectional, &FOUR, [2.0, 2.0]),
            &[0.0, 0.5, 0.0, 0.0, 0.5],
        );
        // Far out along forward: forward alone.
        close(
            &w(TreeKind::SimpleDirectional, &FOUR, [0.0, 9.0]),
            &[0.0, 1.0, 0.0, 0.0, 0.0],
        );
    }

    #[test]
    fn cartesian_gradient_band_matches_hand_worked_values() {
        // Two children: plain linear along the line between them.
        let two = [[0.0, 0.0], [2.0, 0.0]];
        close(
            &w(TreeKind::FreeformCartesian, &two, [0.5, 0.0]),
            &[0.75, 0.25],
        );
        // Off the line, only the projection counts.
        close(
            &w(TreeKind::FreeformCartesian, &two, [0.5, 3.0]),
            &[0.75, 0.25],
        );
        // FOUR at (1, 0). h = min over the others of 1 - (ip·ij)/|ij|².
        // Idle: forward 1, back 1, left 1.5, right 1 - 2/4 = 0.5 → 0.5.
        // Right: idle 1 - 2/4 = 0.5, forward and back 1 - 2/8 = 0.75,
        // left 1 - 4/16 = 0.75 → 0.5. Forward: idle 1 - 4/4 = 0 → 0;
        // back likewise; left: idle 1 - 6/4 < 0 → 0.
        close(
            &w(TreeKind::FreeformCartesian, &FOUR, [1.0, 0.0]),
            &[0.5, 0.0, 0.0, 0.0, 0.5],
        );
    }

    #[test]
    fn polar_gradient_band_blends_speed_along_a_direction_and_angle_between() {
        // Walk (2) and run (4) forward, idle at the centre.
        let t = [[0.0, 0.0], [0.0, 2.0], [0.0, 4.0], [2.0, 0.0], [4.0, 0.0]];
        // Forward at 3 m/s: walk and run evenly, nothing sideways.
        close(
            &w(TreeKind::FreeformDirectional, &t, [0.0, 3.0]),
            &[0.0, 0.5, 0.5, 0.0, 0.0],
        );
        // Diagonal at the walk's speed: forward walk and right walk
        // share most of it evenly, a little idle, no run. By hand: the
        // walks' h is 0.5 each (half the angle to the other walk), the
        // idle's 1 - 4/(4 + π²/4) ≈ 0.381, the runs' 0.
        let d = 2.0 / std::f32::consts::SQRT_2;
        let ws = w(TreeKind::FreeformDirectional, &t, [d, d]);
        let sum = 1.0 + (1.0 - 4.0 / (4.0 + std::f32::consts::PI.powi(2) / 4.0));
        close(&ws, &[(sum - 1.0) / sum, 0.5 / sum, 0.0, 0.5 / sum, 0.0]);
        // At 1 m/s forward: idle and walk evenly (half the magnitude).
        close(
            &w(TreeKind::FreeformDirectional, &t, [0.0, 1.0]),
            &[0.5, 0.5, 0.0, 0.0, 0.0],
        );
    }

    #[test]
    fn direct_weights_are_the_parameters_normalized() {
        let mut out = [0.0; 3];
        direct_weights(&[1.0, 3.0, -2.0], &mut out);
        close(&out, &[0.25, 0.75, 0.0]);
        let mut out = [0.0; 2];
        direct_weights(&[0.0, 0.0], &mut out);
        close(&out, &[1.0, 0.0]);
    }

    #[test]
    fn validate_holds_the_shape() {
        let child = |x: f32, y: f32| ChildDef {
            motion: MotionDef::Asset(AssetRef::default()),
            position: [x, y],
            time_scale: 1.0,
            parameter: None,
        };
        let float = |i: usize| i < 2;
        let mut t = TreeDef {
            kind: TreeKind::FreeformDirectional,
            parameters: vec![0, 1],
            children: vec![child(0.0, 0.0), child(0.0, 2.0)],
        };
        assert_eq!(t.validate(&float), Ok(()));
        t.children.push(child(0.0, 2.0));
        assert!(t.validate(&float).unwrap_err().contains("same position"));
        t.children.pop();
        t.parameters = vec![0];
        assert!(t.validate(&float).unwrap_err().contains("2 parameter"));
        t.parameters = vec![0, 5];
        assert!(t.validate(&float).unwrap_err().contains("floats"));
        t.parameters = vec![0, 1];
        let mut deep = t.clone();
        for _ in 0..TreeDef::MAX_DEPTH {
            let inner = deep.clone();
            deep.children[0].motion = MotionDef::Tree(Box::new(inner));
        }
        assert!(deep.validate(&float).unwrap_err().contains("nest"));
        assert_eq!(t.leaf_count(), 2);
    }
}
