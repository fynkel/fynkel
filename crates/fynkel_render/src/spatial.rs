//! Persistent sphere BVH. Small membership changes repair leaves in place;
//! a doubled hierarchy cost triggers a spatial rebuild after large movement.
use crate::culling::{Frustum, Sphere};

/// Work actually performed by one index update; used by frame diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum UpdateKind {
    Unchanged = 0,
    MembershipRebuild = 1,
    SparseRefit = 2,
    DenseRefit = 3,
    CostRebuild = 4,
    MembershipRepair = 5,
    DepthRebuild = 6,
}

#[derive(Clone, Copy)]
struct Node {
    bound: Sphere,
    children: Option<(usize, usize)>,
    item: usize,
    parent: Option<usize>,
    range: (usize, usize),
    height: usize,
}
#[derive(Default)]
pub(crate) struct SpatialIndex {
    bounds: Vec<Sphere>,
    order: Vec<usize>,
    nodes: Vec<Node>,
    stack: Vec<usize>,
    build_cost: f64,
    built_len: usize,
    current_cost: f64,
    leaves: Vec<usize>,
    changed: Vec<usize>,
    root: Option<usize>,
    free: Vec<usize>,
    postorder: Vec<usize>,
}
impl SpatialIndex {
    pub fn update(&mut self, bounds: &[Sphere]) -> UpdateKind {
        let old_len = self.bounds.len();
        if old_len != bounds.len() {
            self.bounds.clear();
            self.bounds.extend_from_slice(bounds);
            // Rebuild large changes and cumulative size drift so the last
            // spatial build remains a meaningful reference for the cost guard.
            if self.root.is_none()
                || bounds.is_empty()
                || old_len.abs_diff(bounds.len()) > old_len / 8
                || self.built_len.abs_diff(bounds.len()) > self.built_len / 8
            {
                self.rebuild();
                return UpdateKind::MembershipRebuild;
            }
            for item in (bounds.len()..old_len).rev() {
                self.remove_leaf(self.leaves[item]);
            }
            self.leaves.resize(bounds.len(), 0);
            // Record indices may all have shifted. Refresh existing leaves
            // before choosing siblings for additions; never query stale bounds.
            self.refresh_topology(true);
            for item in old_len..bounds.len() {
                self.insert_leaf(item);
                let max_height = 2 * bounds.len().ilog2() as usize + 4;
                if self.nodes[self.root.unwrap()].height > max_height {
                    self.rebuild();
                    return UpdateKind::DepthRebuild;
                }
            }
            if bounds.len() > old_len {
                self.refresh_topology(false);
            }
            self.current_cost = self.cost();
            if self.current_cost > self.build_cost * 2.0 {
                self.rebuild();
                return UpdateKind::CostRebuild;
            }
            return UpdateKind::MembershipRepair;
        }
        if self.root.is_none() {
            return UpdateKind::Unchanged;
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
            return UpdateKind::Unchanged;
        }
        self.bounds.copy_from_slice(bounds);
        let dense = self.changed.len() > bounds.len() / 8;
        if dense {
            for at in 0..self.postorder.len() {
                self.refit(self.postorder[at]);
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
            UpdateKind::CostRebuild
        } else if dense {
            UpdateKind::DenseRefit
        } else {
            UpdateKind::SparseRefit
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
        self.postorder
            .iter()
            .map(|&i| &self.nodes[i])
            .filter(|n| n.bound.is_finite())
            .map(|n| f64::from(n.bound.radius).powi(2))
            .sum()
    }
    fn rebuild(&mut self) {
        self.nodes.clear();
        self.free.clear();
        self.postorder.clear();
        self.root = None;
        self.order.clear();
        self.order.extend(0..self.bounds.len());
        self.leaves.resize(self.bounds.len(), 0);
        if !self.order.is_empty() {
            self.root = Some(self.build(0, self.order.len()));
        }
        self.current_cost = self.cost();
        self.build_cost = self.current_cost.max(f64::EPSILON);
        self.built_len = self.bounds.len();
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
            height: 0,
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
            self.nodes[at].height = 1 + self.nodes[a].height.max(self.nodes[b].height);
        }
        self.postorder.push(at);
        at
    }
    fn allocate(&mut self, node: Node) -> usize {
        if let Some(i) = self.free.pop() {
            self.nodes[i] = node;
            i
        } else {
            let i = self.nodes.len();
            self.nodes.push(node);
            i
        }
    }

    fn remove_leaf(&mut self, leaf: usize) {
        let parent = self.nodes[leaf].parent.expect("nonempty retained tree");
        let (a, b) = self.nodes[parent].children.unwrap();
        let sibling = if a == leaf { b } else { a };
        let grandparent = self.nodes[parent].parent;
        self.nodes[sibling].parent = grandparent;
        if let Some(g) = grandparent {
            let (a, b) = self.nodes[g].children.unwrap();
            self.nodes[g].children = Some(if a == parent {
                (sibling, b)
            } else {
                (a, sibling)
            });
        } else {
            self.root = Some(sibling);
        }
        self.free.extend([leaf, parent]);
    }

    fn insert_leaf(&mut self, item: usize) {
        let bound = self.bounds[item];
        let mut sibling = self.root.unwrap();
        while let Some((a, b)) = self.nodes[sibling].children {
            let score = |i: usize| {
                let n = self.nodes[i];
                (
                    Self::bound_cost(n.bound.union(&bound)) - Self::bound_cost(n.bound),
                    n.height,
                )
            };
            let (ca, ha) = score(a);
            let (cb, hb) = score(b);
            sibling = if ca.total_cmp(&cb).then(ha.cmp(&hb)).is_le() {
                a
            } else {
                b
            };
        }
        let old_parent = self.nodes[sibling].parent;
        let leaf = self.allocate(Node {
            bound,
            children: None,
            item,
            parent: None,
            range: (0, 0),
            height: 0,
        });
        let parent = self.allocate(Node {
            bound: self.nodes[sibling].bound.union(&bound),
            children: Some((sibling, leaf)),
            item,
            parent: old_parent,
            range: (0, 0),
            height: 1,
        });
        self.nodes[leaf].parent = Some(parent);
        self.nodes[sibling].parent = Some(parent);
        self.leaves[item] = leaf;
        if let Some(p) = old_parent {
            let (a, b) = self.nodes[p].children.unwrap();
            self.nodes[p].children = Some(if a == sibling {
                (parent, b)
            } else {
                (a, parent)
            });
        } else {
            self.root = Some(parent);
        }
        let mut next = Some(parent);
        while let Some(i) = next {
            self.refit(i);
            if let Some((a, b)) = self.nodes[i].children {
                self.nodes[i].height = 1 + self.nodes[a].height.max(self.nodes[b].height);
            }
            next = self.nodes[i].parent;
        }
    }

    // Linear metadata/refit pass, without spatial partitioning. Node slots may
    // be recycled, so numeric node order is no longer a traversal order.
    fn refresh_topology(&mut self, refit: bool) {
        self.order.clear();
        self.postorder.clear();
        if let Some(root) = self.root {
            self.refresh_subtree(root, refit);
        }
    }

    fn refresh_subtree(&mut self, i: usize, refit: bool) {
        let start = self.order.len();
        if let Some((a, b)) = self.nodes[i].children {
            self.refresh_subtree(a, refit);
            self.refresh_subtree(b, refit);
            self.nodes[i].height = 1 + self.nodes[a].height.max(self.nodes[b].height);
        } else {
            self.order.push(self.nodes[i].item);
        }
        if refit {
            self.refit(i);
        }
        self.nodes[i].range = (start, self.order.len());
        self.postorder.push(i);
    }

    pub fn overlaps(&mut self, sphere: &Sphere, out: &mut Vec<usize>) {
        out.clear();
        self.stack.clear();
        let finite = sphere.is_finite();
        if let Some(root) = self.root {
            self.stack.push(root);
        }
        while let Some(i) = self.stack.pop() {
            let node = &self.nodes[i];
            if finite
                && node.bound.is_finite()
                && sphere
                    .center
                    .as_dvec3()
                    .distance_squared(node.bound.center.as_dvec3())
                    > {
                        let reach = f64::from(sphere.radius) + f64::from(node.bound.radius);
                        reach * reach
                    }
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
        if let Some(root) = self.root {
            self.stack.push(root);
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
    #[ignore = "manual membership-churn qualification"]
    fn membership_churn_qualification() {
        let mut index = SpatialIndex::default();
        let mut bounds: Vec<_> = (0..6000)
            .map(|i| Sphere {
                center: glam::Vec3::new((i % 100) as f32, (i / 100) as f32, 0.0),
                radius: 0.25,
            })
            .collect();
        index.update(&bounds);
        let mut samples = Vec::new();
        for frame in 0..120 {
            if frame % 2 == 0 {
                bounds.push(bounds[frame]);
            } else {
                bounds.pop();
            }
            for b in &mut bounds {
                b.center.z += 0.001;
            }
            let start = std::time::Instant::now();
            std::hint::black_box(index.update(&bounds));
            samples.push(start.elapsed().as_secs_f64() * 1e6);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "membership_churn_us p50={} p95={}",
            samples[60], samples[114]
        );
    }

    #[test]
    fn update_kind_reports_the_work_and_resets_on_each_call() {
        let mut index = SpatialIndex::default();
        let mut bounds: Vec<_> = (0..16)
            .map(|i| Sphere {
                center: glam::Vec3::new(i as f32, 0.0, 0.0),
                radius: 1.0,
            })
            .collect();
        assert_eq!(index.update(&bounds), UpdateKind::MembershipRebuild);
        assert_eq!(index.update(&bounds), UpdateKind::Unchanged);
        bounds[0].center.y += 0.01;
        assert_eq!(index.update(&bounds), UpdateKind::SparseRefit);
        for b in &mut bounds {
            b.center.z += 0.01;
        }
        assert_eq!(index.update(&bounds), UpdateKind::DenseRefit);
        bounds[0].center.x = -10000.0;
        assert_eq!(index.update(&bounds), UpdateKind::CostRebuild);
        assert_eq!(index.update(&bounds), UpdateKind::Unchanged);
        bounds.pop();
        assert_eq!(index.update(&bounds), UpdateKind::MembershipRepair);
    }

    #[test]
    fn membership_repairs_match_linear_queries_across_shift_reuse_and_invalid_bounds() {
        let f = Frustum::from_view_proj(&glam::Mat4::IDENTITY);
        let sphere = Sphere {
            center: glam::Vec3::ZERO,
            radius: 1.0,
        };
        let mut bounds: Vec<_> = (0..128)
            .map(|i| Sphere {
                center: glam::Vec3::new(i as f32 * 0.04 - 2.5, 0.0, 0.5),
                radius: 0.02,
            })
            .collect();
        let mut index = SpatialIndex::default();
        let mut visible = Vec::new();
        let mut overlaps = Vec::new();
        let mut repairs = 0;
        for frame in 0..180 {
            // Shift dense record IDs through middle removals/additions; move
            // every survivor, recycle node slots and include unknown bounds.
            if frame % 2 == 0 {
                bounds.insert(3, sphere);
            } else {
                bounds.remove(7);
            }
            for b in &mut bounds {
                if b.is_finite() {
                    b.center.y = (frame as f32 * 0.1).sin();
                }
            }
            if frame % 9 == 0 {
                bounds[0] = Sphere::UNBOUNDED;
            } else {
                bounds[0] = Sphere {
                    center: glam::Vec3::ZERO,
                    radius: 0.1,
                };
            }
            if frame == 70 {
                bounds[5].center = glam::Vec3::NAN;
            }
            let kind = index.update(&bounds);
            repairs += usize::from(kind == UpdateKind::MembershipRepair);
            index.query(&f, &mut visible);
            assert_eq!(
                visible,
                bounds.iter().map(|b| f.intersects(b)).collect::<Vec<_>>()
            );
            index.overlaps(&sphere, &mut overlaps);
            let expected: Vec<_> = bounds
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    (!b.is_finite()
                        || sphere.center.as_dvec3().distance(b.center.as_dvec3())
                            <= f64::from(sphere.radius) + f64::from(b.radius))
                    .then_some(i)
                })
                .collect();
            assert_eq!(overlaps, expected);
            let mut order = index.order.clone();
            order.sort_unstable();
            assert_eq!(order, (0..bounds.len()).collect::<Vec<_>>());
            assert_eq!(index.postorder.len(), bounds.len() * 2 - 1);
            assert!(
                index.nodes.len() <= (bounds.len() + 1) * 2 - 1,
                "freed nodes must be reused"
            );
            for &i in &index.postorder {
                if let Some((a, b)) = index.nodes[i].children {
                    assert_eq!(index.nodes[a].parent, Some(i));
                    assert_eq!(index.nodes[b].parent, Some(i));
                } else {
                    assert_eq!(index.leaves[index.nodes[i].item], i);
                }
            }
        }
        assert!(
            repairs > 150,
            "ordinary churn should not require repartitioning"
        );
        for n in [0, 1, 2, 32, 3, 0, 128] {
            bounds.resize(n, sphere);
            index.update(&bounds);
            index.query(&f, &mut visible);
            assert_eq!(
                visible,
                bounds.iter().map(|b| f.intersects(b)).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn ordered_growth_keeps_depth_bounded_and_queries_current() {
        let mut index = SpatialIndex::default();
        let mut bounds: Vec<_> = (0..128)
            .map(|i| Sphere {
                center: glam::Vec3::new(i as f32, 0.0, 0.0),
                radius: 0.1,
            })
            .collect();
        let mut visible = Vec::new();
        let f = Frustum::from_view_proj(&glam::Mat4::IDENTITY);
        for i in 128..600 {
            bounds.push(Sphere {
                center: glam::Vec3::new(i as f32, 0.0, 0.0),
                radius: 0.1,
            });
            index.update(&bounds);
            assert!(
                index.nodes[index.root.unwrap()].height <= 2 * bounds.len().ilog2() as usize + 4
            );
            index.query(&f, &mut visible);
            assert_eq!(
                visible,
                bounds.iter().map(|b| f.intersects(b)).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn cumulative_small_growth_refreshes_the_build_reference() {
        let sphere = Sphere {
            center: glam::Vec3::ZERO,
            radius: 1.0,
        };
        let mut bounds = vec![sphere; 128];
        let mut index = SpatialIndex::default();
        index.update(&bounds);
        for _ in 0..16 {
            bounds.push(sphere);
            assert_eq!(index.update(&bounds), UpdateKind::MembershipRepair);
        }
        bounds.push(sphere);
        assert_eq!(index.update(&bounds), UpdateKind::MembershipRebuild);
        assert_eq!(index.built_len, 145);
    }

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
