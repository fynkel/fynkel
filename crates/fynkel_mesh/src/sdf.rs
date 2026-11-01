//! Flat text (stage 15 §3.2): each letter drawn once into an atlas as its
//! distance from the outline, as TextMeshPro's signed distance fields. A
//! letter is then one square, crisp at any size, and its outline and
//! softness are a shader's numbers.
//!
//! A pixel is 128 on the letter's edge, more inside and less outside, by
//! 127 over [`SdfAtlas::SPREAD`] pixels. Letters are packed on shelves,
//! the atlas growing (taller, then wider) as they come; one drawn keeps its
//! place and its pixels for the atlas's life.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec2;

use crate::MeshError;
use crate::text::{Font, TextAlign, TextStyle};

/// The atlas's first size.
const START: (u32, u32) = (512, 128);

/// The largest it grows to, each way: what every phone's GPU takes.
const LARGEST: u32 = 4096;

/// Pixels left empty round each letter, so sampling one never reads its
/// neighbour.
const GAP: u32 = 1;

/// One letter in the atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasGlyph {
    /// Its pixels: x, y (from the top-left), width and height.
    pub rect: [u16; 4],
    /// The square they cover, as parts of the font's size from the pen's
    /// point: left, bottom, right, top; the spread round the outline
    /// included.
    pub bounds: [f32; 4],
}

/// The letters of one font drawn as distance fields: one byte a pixel.
#[derive(Debug)]
pub struct SdfAtlas {
    size: (u32, u32),
    pixels: Vec<u8>,
    /// Each letter asked for, `None` for one with nothing to draw.
    glyphs: HashMap<u16, Option<AtlasGlyph>>,
    /// Each shelf's top, height and how far along it is filled.
    shelves: Vec<(u32, u32, u32)>,
    version: u64,
    /// A letter's loops, kept so drawing one allocates little.
    loops: Vec<Vec<Vec2>>,
}

impl Default for SdfAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl SdfAtlas {
    /// Pixels in the font's em: capitals about 34 tall.
    pub const EM_PIXELS: f32 = 48.0;

    /// How many pixels each side of the outline the distance reaches.
    pub const SPREAD: f32 = 6.0;

    /// An empty atlas.
    pub fn new() -> Self {
        SdfAtlas {
            size: START,
            pixels: vec![0; (START.0 * START.1) as usize],
            glyphs: HashMap::new(),
            shelves: Vec::new(),
            version: 0,
            loops: Vec::new(),
        }
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Its pixels, row by row from the top: width × height bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Goes up each time a letter is drawn into it.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// `glyph` as drawn, when it has been.
    pub fn glyph_known(&self, glyph: u16) -> Option<AtlasGlyph> {
        self.glyphs.get(&glyph).copied().flatten()
    }

    /// `glyph` of `font`, drawn into the atlas the first time it's asked
    /// for; `None` for one with nothing to draw (a space), or once the
    /// atlas is as large as it grows and full.
    pub fn glyph(&mut self, font: &Font, glyph: u16) -> Option<AtlasGlyph> {
        if let Some(&known) = self.glyphs.get(&glyph) {
            return known;
        }
        let drawn = self.draw(font, glyph);
        self.glyphs.insert(glyph, drawn);
        drawn
    }

    fn draw(&mut self, font: &Font, glyph: u16) -> Option<AtlasGlyph> {
        let mut loops = std::mem::take(&mut self.loops);
        let has = font.outline_into(glyph, &mut loops);
        let drawn = has.then(|| self.draw_loops(font.units_per_em(), &loops)).flatten();
        self.loops = loops;
        drawn
    }

    fn draw_loops(&mut self, units_per_em: f32, loops: &[Vec<Vec2>]) -> Option<AtlasGlyph> {
        // Pixels a font unit, and the letter's box in its units.
        let scale = Self::EM_PIXELS / units_per_em;
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in loops.iter().flatten() {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let pad = Self::SPREAD + 1.0;
        let width = ((hi.x - lo.x) * scale + 2.0 * pad).ceil() as u32;
        let height = ((hi.y - lo.y) * scale + 2.0 * pad).ceil() as u32;
        let (x, y) = self.place(width, height)?;
        // Its top-left corner in the font's units.
        let left = lo.x - pad / scale;
        let top = hi.y + pad / scale;
        let reach = Self::SPREAD / scale;
        let stride = self.size.0 as usize;
        for row in 0..height {
            let py = top - (row as f32 + 0.5) / scale;
            for column in 0..width {
                let p = Vec2::new(left + (column as f32 + 0.5) / scale, py);
                let (distance, winding) = nearest(p, loops);
                let signed = if winding != 0 { distance } else { -distance };
                let value = (0.5 + signed / (2.0 * reach)).clamp(0.0, 1.0);
                self.pixels[(y + row) as usize * stride + (x + column) as usize] = (value * 255.0).round() as u8;
            }
        }
        self.version += 1;
        let em = |v: f32| v / units_per_em;
        let right = left + width as f32 / scale;
        let bottom = top - height as f32 / scale;
        Some(AtlasGlyph {
            rect: [x as u16, y as u16, width as u16, height as u16],
            bounds: [em(left), em(bottom), em(right), em(top)],
        })
    }

    /// Room for a `width` × `height` letter: on the first shelf it fits,
    /// or a new one, the atlas growing for it when it must.
    fn place(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        let (w, h) = (width + GAP, height + GAP);
        loop {
            let found = self.shelves.iter_mut().find(|(_, tall, filled)| *tall >= h && *filled + w <= self.size.0);
            if let Some((top, _, filled)) = found {
                let at = (*filled, *top);
                *filled += w;
                return Some(at);
            }
            let next = self.shelves.last().map_or(0, |(top, tall, _)| top + tall);
            if next + h <= self.size.1 && w <= self.size.0 {
                self.shelves.push((next, h, w));
                return Some((0, next));
            }
            self.grow()?;
        }
    }

    /// Twice as tall, or once as tall as it grows, twice as wide; keeping
    /// every pixel where it was.
    fn grow(&mut self) -> Option<()> {
        let (w, h) = self.size;
        let size = if h < LARGEST {
            (w, h * 2)
        } else if w < LARGEST {
            (w * 2, h)
        } else {
            return None;
        };
        let mut pixels = vec![0; (size.0 * size.1) as usize];
        for row in 0..h as usize {
            let from = row * w as usize;
            pixels[row * size.0 as usize..][..w as usize].copy_from_slice(&self.pixels[from..from + w as usize]);
        }
        self.pixels = pixels;
        self.size = size;
        self.version += 1;
        Some(())
    }
}

/// How far `p` is from the nearest edge of `loops`, and their winding
/// round it (not 0: inside, as fonts fill).
fn nearest(p: Vec2, loops: &[Vec<Vec2>]) -> (f32, i32) {
    let mut best = f32::MAX;
    let mut winding = 0;
    for ring in loops {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
            best = best.min((a + ab * t - p).length_squared());
            if a.y <= p.y {
                if b.y > p.y && ab.perp_dot(p - a) > 0.0 {
                    winding += 1;
                }
            } else if b.y <= p.y && ab.perp_dot(p - a) < 0.0 {
                winding -= 1;
            }
        }
    }
    (best.sqrt(), winding)
}

/// One letter of flat text: the square it's drawn on and its pixels in
/// the atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphQuad {
    /// In metres from the text's origin: left, bottom, right, top.
    pub rect: [f32; 4],
    /// Its pixels in the atlas: x, y (from the top-left), width, height.
    pub pixels: [u16; 4],
}

/// What a font keeps of a character once it's met it.
#[derive(Clone, Copy, Debug)]
struct Letter {
    glyph: u16,
    /// How far it takes the pen, as a part of the font's size.
    advance: f32,
    drawn: Option<AtlasGlyph>,
}

/// A font for flat text: its file, its atlas, and what it has read of the
/// file, so laying text out reads it again only for a character or a pair
/// of them it hasn't met. A number changed every frame costs only its
/// layout.
#[derive(Debug)]
pub struct SdfFont {
    data: Arc<[u8]>,
    atlas: SdfAtlas,
    letters: HashMap<char, Letter>,
    /// How much closer each pair of letters goes, as a part of the size.
    pairs: HashMap<(u16, u16), f32>,
    line_height: f32,
    reads: u64,
}

impl SdfFont {
    /// The font in `data`, or a [`MeshError::Outline`] saying why it isn't
    /// one.
    pub fn new(data: Arc<[u8]>) -> Result<Self, MeshError> {
        let line_height = Font::parse(&data)?.line_height();
        Ok(SdfFont {
            data,
            atlas: SdfAtlas::new(),
            letters: HashMap::new(),
            pairs: HashMap::new(),
            line_height,
            reads: 0,
        })
    }

    /// Its file's bytes.
    pub fn data(&self) -> &Arc<[u8]> {
        &self.data
    }

    pub fn atlas(&self) -> &SdfAtlas {
        &self.atlas
    }

    /// As [`Font::line_height`].
    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    /// How many times it has read its file for something it hadn't met.
    pub fn reads(&self) -> u64 {
        self.reads
    }

    /// `text`'s letters as squares, in place of what `out` had, laid out
    /// as [`Font::layout`] lays them out; letters with nothing to draw
    /// (spaces) left out. New letters are drawn into the atlas. Allocates
    /// nothing once `out` has room and every letter and pair is known.
    pub fn quads(&mut self, text: &str, style: &TextStyle, out: &mut Vec<GlyphQuad>) {
        out.clear();
        self.learn(text);
        let step = self.line_height * style.line_spacing * style.size;
        for (row, line) in text.split('\n').enumerate() {
            let width = self.advance(line) * style.size;
            let mut pen = match style.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => -width * 0.5,
                TextAlign::Right => -width,
            };
            let y = -(row as f32) * step;
            let mut before: Option<u16> = None;
            for c in line.chars() {
                let letter = self.letters[&c];
                if let Some(left) = before {
                    pen += self.pairs[&(left, letter.glyph)] * style.size;
                }
                if let Some(g) = letter.drawn {
                    let [l, b, r, t] = g.bounds.map(|v| v * style.size);
                    out.push(GlyphQuad { rect: [pen + l, y + b, pen + r, y + t], pixels: g.rect });
                }
                pen += letter.advance * style.size;
                before = Some(letter.glyph);
            }
        }
    }

    /// How far `line` takes the pen, as a part of the size; every letter
    /// and pair in it known.
    fn advance(&self, line: &str) -> f32 {
        let mut pen = 0.0;
        let mut before: Option<u16> = None;
        for c in line.chars() {
            let letter = self.letters[&c];
            if let Some(left) = before {
                pen += self.pairs[&(left, letter.glyph)];
            }
            pen += letter.advance;
            before = Some(letter.glyph);
        }
        pen
    }

    /// Reads the file, once, for whatever of `text` isn't known yet.
    fn learn(&mut self, text: &str) {
        let known = |s: &Self| {
            text.split('\n').all(|line| {
                let mut before: Option<u16> = None;
                line.chars().all(|c| {
                    let Some(letter) = s.letters.get(&c) else { return false };
                    let paired = before.is_none_or(|left| s.pairs.contains_key(&(left, letter.glyph)));
                    before = Some(letter.glyph);
                    paired
                })
            })
        };
        if known(self) {
            return;
        }
        self.reads += 1;
        let data = Arc::clone(&self.data);
        let font = Font::parse(&data).expect("checked when it was made");
        let em = font.units_per_em();
        for line in text.split('\n') {
            let mut before: Option<u16> = None;
            for c in line.chars() {
                let letter = match self.letters.get(&c) {
                    Some(&letter) => letter,
                    None => {
                        let glyph = font.glyph_id(c).unwrap_or(0);
                        let drawn = self.atlas.glyph(&font, glyph);
                        let letter = Letter { glyph, advance: font.advance_of(glyph), drawn };
                        self.letters.insert(c, letter);
                        letter
                    }
                };
                if let Some(left) = before {
                    self.pairs.entry((left, letter.glyph)).or_insert_with(|| font.kerning_of(left, letter.glyph) / em);
                }
                before = Some(letter.glyph);
            }
        }
    }
}
