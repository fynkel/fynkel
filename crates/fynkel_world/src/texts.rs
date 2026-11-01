//! Flat text in the scene (`docs/stage-15.md` §3.2): a [`TextRenderer`] on
//! an object draws its text as TextMeshPro does, each letter a square of
//! its font's distance-field atlas, crisp at any size, an outline round it
//! if asked. The world keeps each font's atlas ([`World::add_font`]); a
//! text is laid out into its squares when it's set, not each frame, so a
//! number changed every frame costs its layout and nothing more.

use std::sync::Arc;

use fynkel_mesh::{GlyphQuad, SdfFont, TextAlign, TextStyle};
use glam::{Vec2, Vec3};

use crate::world::{Result, World, WorldError};
use crate::{Color, FontId, ObjectId};

/// Which way a text (or, later, a sprite) faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Facing {
    /// As its object turns: its front along the object's +Z.
    #[default]
    None,
    /// Square to the camera, its top the camera's up.
    Camera,
    /// Turned round up (Y) to face the camera, standing upright, as a tree.
    Upright,
    /// Flat, facing up, its top away along -Z.
    Up,
}

/// A text renderer's settings (TextMeshPro's).
#[derive(Clone, Debug, PartialEq)]
pub struct TextSettings {
    pub font: FontId,
    /// The font's size, its em, in metres.
    pub size: f32,
    pub align: TextAlign,
    /// Lines are this many times the font's own line height apart.
    pub line_spacing: f32,
    /// The letters' colour.
    pub color: Color,
    /// How wide the outline round each letter is: 0 none, 1 as far out as
    /// the atlas reaches (about a ninth of the size).
    pub outline: f32,
    pub outline_color: Color,
    /// How soft the letters' edges are: 0 crisp, 1 blurred across the
    /// atlas's reach.
    pub softness: f32,
    /// A shadow under the letters (TextMeshPro's underlay): none while
    /// its alpha is 0.
    pub shadow_color: Color,
    /// Where the shadow is from the letters, in ems: (0.05, -0.05) a
    /// little down and right.
    pub shadow_offset: Vec2,
    /// How soft the shadow's edges are, 0 to 1, as `softness`.
    pub shadow_softness: f32,
    pub facing: Facing,
    /// Drawn over everything, walls and all (a hit number).
    pub on_top: bool,
    /// Lit as the scene is; else shown as its colour says, in the dark too.
    pub lit: bool,
}

impl TextSettings {
    /// White, centred, unlit, as the object turns, at `size`.
    pub fn new(font: FontId, size: f32) -> Self {
        TextSettings {
            font,
            size,
            align: TextAlign::Center,
            line_spacing: 1.0,
            color: Color::WHITE,
            outline: 0.0,
            outline_color: Color::BLACK,
            softness: 0.0,
            shadow_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
            shadow_offset: Vec2::new(0.05, -0.05),
            shadow_softness: 0.0,
            facing: Facing::None,
            on_top: false,
            lit: false,
        }
    }

    fn refused(&self) -> Option<&'static str> {
        if !(self.size.is_finite() && self.size >= 0.0) {
            return Some("a text's size is metres of 0 or more");
        }
        if !self.line_spacing.is_finite() {
            return Some("a text's line spacing isn't finite");
        }
        if !(0.0..=1.0).contains(&self.outline) {
            return Some("a text's outline is 0 to 1");
        }
        if !(0.0..=1.0).contains(&self.softness) {
            return Some("a text's softness is 0 to 1");
        }
        if !(0.0..=1.0).contains(&self.shadow_softness) {
            return Some("a text's shadow softness is 0 to 1");
        }
        if !self.shadow_offset.is_finite() {
            return Some("a text's shadow offset isn't finite");
        }
        let finite = |c: Color| c.to_linear().iter().all(|v| v.is_finite());
        if !finite(self.color) || !finite(self.outline_color) || !finite(self.shadow_color) {
            return Some("a text's colours have a number that isn't finite");
        }
        None
    }

    fn style(&self) -> TextStyle {
        TextStyle { size: self.size, align: self.align, line_spacing: self.line_spacing }
    }
}

/// A text and the squares its letters are drawn on.
#[derive(Debug)]
pub struct TextRenderer {
    settings: TextSettings,
    text: String,
    quads: Vec<GlyphQuad>,
}

impl TextRenderer {
    pub fn settings(&self) -> &TextSettings {
        &self.settings
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its letters' squares, in metres in its object's XY plane, the first
    /// line on the X axis; spaces left out.
    pub fn quads(&self) -> &[GlyphQuad] {
        &self.quads
    }

    /// The box round its letters, in its object's space, or `None` with
    /// none.
    /// Whether it has a shadow: its colour shows.
    pub fn shadowed(&self) -> bool {
        self.settings.shadow_color.a > 0.0
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let first = self.quads.first()?;
        let mut r = first.rect;
        for q in &self.quads[1..] {
            r = [r[0].min(q.rect[0]), r[1].min(q.rect[1]), r[2].max(q.rect[2]), r[3].max(q.rect[3])];
        }
        Some((Vec3::new(r[0], r[1], 0.0), Vec3::new(r[2], r[3], 0.0)))
    }
}

/// A font the world keeps for flat text, its atlas, and the atlas's
/// pixels as last taken for the GPU.
#[derive(Debug)]
pub struct WorldFont {
    font: SdfFont,
    pixels: Arc<AtlasPixels>,
}

/// An atlas's pixels: one byte each, row by row from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasPixels {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// The atlas's version they were taken at.
    pub version: u64,
}

impl WorldFont {
    pub fn font(&self) -> &SdfFont {
        &self.font
    }

    /// Its atlas's pixels: taken again whenever a letter is drawn into it.
    pub fn pixels(&self) -> &Arc<AtlasPixels> {
        &self.pixels
    }

    fn take_pixels(&mut self) {
        let atlas = self.font.atlas();
        if self.pixels.version != atlas.version() {
            let (width, height) = atlas.size();
            let pixels = atlas.pixels().to_vec();
            self.pixels = Arc::new(AtlasPixels { width, height, pixels, version: atlas.version() });
        }
    }
}

impl World {
    /// The TrueType or OpenType font in `data`, kept for flat text, or
    /// why it isn't one.
    pub fn add_font(&mut self, data: Arc<[u8]>) -> Result<FontId> {
        let font = SdfFont::new(data).map_err(|_| WorldError::Font)?;
        let id = FontId::new(self.font_ids.next());
        let mut font = WorldFont {
            font,
            pixels: Arc::new(AtlasPixels { width: 0, height: 0, pixels: Vec::new(), version: u64::MAX }),
        };
        font.take_pixels();
        self.fonts.insert(id, font);
        Ok(id)
    }

    /// Frees `font`; refused while a text uses it.
    pub fn remove_font(&mut self, font: FontId) -> Result<()> {
        self.font(font)?;
        if let Some((&object, _)) = self.texts.iter().find(|(_, t)| t.settings.font == font) {
            return Err(WorldError::FontInUse { font, object });
        }
        self.fonts.remove(&font);
        Ok(())
    }

    pub fn font(&self, font: FontId) -> Result<&WorldFont> {
        self.fonts.get(&font).ok_or(WorldError::NoFont(font))
    }

    /// Every font, by its id, in no particular order.
    pub fn fonts(&self) -> impl Iterator<Item = (FontId, &WorldFont)> + '_ {
        self.fonts.iter().map(|(&id, f)| (id, f))
    }

    /// Gives `object` a text renderer with `settings` showing `text`, or
    /// changes the one it has. Refused for a missing object or font, or
    /// settings that can't be.
    pub fn set_text_renderer(&mut self, object: ObjectId, settings: TextSettings, text: &str) -> Result<()> {
        self.entity(object)?;
        self.font(settings.font)?;
        if let Some(why) = settings.refused() {
            return Err(WorldError::Text(why));
        }
        let renderer = self.texts.entry(object).or_insert_with(|| TextRenderer {
            settings: settings.clone(),
            text: String::new(),
            quads: Vec::new(),
        });
        renderer.settings = settings;
        renderer.text.clear();
        renderer.text.push_str(text);
        self.lay_out(object);
        Ok(())
    }

    /// Changes only what `object`'s text renderer shows. Allocates nothing
    /// once its text and squares have room, and its font knows every
    /// letter.
    pub fn set_text(&mut self, object: ObjectId, text: &str) -> Result<()> {
        self.entity(object)?;
        let renderer =
            self.texts.get_mut(&object).ok_or(WorldError::NoComponent { object, component: "TextRenderer" })?;
        if renderer.text != text {
            renderer.text.clear();
            renderer.text.push_str(text);
            self.lay_out(object);
        }
        Ok(())
    }

    fn lay_out(&mut self, object: ObjectId) {
        let renderer = self.texts.get_mut(&object).expect("laid out once it's there");
        let font = self.fonts.get_mut(&renderer.settings.font).expect("a text's font is the world's");
        font.font.quads(&renderer.text, &renderer.settings.style(), &mut renderer.quads);
        font.take_pixels();
    }

    pub fn remove_text_renderer(&mut self, object: ObjectId) -> Result<()> {
        self.entity(object)?;
        self.texts.remove(&object).ok_or(WorldError::NoComponent { object, component: "TextRenderer" })?;
        Ok(())
    }

    pub fn text_renderer(&self, object: ObjectId) -> Result<&TextRenderer> {
        self.entity(object)?;
        self.texts.get(&object).ok_or(WorldError::NoComponent { object, component: "TextRenderer" })
    }

    pub fn has_texts(&self) -> bool {
        !self.texts.is_empty()
    }

    /// Every text renderer, by its object, in no particular order.
    pub fn text_renderers(&self) -> impl Iterator<Item = (ObjectId, &TextRenderer)> + '_ {
        self.texts.iter().map(|(&id, t)| (id, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roboto() -> Arc<[u8]> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/assets/fonts/Roboto-Regular.ttf"))
            .unwrap()
            .into()
    }

    #[test]
    fn a_text_is_laid_out_when_its_set_and_its_font_atlas_taken_for_the_gpu() {
        let mut world = World::new();
        let font = world.add_font(roboto()).unwrap();
        let empty = world.font(font).unwrap().pixels().clone();
        let id = world.spawn();
        world.set_text_renderer(id, TextSettings::new(font, 0.5), "Hi there").unwrap();
        let text = world.text_renderer(id).unwrap();
        assert_eq!(text.text(), "Hi there");
        assert_eq!(text.quads().len(), 7, "a square a letter, not the space");
        let (lo, hi) = text.bounds().unwrap();
        assert!(lo.x < -0.5 && hi.x > 0.5 && (lo.x + hi.x).abs() < 0.05, "centred: {lo} {hi}");
        let pixels = world.font(font).unwrap().pixels().clone();
        assert!(pixels.version > empty.version && pixels.pixels.iter().any(|&p| p > 128), "letters drawn");
        assert_eq!(pixels.pixels.len(), (pixels.width * pixels.height) as usize);

        // The same letters again: the same pixels, not taken again.
        world.set_text(id, "there Hi").unwrap();
        assert!(Arc::ptr_eq(world.font(font).unwrap().pixels(), &pixels));
        assert_eq!(world.text_renderer(id).unwrap().quads().len(), 7);
    }

    #[test]
    fn a_font_in_use_stays_and_bad_settings_are_refused() {
        let mut world = World::new();
        assert!(matches!(world.add_font(b"no font".to_vec().into()), Err(WorldError::Font)));
        let font = world.add_font(roboto()).unwrap();
        let id = world.spawn();
        let bad = [
            TextSettings { size: -1.0, ..TextSettings::new(font, 1.0) },
            TextSettings { outline: 2.0, ..TextSettings::new(font, 1.0) },
            TextSettings { softness: f32::NAN, ..TextSettings::new(font, 1.0) },
        ];
        for settings in bad {
            assert!(matches!(world.set_text_renderer(id, settings, "x"), Err(WorldError::Text(_))));
        }
        assert!(world.text_renderer(id).is_err(), "refused settings give it none");
        assert!(world.set_text(id, "x").is_err(), "nothing to change");
        world.set_text_renderer(id, TextSettings::new(font, 1.0), "x").unwrap();
        assert!(matches!(world.remove_font(font), Err(WorldError::FontInUse { .. })));
        world.despawn(id).unwrap();
        assert_eq!(world.text_renderers().count(), 0);
        world.remove_font(font).unwrap();
        assert!(matches!(world.font(font), Err(WorldError::NoFont(_))));
    }
}
