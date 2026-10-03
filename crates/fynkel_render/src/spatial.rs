//! Persistent sphere BVH. Refits preserve topology between membership changes;
//! a doubled hierarchy cost triggers a spatial rebuild after large movement.
use crate::culling::{Frustum, Sphere};

#[derive(Clone, Copy)]
struct Node {
    bound: Sphere,
    children: Option<(usize, usize)>,
    item: usize,
    parent: Option<usize>,
    range: (usize, usize),
}
#[derive(Default)]
pub(crate) struct SpatialIndex {
    bounds: Vec<Sphere>,
    order: Vec<usize>,
    nodes: Vec<Node>,
    stack: Vec<usize>,
    build_cost: f64,
    current_cost: f64,
    leaves: Vec<usize>,
    changed: Vec<usize>,
}
impl SpatialIndex {
    pub fn update(&mut self, bounds: &[Sphere]) {
        if self.bounds.len() != bounds.len() || self.nodes.is_empty() {
            self.bounds.clear();
            self.bounds.extend_from_slice(bounds);
            self.rebuild();
            return;
        }
        self.changed.clear();
        self.changed.extend(
            self.bounds
                .iter()
                .zip(bounds)
                .enumerate()
                .filter_map(|(i, (a, b))| (a != b).then_some(i)),
        );
        if self.changed.is_empty() {
            return;
        }
        self.bounds.copy_from_slice(bounds);
        if self.changed.len() > bounds.len() / 8 {
            for i in (0..self.nodes.len()).rev() {
                self.refit(i);
            }
            self.current_cost = self.cost();
        } else {
            for changed in 0..self.changed.len() {
                let mut next = Some(self.leaves[self.changed[changed]]);
                while let Some(i) = next {
                    let before = self.nodes[i].bound;
                    self.refit(i);
                    let after = self.nodes[i].bound;
                    self.current_cost += Self::bound_cost(after) - Self::bound_cost(before);
                    if before == after {
                        break;
                    }
                    next = self.nodes[i].parent;
                }
            }
        }
        if self.current_cost > self.build_cost * 2.0 {
            self.rebuild();
        }
    }
    fn bound_cost(bound: Sphere) -> f64 {
        if bound.is_finite() {
            f64::from(bound.radius).powi(2)
        } else {
            0.0
        }
    }
    fn refit(&mut self, i: usize) {
        let node = self.nodes[i];
        self.nodes[i].bound = match node.children {
            Some((a, b)) => self.nodes[a].bound.union(&self.nodes[b].bound),
            None => self.bounds[node.item],
        };
    }
    fn cost(&self) -> f64 {
        self.nodes
            .iter()
            .filter(|n| n.bound.is_finite())
            .map(|n| f64::from(n.bound.radius).powi(2))
            .sum()
    }
    fn rebuild(&mut self) {
        self.nodes.clear();
        self.order.clear();
        self.order.extend(0..self.bounds.len());
        self.leaves.resize(self.bounds.len(), 0);
        if !self.order.is_empty() {
            self.build(0, self.order.len());
        }
        self.current_cost = self.cost();
        self.build_cost = self.current_cost.max(f64::EPSILON);
    }
    fn build(&mut self, start: usize, end: usize) -> usize {
        let item = self.order[start];
        let at = self.nodes.len();
        self.nodes.push(Node {
            bound: self.bounds[item],
            children: None,
            item,
            parent: None,
            range: (start, end),
        });
        self.leaves[item] = at;
        if end - start > 1 {
            let mut lo = glam::Vec3::splat(f32::INFINITY);
            let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
            for &i in &self.order[start..end] {
                if self.bounds[i].center.is_finite() {
                    lo = lo.min(self.bounds[i].center);
                    hi = hi.max(self.bounds[i].center);
                }
            }
            let extent = hi - lo;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            let mid = (end - start) / 2;
            self.order[start..end].select_nth_unstable_by(mid, |a, b| {
                self.bounds[*a].center[axis]
                    .total_cmp(&self.bounds[*b].center[axis])
                    .then(a.cmp(b))
            });
            let a = self.build(start, start + mid);
            let b = self.build(start + mid, end);
            self.nodes[at].children = Some((a, b));
            self.nodes[a].parent = Some(at);
            self.nodes[b].parent = Some(at);
            self.nodes[at].bound = self.nodes[a].bound.union(&self.nodes[b].bound);
        }
        at
    }
    pub fn overlaps(&mut self, sphere: &Sphere, out: &mut Vec<usize>) {
        out.clear();
        self.stack.clear();
        if !self.nodes.is_empty() {
            self.stack.push(0);
        }
        while let Some(i) = self.stack.pop() {
            let node = self.nodes[i];
            if sphere.is_finite()
                && node.bound.is_finite()
                && sphere
                    .center
                    .as_dvec3()
                    .distance(node.bound.center.as_dvec3())
                    > f64::from(sphere.radius) + f64::from(node.bound.radius)
            {
                continue;
            }
            match node.children {
                Some((a, b)) => {
                    self.stack.push(a);
                    self.stack.push(b);
                }
                None => out.push(node.item),
            }
        }
        out.sort_unstable();
    }

    pub fn query(&mut self, frustum: &Frustum, out: &mut Vec<bool>) -> usize {
        out.clear();
        out.resize(self.bounds.len(), false);
        self.stack.clear();
        if !self.nodes.is_empty() {
            self.stack.push(0);
        }
        let mut tested = 0;
        while let Some(i) = self.stack.pop() {
            let node = self.nodes[i];
            tested += 1;
            if !frustum.intersects(&node.bound) {
                continue;
            }
            if frustum.contains(&node.bound) {
                for &item in &self.order[node.range.0..node.range.1] {
                    out[item] = true;
                }
                continue;
            }
            match node.children {
                Some((a, b)) => {
                    self.stack.push(a);
                    self.stack.push(b);
                }
                None => out[node.item] = true,
            }
        }
        tested
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_matches_linear_culling_after_motion_removal_and_invalid_bounds() {
        let f = Frustum::from_view_proj(&glam::Mat4::IDENTITY);
        let mut index = SpatialIndex::default();
        let mut visible = Vec::new();
        let mut bounds: Vec<_> = (0..2048)
            .map(|i| Sphere {
                center: glam::Vec3::new(i as f32 * 0.1 - 100.0, 0.0, 0.5),
                radius: 0.05,
            })
            .collect();
        for frame in 0..8 {
            if frame == 2 {
                bounds[9] = Sphere::UNBOUNDED;
            }
            if frame == 3 {
                bounds[9].center = glam::Vec3::NAN;
            }
            if frame == 4 {
                bounds.truncate(600);
            }
            if frame == 5 {
                for b in &mut bounds {
                    b.center.x += 100.0;
                }
            }
            index.update(&bounds);
            let tested = index.query(&f, &mut visible);
            assert_eq!(
                visible,
                bounds.iter().map(|b| f.intersects(b)).collect::<Vec<_>>()
            );
            if frame == 0 {
                assert!(
                    tested < bounds.len() / 4,
                    "spatial rejection must avoid linear plane tests"
                );
            }
        }
        index.update(&[]);
        index.query(&f, &mut visible);
        assert!(visible.is_empty());
    }
}
