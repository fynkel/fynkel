//! Text made solid: each letter's outline read from a TrueType or
//! OpenType font, its curves cut into short straight edges, the loops
//! inside others made holes (the middle of an "o", a "B"'s two), and each
//! piece extruded as [`MeshData::extrude`] does.

use glam::{Mat4, Vec2};
use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Tag};

use crate::{MeshData, MeshError};

/// How far a curve cut into straight edges may stray from the font's, as
/// a part of the font's size: 1 mm on a letter a metre tall.
const CURVE_TOLERANCE: f32 = 0.001;

/// The most edges one curve is cut into.
const MOST_EDGES_PER_CURVE: u32 = 32;

/// A TrueType (.ttf) or OpenType (.otf) font, read from its file's bytes;
/// the first in a collection (.ttc).
pub struct Font<'a> {
    face: Face<'a>,
    /// The GPOS lookups its `kern` feature uses, for a font with no `kern`
    /// table: most made since OpenType.
    kern_lookups: Vec<u16>,
}

impl std::fmt::Debug for Font<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font").field("units_per_em", &self.face.units_per_em()).finish_non_exhaustive()
    }
}

/// Where each line goes across the origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    /// Starts at the origin.
    Left,
    /// Centred on it.
    Center,
    /// Ends at it.
    Right,
}

/// How text is laid out.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    /// The font's size, its em, in metres: capitals are about 0.7 of it.
    pub size: f32,
    pub align: TextAlign,
    /// Lines are this many times the font's own line height apart.
    pub line_spacing: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { size: 1.0, align: TextAlign::Center, line_spacing: 1.0 }
    }
}

/// One piece of a letter: an outline and the holes inside it, in metres,
/// as [`MeshData::extrude`] takes them.
#[derive(Clone, Debug, Default)]
pub struct TextShape {
    pub outline: Vec<Vec2>,
    pub holes: Vec<Vec<Vec2>>,
}

impl<'a> Font<'a> {
    /// The font in `data`, or a [`MeshError::Outline`] saying why it isn't
    /// one.
    pub fn parse(data: &'a [u8]) -> Result<Self, MeshError> {
        let face =
            Face::parse(data, 0).map_err(|e| MeshError::Outline(format!("not a font the engine can read: {e}")))?;
        let mut kern_lookups: Vec<u16> = (face.tables().gpos.iter())
            .flat_map(|gpos| gpos.features.into_iter())
            .filter(|f| f.tag == Tag::from_bytes(b"kern"))
            .flat_map(|f| f.lookup_indices.into_iter())
            .collect();
        kern_lookups.sort_unstable();
        kern_lookups.dedup();
        Ok(Font { face, kern_lookups })
    }

    /// How far apart lines are, as a part of the font's size: ascender
    /// less descender, plus the gap the font asks for.
    pub fn line_height(&self) -> f32 {
        let f = &self.face;
        (f.ascender() as f32 - f.descender() as f32 + f.line_gap() as f32) / f.units_per_em() as f32
    }

    /// How wide the widest line of `text` is and how tall the lines are
    /// together, in metres, at `style`'s size and spacing.
    pub fn measure(&self, text: &str, style: &TextStyle) -> Vec2 {
        let scale = style.size / self.face.units_per_em() as f32;
        let widest = text.split('\n').map(|line| self.advance(line)).fold(0.0, f32::max);
        let lines = text.split('\n').count() as f32;
        Vec2::new(widest * scale, lines * self.line_height() * style.line_spacing * style.size)
    }

    /// `text`'s letters as outlines with holes, in the XY plane: the first
    /// line's letters stand on the X axis, each line below the last, and
    /// each is aligned on the origin. `\n` starts a line; a character the
    /// font hasn't got is drawn as its missing-letter box, when it has one.
    pub fn shapes(&self, text: &str, style: &TextStyle) -> Vec<TextShape> {
        let face = &self.face;
        let scale = style.size / face.units_per_em() as f32;
        let step = self.line_height() * style.line_spacing * style.size;
        let mut contours = Contours::new(face.units_per_em() as f32 * CURVE_TOLERANCE);
        let mut shapes = Vec::new();
        for (row, line) in text.split('\n').enumerate() {
            let width = self.advance(line) * scale;
            let start = match style.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => -width * 0.5,
                TextAlign::Right => -width,
            };
            let mut pen = 0.0;
            let mut before: Option<GlyphId> = None;
            for c in line.chars() {
                let glyph = face.glyph_index(c).unwrap_or(GlyphId(0));
                if let Some(left) = before {
                    pen += self.kerning(left, glyph);
                }
                contours.all.clear();
                if face.outline_glyph(glyph, &mut contours).is_some() {
                    let at = Vec2::new(start + pen * scale, -(row as f32) * step);
                    for contour in &mut contours.all {
                        for p in contour.iter_mut() {
                            *p = *p * scale + at;
                        }
                    }
                    shapes.extend(nest(&contours.all));
                }
                pen += face.glyph_hor_advance(glyph).unwrap_or(0) as f32;
                before = Some(glyph);
            }
        }
        shapes
    }

    /// How far `line` takes the pen, in the font's units, kerning
    /// included.
    fn advance(&self, line: &str) -> f32 {
        let mut pen = 0.0;
        let mut before: Option<GlyphId> = None;
        for c in line.chars() {
            let glyph = self.face.glyph_index(c).unwrap_or(GlyphId(0));
            if let Some(left) = before {
                pen += self.kerning(left, glyph);
            }
            pen += self.face.glyph_hor_advance(glyph).unwrap_or(0) as f32;
            before = Some(glyph);
        }
        pen
    }

    /// How much closer (below 0) or further apart `right` goes after
    /// `left`, in the font's units: from its `kern` table, or else the
    /// pairs in its GPOS `kern` lookups.
    fn kerning(&self, left: GlyphId, right: GlyphId) -> f32 {
        let tables = self.face.tables();
        if let Some(kern) = tables.kern {
            return (kern.subtables.into_iter())
                .filter(|s| s.horizontal && !s.variable && !s.has_cross_stream)
                .find_map(|s| s.glyphs_kerning(left, right))
                .map_or(0.0, f32::from);
        }
        let Some(gpos) = tables.gpos else { return 0.0 };
        let mut sum = 0.0;
        for &i in &self.kern_lookups {
            let Some(lookup) = gpos.lookups.get(i) else { continue };
            // In a lookup, the first subtable with the pair says.
            let found = lookup.subtables.into_iter::<PositioningSubtable>().find_map(|sub| match sub {
                PositioningSubtable::Pair(PairAdjustment::Format1 { coverage, sets }) => {
                    sets.get(coverage.get(left)?)?.get(right).map(|(first, _)| first.x_advance)
                }
                PositioningSubtable::Pair(PairAdjustment::Format2 { coverage, classes, matrix }) => {
                    coverage.get(left)?;
                    matrix.get((classes.0.get(left), classes.1.get(right))).map(|(first, _)| first.x_advance)
                }
                _ => None,
            });
            sum += found.map_or(0.0, f32::from);
        }
        sum
    }
}

impl MeshData {
    /// `text` in `font` made solid, each letter a slab `depth` thick along
    /// Z centred on the origin, its front facing +Z, laid out as
    /// [`Font::shapes`] says. Textured as [`MeshData::extrude`]: flat on
    /// the front and back, `uv_per_metre` times a metre, so across the
    /// letters as one. Text with nothing to draw (spaces) makes an empty
    /// mesh.
    pub fn text(font: &Font, text: &str, style: &TextStyle, depth: f32, uv_per_metre: f32) -> Result<Self, MeshError> {
        let mut pieces = Vec::new();
        for shape in font.shapes(text, style) {
            let holes: Vec<&[Vec2]> = shape.holes.iter().map(Vec::as_slice).collect();
            pieces.push(MeshData::extrude(&shape.outline, &holes, depth, uv_per_metre)?);
        }
        if pieces.is_empty() {
            return Ok(MeshData::empty());
        }
        let placed: Vec<(&MeshData, Mat4)> = pieces.iter().map(|m| (m, Mat4::IDENTITY)).collect();
        MeshData::combine(&placed)
    }
}

/// A glyph's loops, its curves cut into edges no further than `tolerance`
/// (in the font's units) from them.
struct Contours {
    all: Vec<Vec<Vec2>>,
    now: Vec<Vec2>,
    tolerance: f32,
}

impl Contours {
    fn new(tolerance: f32) -> Self {
        Contours { all: Vec::new(), now: Vec::new(), tolerance }
    }

    fn last(&self) -> Vec2 {
        self.now.last().copied().unwrap_or(Vec2::ZERO)
    }

    /// The loop so far as one, less points on top of the one before and a
    /// last point back on the first; one of fewer than 3 points or no area
    /// is dropped.
    fn finish(&mut self) {
        let mut ring = std::mem::take(&mut self.now);
        ring.dedup_by(|b, a| (*b - *a).length_squared() <= 1e-12);
        while ring.len() > 1 && (ring[0] - ring[ring.len() - 1]).length_squared() <= 1e-12 {
            ring.pop();
        }
        if ring.len() >= 3 && signed_area(&ring).abs() > 0.0 {
            self.all.push(ring);
        }
    }

    fn edges(&self, bend: f32) -> u32 {
        ((bend / self.tolerance).sqrt().ceil() as u32).clamp(1, MOST_EDGES_PER_CURVE)
    }
}

impl OutlineBuilder for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.now.push(Vec2::new(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.now.push(Vec2::new(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b, c) = (self.last(), Vec2::new(x1, y1), Vec2::new(x, y));
        // A chord strays at most |a - 2b + c| / 4n² from the curve.
        let n = self.edges((a - 2.0 * b + c).length() / 4.0);
        for k in 1..=n {
            let t = k as f32 / n as f32;
            let u = 1.0 - t;
            self.now.push(a * u * u + b * 2.0 * u * t + c * t * t);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, c, d) = (self.last(), Vec2::new(x1, y1), Vec2::new(x2, y2), Vec2::new(x, y));
        // And a cubic's at most 3·max(|a - 2b + c|, |b - 2c + d|) / 4n².
        let bend = (a - 2.0 * b + c).length().max((b - 2.0 * c + d).length());
        let n = self.edges(bend * 0.75);
        for k in 1..=n {
            let t = k as f32 / n as f32;
            let u = 1.0 - t;
            self.now.push(a * u * u * u + b * 3.0 * u * u * t + c * 3.0 * u * t * t + d * t * t * t);
        }
    }

    fn close(&mut self) {
        self.finish();
    }
}

/// `loops` sorted into pieces: a loop inside an even number of others is
/// an outline, and one inside an odd number a hole in the smallest outline
/// around it. Fonts wind outlines and holes opposite ways, but which way
/// differs between TrueType and OpenType, so nesting decides.
fn nest(loops: &[Vec<Vec2>]) -> Vec<TextShape> {
    let areas: Vec<f32> = loops.iter().map(|l| signed_area(l).abs()).collect();
    // Which loops each is inside.
    let around: Vec<Vec<usize>> = (0..loops.len())
        .map(|i| (0..loops.len()).filter(|&j| j != i && areas[j] > areas[i] && inside(&loops[i], &loops[j])).collect())
        .collect();
    let mut shapes: Vec<Option<TextShape>> = vec![None; loops.len()];
    for i in 0..loops.len() {
        if around[i].len().is_multiple_of(2) {
            shapes[i] = Some(TextShape { outline: loops[i].clone(), holes: Vec::new() });
        }
    }
    for i in 0..loops.len() {
        if !around[i].len().is_multiple_of(2) {
            let depth = around[i].len() - 1;
            let parent = around[i]
                .iter()
                .copied()
                .filter(|&j| around[j].len() == depth)
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]));
            if let Some(shape) = parent.and_then(|j| shapes[j].as_mut()) {
                shape.holes.push(loops[i].clone());
            }
        }
    }
    shapes.into_iter().flatten().collect()
}

/// Whether the loop `a` is inside the loop `b`: the first of `a`'s points
/// not on `b`'s edges says. Loops in a font touch at most, never cross.
fn inside(a: &[Vec2], b: &[Vec2]) -> bool {
    a.iter().find_map(|&p| strictly_inside(p, b)).unwrap_or(false)
}

/// Whether `q` is inside `ring`, or none when it's on an edge.
fn strictly_inside(q: Vec2, ring: &[Vec2]) -> Option<bool> {
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let (along, to) = (b - a, q - a);
        let on_line = along.perp_dot(to).abs() <= 1e-6 * along.length() * (1.0 + q.abs().max_element());
        if on_line && to.dot(along) >= 0.0 && to.dot(along) <= along.length_squared() {
            return None;
        }
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    Some(inside)
}

/// Twice the signed area: more than 0 for counter-clockwise.
fn signed_area(ring: &[Vec2]) -> f32 {
    (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum()
}
