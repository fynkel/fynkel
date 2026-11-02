//! A box tree over the world boxes of the objects a ray can hit (stage
//! 16's phase 4), so a cast looks at the few objects along its line
//! instead of every one: 100 tags among 3,000 cubes cost the Note10+'s UI
//! thread 1.27 ms a frame, every cube's box tested on each look.
//!
//! Made once enough casts have reached the same objects; as they move,
//! each box is worked out again and the tree's boxes refitted around them
//! (one pass, no sorting), and it's made again when the objects change or
//! the refitted boxes have grown to twice the area they had, as things
//! wander from where the tree split them.

use glam::Vec3;

/// Objects a leaf holds at most.
const LEAF: usize = 4;

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: Vec3,
    hi: Vec3,
    /// A leaf's first object in [`ObjectTree::order`]; an inner node's
    /// second child (its first is the next node).
    start: u32,
    /// A leaf's object count; 0 for an inner node.
    count: u32,
}

/// The tree over a list of boxes, by their indices in it.
#[derive(Debug, Default)]
pub(crate) struct ObjectTree {
    nodes: Vec<Node>,
    /// The boxes' indices in the leaves' order.
    order: Vec<u32>,
    /// The inner nodes' boxes' area when it was made.
    made_area: f32,
}

impl ObjectTree {
    /// Whether it holds anything.
    pub(crate) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Makes it again over `boxes` (each a low and a high corner).
    pub(crate) fn make(&mut self, boxes: &[(Vec3, Vec3)]) {
        self.nodes.clear();
        self.order.clear();
        self.order.extend(0..boxes.len() as u32);
        if !boxes.is_empty() {
            let mut order = std::mem::take(&mut self.order);
            self.build(boxes, &mut order, 0, boxes.len());
            self.order = order;
        }
        self.made_area = self.inner_area();
    }

    /// Fits its boxes to `boxes` again, the same list it was made over with
    /// the boxes moved. False when they've grown so much it should be made
    /// again.
    pub(crate) fn refit(&mut self, boxes: &[(Vec3, Vec3)]) -> bool {
        // Children come after their parent, so from the back each node's
        // children are already fitted.
        for at in (0..self.nodes.len()).rev() {
            let node = self.nodes[at];
            let (lo, hi) = if node.count > 0 {
                let leaf = &self.order[node.start as usize..(node.start + node.count) as usize];
                leaf.iter().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), &i| {
                    let (a, b) = boxes[i as usize];
                    (lo.min(a), hi.max(b))
                })
            } else {
                let (first, second) = (self.nodes[at + 1], self.nodes[node.start as usize]);
                (first.lo.min(second.lo), first.hi.max(second.hi))
            };
            (self.nodes[at].lo, self.nodes[at].hi) = (lo, hi);
        }
        self.inner_area() <= 2.0 * self.made_area.max(f32::MIN_POSITIVE)
    }

    /// The inner nodes' boxes' surface area, added up: what a ray pays to
    /// go through the tree, roughly.
    fn inner_area(&self) -> f32 {
        self.nodes.iter().filter(|n| n.count == 0).map(|n| area(n.lo, n.hi)).sum()
    }

    /// Makes the node for `order[from..to]`, and those below it.
    fn build(&mut self, boxes: &[(Vec3, Vec3)], order: &mut [u32], from: usize, to: usize) {
        let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        let (mut centres_lo, mut centres_hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        let centre = |i: u32| {
            let (a, b) = boxes[i as usize];
            (a + b) * 0.5
        };
        for &i in &order[from..to] {
            let (a, b) = boxes[i as usize];
            (lo, hi) = (lo.min(a), hi.max(b));
            (centres_lo, centres_hi) = (centres_lo.min(centre(i)), centres_hi.max(centre(i)));
        }
        let at = self.nodes.len();
        self.nodes.push(Node { lo, hi, start: from as u32, count: (to - from) as u32 });
        let size = centres_hi - centres_lo;
        // All the centres at one point can't be split: a leaf, however many.
        if to - from <= LEAF || size.max_element() <= 0.0 || !size.is_finite() {
            return;
        }
        let axis = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        let split = (centres_lo[axis] + centres_hi[axis]) * 0.5;
        let (mut left, mut right) = (from, to);
        while left < right {
            if centre(order[left])[axis] < split {
                left += 1;
            } else {
                right -= 1;
                order.swap(left, right);
            }
        }
        let middle = if left == from || left == to { from + (to - from) / 2 } else { left };
        self.build(boxes, order, from, middle);
        let second = self.nodes.len() as u32;
        self.nodes[at].count = 0;
        self.nodes[at].start = second;
        self.build(boxes, order, middle, to);
    }

    /// Calls `visit` with each box index whose leaf the ray from `origin`
    /// along `direction` enters, nearer leaves first, while the leaf's box
    /// starts no further than `reach()` (asked again as hits shorten it).
    pub(crate) fn walk(
        &self,
        origin: Vec3,
        direction: Vec3,
        mut reach: impl FnMut() -> f32,
        mut visit: impl FnMut(usize),
    ) {
        if self.nodes.is_empty() {
            return;
        }
        let inverse = direction.recip();
        // A tree over 2^32 objects is under 64 deep.
        let mut stack = [(0_u32, 0.0_f32); 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let (index, enter) = stack[top];
            let limit = reach();
            // A box starting at the closest hit so far may still hold one
            // as close, which a tie may favour: only those beyond go.
            if enter > limit {
                continue;
            }
            let node = self.nodes[index as usize];
            if node.count > 0 {
                for &i in &self.order[node.start as usize..(node.start + node.count) as usize] {
                    visit(i as usize);
                }
                continue;
            }
            let (first, second) = (index + 1, node.start);
            let near = |n: u32| {
                let n = self.nodes[n as usize];
                enters(origin, inverse, n.lo, n.hi, limit)
            };
            match (near(first), near(second)) {
                (Some(a), Some(b)) => {
                    // The farther pushed first, so the nearer is taken next.
                    let (far, near) = if a <= b { ((second, b), (first, a)) } else { ((first, a), (second, b)) };
                    stack[top] = far;
                    stack[top + 1] = near;
                    top += 2;
                }
                (Some(a), None) => {
                    stack[top] = (first, a);
                    top += 1;
                }
                (None, Some(b)) => {
                    stack[top] = (second, b);
                    top += 1;
                }
                (None, None) => {}
            }
        }
    }
}

/// A box's surface area, half of it.
fn area(lo: Vec3, hi: Vec3) -> f32 {
    let d = (hi - lo).max(Vec3::ZERO);
    d.x * d.y + d.y * d.z + d.z * d.x
}

/// Where the ray enters the box, if no further than `reach` (the slab
/// test, with the direction's reciprocal).
fn enters(origin: Vec3, inverse: Vec3, lo: Vec3, hi: Vec3, reach: f32) -> Option<f32> {
    let a = (lo - origin) * inverse;
    let b = (hi - origin) * inverse;
    let near = a.min(b).max_element().max(0.0);
    let far = a.max(b).min_element().min(reach);
    (near <= far).then_some(near)
}
