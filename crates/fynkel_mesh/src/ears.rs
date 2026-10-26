//! Ear clipping of an outline with holes, as mapbox's earcut does it
//! (ISC licence, by Vladimir Agafonkin): each hole is joined to the
//! outline by a cut from its leftmost point to a corner it can see, the
//! holes taken from the left; ears are found among nearby points by a
//! z-order curve once there are more than 80; and an outline that touches
//! or crosses itself is cured and split rather than filled outside.
//!
//! The points live in one list, linked round each loop (and, for the
//! search, along the curve); a cut visits a point twice, as two nodes.

use std::cmp::Ordering;
use std::ops::Range;

use glam::Vec2;

/// No node: the end of the curve's list.
const NONE: usize = usize::MAX;

/// Above this many points, ears are looked for along the z-order curve.
const HASHED_ABOVE: usize = 80;

#[derive(Clone, Copy)]
struct Node {
    /// The point's index, as it goes into the triangles.
    i: u32,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
    /// Where on the z-order curve, and the neighbours along it.
    z: u32,
    prev_z: usize,
    next_z: usize,
    /// A hole of one point: kept even where it's in line.
    steiner: bool,
}

struct Ears {
    nodes: Vec<Node>,
    triangles: Vec<u32>,
    min_x: f64,
    min_y: f64,
    /// 32767 over the larger side of the bounds, or 0 for no curve.
    inv_size: f64,
}

/// Triangles filling the loop `rings[0]` of `points` less the loops after
/// it, three indices each, counter-clockwise with X right and Y up. The
/// loops may be wound either way.
pub(crate) fn triangulate(points: &[Vec2], rings: &[Range<usize>]) -> Vec<u32> {
    let mut e = Ears {
        nodes: Vec::with_capacity(points.len() + 2 * rings.len() + 8),
        triangles: Vec::with_capacity(3 * (points.len() + 2 * rings.len())),
        min_x: 0.0,
        min_y: 0.0,
        inv_size: 0.0,
    };
    let Some(first) = rings.first().cloned() else { return e.triangles };
    let Some(mut outer) = e.linked_list(points, first, true) else { return e.triangles };
    if e.nodes[outer].next == e.nodes[outer].prev {
        return e.triangles;
    }
    if rings.len() > 1 {
        outer = e.eliminate_holes(points, &rings[1..], outer);
    }
    if points.len() > HASHED_ABOVE {
        let (mut min, mut max) = (Vec2::MAX, Vec2::MIN);
        for &p in points {
            (min, max) = (min.min(p), max.max(p));
        }
        let size = f64::from((max - min).max_element());
        e.min_x = f64::from(min.x);
        e.min_y = f64::from(min.y);
        e.inv_size = if size > 0.0 { 32767.0 / size } else { 0.0 };
    }
    e.earcut_linked(outer, 0);
    e.triangles
}

impl Ears {
    fn prev(&self, n: usize) -> usize {
        self.nodes[n].prev
    }

    fn next(&self, n: usize) -> usize {
        self.nodes[n].next
    }

    fn equals(&self, a: usize, b: usize) -> bool {
        let (a, b) = (&self.nodes[a], &self.nodes[b]);
        a.x == b.x && a.y == b.y
    }

    /// The turn at q from p to r: less than 0 for a convex corner of a
    /// counter-clockwise loop.
    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        let (p, q, r) = (&self.nodes[p], &self.nodes[q], &self.nodes[r]);
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }

    /// A loop of nodes for `ring`, wound counter-clockwise for the outline
    /// (`outer`) and clockwise for a hole; None for no points.
    fn linked_list(&mut self, points: &[Vec2], ring: Range<usize>, outer: bool) -> Option<usize> {
        let mut sum = 0.0;
        let mut j = ring.end - 1;
        for i in ring.clone() {
            let (a, b) = (points[i], points[j]);
            sum += f64::from(b.x - a.x) * f64::from(a.y + b.y);
            j = i;
        }
        let mut last = NONE;
        if outer == (sum > 0.0) {
            for i in ring {
                last = self.insert(i, points[i], last);
            }
        } else {
            for i in ring.rev() {
                last = self.insert(i, points[i], last);
            }
        }
        if last == NONE {
            return None;
        }
        if self.equals(last, self.next(last)) {
            self.remove(last);
            last = self.next(last);
        }
        Some(last)
    }

    fn insert(&mut self, i: usize, p: Vec2, last: usize) -> usize {
        let n = self.nodes.len();
        self.nodes.push(Node {
            i: i as u32,
            x: f64::from(p.x),
            y: f64::from(p.y),
            prev: n,
            next: n,
            z: 0,
            prev_z: NONE,
            next_z: NONE,
            steiner: false,
        });
        if last != NONE {
            let after = self.next(last);
            self.nodes[n].next = after;
            self.nodes[n].prev = last;
            self.nodes[after].prev = n;
            self.nodes[last].next = n;
        }
        n
    }

    /// Takes `n` out of its loop and the curve; it keeps its own links, so
    /// a walk can go on from it.
    fn remove(&mut self, n: usize) {
        let Node { prev, next, prev_z, next_z, .. } = self.nodes[n];
        self.nodes[next].prev = prev;
        self.nodes[prev].next = next;
        if prev_z != NONE {
            self.nodes[prev_z].next_z = next_z;
        }
        if next_z != NONE {
            self.nodes[next_z].prev_z = prev_z;
        }
    }

    /// Takes out points the same as the next and corners in line, from
    /// `start` round to `end` (or `start`).
    fn filter_points(&mut self, start: usize, end: Option<usize>) -> usize {
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let mut again = false;
            let next = self.next(p);
            if !self.nodes[p].steiner && (self.equals(p, next) || self.area(self.prev(p), p, next) == 0.0) {
                self.remove(p);
                p = self.prev(p);
                end = p;
                if p == self.next(p) {
                    break;
                }
                again = true;
            } else {
                p = next;
            }
            if !again && p == end {
                break;
            }
        }
        end
    }

    /// Cuts ears off the loop at `ear`; where none is left, tidies the loop
    /// and tries again, then cures where it crosses itself, then splits it
    /// in two.
    fn earcut_linked(&mut self, mut ear: usize, pass: u8) {
        if pass == 0 && self.inv_size != 0.0 {
            self.index_curve(ear);
        }
        let mut stop = ear;
        while self.prev(ear) != self.next(ear) {
            let (prev, next) = (self.prev(ear), self.next(ear));
            let is_ear = if self.inv_size != 0.0 { self.is_ear_hashed(ear) } else { self.is_ear(ear) };
            if is_ear {
                self.triangles.extend([self.nodes[prev].i, self.nodes[ear].i, self.nodes[next].i]);
                self.remove(ear);
                ear = self.next(next);
                stop = ear;
                continue;
            }
            ear = next;
            if ear == stop {
                match pass {
                    0 => {
                        let ear = self.filter_points(ear, None);
                        self.earcut_linked(ear, 1);
                    }
                    1 => {
                        let ear = self.filter_points(ear, None);
                        let ear = self.cure_local_intersections(ear);
                        self.earcut_linked(ear, 2);
                    }
                    _ => self.split_earcut(ear),
                }
                break;
            }
        }
    }

    /// The bounds of the triangle a, b, c.
    fn bounds(&self, a: usize, b: usize, c: usize) -> (f64, f64, f64, f64) {
        let (a, b, c) = (&self.nodes[a], &self.nodes[b], &self.nodes[c]);
        (a.x.min(b.x).min(c.x), a.y.min(b.y).min(c.y), a.x.max(b.x).max(c.x), a.y.max(b.y).max(c.y))
    }

    /// Whether `p` blocks the ear a, b, c: inside it (other than at a), and
    /// a reflex corner or in line.
    fn blocks(&self, p: usize, a: usize, b: usize, c: usize, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
        let q = &self.nodes[p];
        let (na, nb, nc) = (&self.nodes[a], &self.nodes[b], &self.nodes[c]);
        q.x >= x0
            && q.x <= x1
            && q.y >= y0
            && q.y <= y1
            && !(na.x == q.x && na.y == q.y)
            && point_in_triangle((na.x, na.y), (nb.x, nb.y), (nc.x, nc.y), (q.x, q.y))
            && self.area(self.prev(p), p, self.next(p)) >= 0.0
    }

    /// Whether the corner at `ear` can be cut off: convex, with no other
    /// point of the loop in it.
    fn is_ear(&self, ear: usize) -> bool {
        let (a, b, c) = (self.prev(ear), ear, self.next(ear));
        if self.area(a, b, c) >= 0.0 {
            return false;
        }
        let bounds = self.bounds(a, b, c);
        let mut p = self.next(c);
        while p != a {
            if self.blocks(p, a, b, c, bounds) {
                return false;
            }
            p = self.next(p);
        }
        true
    }

    /// As `is_ear`, looking only at points on the curve within the ear's
    /// bounds.
    fn is_ear_hashed(&self, ear: usize) -> bool {
        let (a, b, c) = (self.prev(ear), ear, self.next(ear));
        if self.area(a, b, c) >= 0.0 {
            return false;
        }
        let bounds = self.bounds(a, b, c);
        let (x0, y0, x1, y1) = bounds;
        let (min_z, max_z) = (self.z_order(x0, y0), self.z_order(x1, y1));
        let (mut p, mut n) = (self.nodes[ear].prev_z, self.nodes[ear].next_z);
        let blocks = |p: usize| p != a && p != c && self.blocks(p, a, b, c, bounds);
        // Both ways at once, then whichever is left.
        while p != NONE && self.nodes[p].z >= min_z && n != NONE && self.nodes[n].z <= max_z {
            if blocks(p) || blocks(n) {
                return false;
            }
            p = self.nodes[p].prev_z;
            n = self.nodes[n].next_z;
        }
        while p != NONE && self.nodes[p].z >= min_z {
            if blocks(p) {
                return false;
            }
            p = self.nodes[p].prev_z;
        }
        while n != NONE && self.nodes[n].z <= max_z {
            if blocks(n) {
                return false;
            }
            n = self.nodes[n].next_z;
        }
        true
    }

    /// Where the loop crosses itself just past a corner (a, p, p.next, b
    /// with a b crossing p p.next), cuts the triangle off there.
    fn cure_local_intersections(&mut self, mut start: usize) -> usize {
        let mut p = start;
        loop {
            let (a, b) = (self.prev(p), self.next(self.next(p)));
            if !self.equals(a, b)
                && self.intersects(a, p, self.next(p), b)
                && self.locally_inside(a, b)
                && self.locally_inside(b, a)
            {
                self.triangles.extend([self.nodes[a].i, self.nodes[p].i, self.nodes[b].i]);
                self.remove(p);
                self.remove(self.next(p));
                p = b;
                start = b;
            }
            p = self.next(p);
            if p == start {
                break;
            }
        }
        self.filter_points(p, None)
    }

    /// Splits the loop in two along a diagonal inside it, and fills each.
    fn split_earcut(&mut self, start: usize) {
        let mut a = start;
        loop {
            let mut b = self.next(self.next(a));
            while b != self.prev(a) {
                if self.nodes[a].i != self.nodes[b].i && self.is_valid_diagonal(a, b) {
                    let c = self.split_polygon(a, b);
                    let a = self.filter_points(a, Some(self.next(a)));
                    let c = self.filter_points(c, Some(self.next(c)));
                    self.earcut_linked(a, 0);
                    self.earcut_linked(c, 0);
                    return;
                }
                b = self.next(b);
            }
            a = self.next(a);
            if a == start {
                return;
            }
        }
    }

    /// Joins each hole into the outline, from the leftmost hole.
    fn eliminate_holes(&mut self, points: &[Vec2], holes: &[Range<usize>], mut outer: usize) -> usize {
        let mut queue = Vec::with_capacity(holes.len());
        for ring in holes {
            let Some(list) = self.linked_list(points, ring.clone(), false) else { continue };
            if list == self.next(list) {
                self.nodes[list].steiner = true;
            }
            queue.push(self.leftmost(list));
        }
        queue.sort_by(|&a, &b| self.compare_x_y_slope(a, b));
        for hole in queue {
            outer = self.eliminate_hole(hole, outer);
        }
        outer
    }

    /// Left first, then down, then by the slope of the edge after.
    fn compare_x_y_slope(&self, a: usize, b: usize) -> Ordering {
        let (na, nb) = (&self.nodes[a], &self.nodes[b]);
        na.x.total_cmp(&nb.x).then(na.y.total_cmp(&nb.y)).then_with(|| {
            let slope = |n: &Node| {
                let next = &self.nodes[n.next];
                (next.y - n.y) / (next.x - n.x)
            };
            slope(na).total_cmp(&slope(nb))
        })
    }

    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let Some(bridge) = self.find_hole_bridge(hole, outer) else { return outer };
        let reverse = self.split_polygon(bridge, hole);
        self.filter_points(reverse, Some(self.next(reverse)));
        self.filter_points(bridge, Some(self.next(bridge)))
    }

    /// A corner of the outline the hole's leftmost point can see, to its
    /// left: the nearer end of the first edge a ray that way meets, or a
    /// corner in front of it at the smallest angle to the ray.
    fn find_hole_bridge(&self, hole: usize, outer: usize) -> Option<usize> {
        let (hx, hy) = (self.nodes[hole].x, self.nodes[hole].y);
        let mut qx = f64::NEG_INFINITY;
        let mut m = NONE;
        let mut p = outer;
        if self.equals(hole, p) {
            return Some(p);
        }
        loop {
            let next = self.next(p);
            if self.equals(hole, next) {
                return Some(next);
            }
            let (np, nn) = (&self.nodes[p], &self.nodes[next]);
            if hy <= np.y && hy >= nn.y && nn.y != np.y {
                let x = np.x + (hy - np.y) * (nn.x - np.x) / (nn.y - np.y);
                if x <= hx && x > qx {
                    qx = x;
                    m = if np.x < nn.x { p } else { next };
                    if x == hx {
                        // The hole touches the edge: its left end.
                        return Some(m);
                    }
                }
            }
            p = next;
            if p == outer {
                break;
            }
        }
        if m == NONE {
            return None;
        }
        let stop = m;
        let (mx, my) = (self.nodes[m].x, self.nodes[m].y);
        let mut tan_min = f64::INFINITY;
        p = m;
        loop {
            let (px, py) = (self.nodes[p].x, self.nodes[p].y);
            if hx >= px
                && px >= mx
                && hx != px
                && point_in_triangle(
                    (if hy < my { hx } else { qx }, hy),
                    (mx, my),
                    (if hy < my { qx } else { hx }, hy),
                    (px, py),
                )
            {
                let tan = (hy - py).abs() / (hx - px);
                let mx_now = self.nodes[m].x;
                if self.locally_inside(p, hole)
                    && (tan < tan_min
                        || (tan == tan_min && (px > mx_now || (px == mx_now && self.sector_contains_sector(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = self.next(p);
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    /// Whether the corner at `m` holds the corner at `p` (two visits of
    /// one point, where a cut was made before).
    fn sector_contains_sector(&self, m: usize, p: usize) -> bool {
        self.area(self.prev(m), m, self.prev(p)) < 0.0 && self.area(self.next(p), m, self.next(m)) < 0.0
    }

    fn leftmost(&self, start: usize) -> usize {
        let (mut p, mut left) = (start, start);
        loop {
            let (np, nl) = (&self.nodes[p], &self.nodes[left]);
            if np.x < nl.x || (np.x == nl.x && np.y < nl.y) {
                left = p;
            }
            p = self.next(p);
            if p == start {
                return left;
            }
        }
    }

    /// Whether a diagonal a b lies inside the loop and crosses none of it.
    fn is_valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (na, nb) = (&self.nodes[a], &self.nodes[b]);
        self.nodes[na.next].i != nb.i
            && self.nodes[na.prev].i != nb.i
            && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b)
                && self.locally_inside(b, a)
                && self.middle_inside(a, b)
                // It doesn't make sectors facing opposite ways.
                && (self.area(na.prev, a, nb.prev) != 0.0 || self.area(a, nb.prev, b) != 0.0))
                // Or it's of no length, between two convex corners.
                || (self.equals(a, b)
                    && self.area(na.prev, a, na.next) > 0.0
                    && self.area(nb.prev, b, nb.next) > 0.0))
    }

    /// Whether segments p1 q1 and p2 q2 meet.
    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        let sign = |v: f64| {
            if v > 0.0 {
                1
            } else if v < 0.0 {
                -1
            } else {
                0
            }
        };
        let (o1, o2) = (sign(self.area(p1, q1, p2)), sign(self.area(p1, q1, q2)));
        let (o3, o4) = (sign(self.area(p2, q2, p1)), sign(self.area(p2, q2, q1)));
        (o1 != o2 && o3 != o4)
            || (o1 == 0 && self.on_segment(p1, p2, q1))
            || (o2 == 0 && self.on_segment(p1, q2, q1))
            || (o3 == 0 && self.on_segment(p2, p1, q2))
            || (o4 == 0 && self.on_segment(p2, q1, q2))
    }

    /// Whether q, in line with p and r, is between them.
    fn on_segment(&self, p: usize, q: usize, r: usize) -> bool {
        let (p, q, r) = (&self.nodes[p], &self.nodes[q], &self.nodes[r]);
        q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
    }

    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let (ia, ib) = (self.nodes[a].i, self.nodes[b].i);
        let mut p = a;
        loop {
            let next = self.next(p);
            let (ip, inext) = (self.nodes[p].i, self.nodes[next].i);
            if ip != ia && inext != ia && ip != ib && inext != ib && self.intersects(p, next, a, b) {
                return true;
            }
            p = next;
            if p == a {
                return false;
            }
        }
    }

    /// Whether the diagonal a b starts into the loop at a.
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (prev, next) = (self.prev(a), self.next(a));
        if self.area(prev, a, next) < 0.0 {
            self.area(a, b, next) >= 0.0 && self.area(a, prev, b) >= 0.0
        } else {
            self.area(a, b, prev) < 0.0 || self.area(a, next, b) < 0.0
        }
    }

    /// Whether the middle of a b is inside the loop.
    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let (px, py) = ((self.nodes[a].x + self.nodes[b].x) / 2.0, (self.nodes[a].y + self.nodes[b].y) / 2.0);
        let mut inside = false;
        let mut p = a;
        loop {
            let (np, nn) = (&self.nodes[p], &self.nodes[self.next(p)]);
            if (np.y > py) != (nn.y > py) && nn.y != np.y && px < (nn.x - np.x) * (py - np.y) / (nn.y - np.y) + np.x {
                inside = !inside;
            }
            p = np.next;
            if p == a {
                return inside;
            }
        }
    }

    /// Cuts the loop along a b into two: a to b and on, and a copy of b
    /// back to a copy of a. Gives the copy of b.
    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let (a2, b2) = (self.nodes.len(), self.nodes.len() + 1);
        let fresh = |n: &Node| Node { prev_z: NONE, next_z: NONE, z: 0, steiner: false, ..*n };
        let (na, nb) = (fresh(&self.nodes[a]), fresh(&self.nodes[b]));
        self.nodes.extend([na, nb]);
        let (an, bp) = (self.next(a), self.prev(b));
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        self.nodes[a2].next = an;
        self.nodes[an].prev = a2;
        self.nodes[b2].next = a2;
        self.nodes[a2].prev = b2;
        self.nodes[bp].next = b2;
        self.nodes[b2].prev = bp;
        b2
    }

    /// Where (x, y) is on the z-order curve over the bounds.
    fn z_order(&self, x: f64, y: f64) -> u32 {
        let spread = |v: f64| {
            let mut v = v as u32;
            v = (v | (v << 8)) & 0x00FF_00FF;
            v = (v | (v << 4)) & 0x0F0F_0F0F;
            v = (v | (v << 2)) & 0x3333_3333;
            (v | (v << 1)) & 0x5555_5555
        };
        spread((x - self.min_x) * self.inv_size) | (spread((y - self.min_y) * self.inv_size) << 1)
    }

    /// Puts the loop at `start` on the curve, sorted along it.
    fn index_curve(&mut self, start: usize) {
        let mut p = start;
        loop {
            if self.nodes[p].z == 0 {
                self.nodes[p].z = self.z_order(self.nodes[p].x, self.nodes[p].y);
            }
            self.nodes[p].prev_z = self.prev(p);
            self.nodes[p].next_z = self.next(p);
            p = self.next(p);
            if p == start {
                break;
            }
        }
        let last = self.nodes[p].prev_z;
        self.nodes[last].next_z = NONE;
        self.nodes[p].prev_z = NONE;
        self.sort_linked(p);
    }

    /// Sorts the curve's list from `list` by z: Simon Tatham's merge sort
    /// of a linked list.
    fn sort_linked(&mut self, mut list: usize) {
        let mut in_size = 1;
        loop {
            let mut p = list;
            list = NONE;
            let mut tail = NONE;
            let mut merges = 0;
            while p != NONE {
                merges += 1;
                let mut q = p;
                let mut p_size = 0;
                for _ in 0..in_size {
                    p_size += 1;
                    q = self.nodes[q].next_z;
                    if q == NONE {
                        break;
                    }
                }
                let mut q_size = in_size;
                while p_size > 0 || (q_size > 0 && q != NONE) {
                    let e;
                    if p_size != 0 && (q_size == 0 || q == NONE || self.nodes[p].z <= self.nodes[q].z) {
                        e = p;
                        p = self.nodes[p].next_z;
                        p_size -= 1;
                    } else {
                        e = q;
                        q = self.nodes[q].next_z;
                        q_size -= 1;
                    }
                    if tail != NONE {
                        self.nodes[tail].next_z = e;
                    } else {
                        list = e;
                    }
                    self.nodes[e].prev_z = tail;
                    tail = e;
                }
                p = q;
            }
            self.nodes[tail].next_z = NONE;
            in_size *= 2;
            if merges <= 1 {
                return;
            }
        }
    }
}

/// Whether p is inside the triangle a, b, c (wound as earcut's loops are),
/// or on its edges.
fn point_in_triangle(a: (f64, f64), b: (f64, f64), c: (f64, f64), p: (f64, f64)) -> bool {
    (c.0 - p.0) * (a.1 - p.1) >= (a.0 - p.0) * (c.1 - p.1)
        && (a.0 - p.0) * (b.1 - p.1) >= (b.0 - p.0) * (a.1 - p.1)
        && (b.0 - p.0) * (c.1 - p.1) >= (c.0 - p.0) * (b.1 - p.1)
}
