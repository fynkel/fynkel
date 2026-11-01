//! Pictures in the scene (`docs/stage-15.md` §3.3): a [`SpriteRenderer`]
//! on an object draws a texture on a quad, as Unity's Sprite Renderer: its
//! colour multiplied in, flipped, from its pivot, cut out or see-through,
//! stretched by 9-slice without stretching its corners, and drawn in its
//! sorting layer and order before by its distance. A sprite is laid out
//! into its quads (one, or up to nine sliced) when it's set, not each
//! frame.

use glam::Vec2;

use crate::texts::Facing;
use crate::world::{Result, World, WorldError};
use crate::{Color, ObjectId, TextureId};

/// A sprite renderer's settings (Unity's).
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteSettings {
    pub texture: TextureId,
    /// Its width and height, in metres.
    pub size: Vec2,
    /// Multiplied into the texture's colours.
    pub color: Color,
    /// Mirrored across, or up and down, about its pivot.
    pub flip_x: bool,
    pub flip_y: bool,
    /// The point of it its object's origin is, 0 to 1 across and up from
    /// its bottom left: (0.5, 0.5) its middle, (0.5, 0) its bottom.
    pub pivot: Vec2,
    /// 9-slice: how far in from its left, bottom, right and top edges, in
    /// the texture's pixels, its corners and edges are. All 0, it's one
    /// quad, the texture stretched across it.
    pub slice: [f32; 4],
    /// How many of the texture's pixels make a metre, for a slice's
    /// corners: they keep that size however big the sprite is.
    pub pixels_per_metre: f32,
    /// 0: see-through by the texture's alpha. Above 0: cut out where the
    /// alpha is below it.
    pub alpha_cutoff: f32,
    pub facing: Facing,
    /// Drawn in order of its sorting layer, then its order in it, before by
    /// its distance: higher over lower.
    pub sorting_layer: i32,
    pub order: i32,
    /// Lit as the scene is; else shown as its texture and colour say.
    pub lit: bool,
}

impl SpriteSettings {
    /// A metre square, white, its pivot its middle, turned as its object
    /// is, unlit.
    pub fn new(texture: TextureId) -> Self {
        SpriteSettings {
            texture,
            size: Vec2::ONE,
            color: Color::WHITE,
            flip_x: false,
            flip_y: false,
            pivot: Vec2::splat(0.5),
            slice: [0.0; 4],
            pixels_per_metre: 100.0,
            alpha_cutoff: 0.0,
            facing: Facing::None,
            sorting_layer: 0,
            order: 0,
            lit: false,
        }
    }

    fn refused(&self) -> Option<&'static str> {
        if !(self.size.is_finite() && self.size.min_element() >= 0.0) {
            return Some("a sprite's size is metres of 0 or more");
        }
        if !self.pivot.is_finite() {
            return Some("a sprite's pivot isn't finite");
        }
        if !self.slice.iter().all(|s| s.is_finite() && *s >= 0.0) {
            return Some("a sprite's slice is pixels of 0 or more");
        }
        if !(self.pixels_per_metre.is_finite() && self.pixels_per_metre > 0.0) {
            return Some("a sprite's pixels per metre is more than 0");
        }
        if !(0.0..=1.0).contains(&self.alpha_cutoff) {
            return Some("a sprite's alpha cutoff is 0 to 1");
        }
        if !self.color.to_linear().iter().all(|v| v.is_finite()) {
            return Some("a sprite's colour has a number that isn't finite");
        }
        None
    }
}

/// One quad of a sprite: where it is in its object's XY plane, in metres
/// (left, bottom, right, top), and the part of the texture it shows
/// (left, top, right, bottom, 0 to 1, v down).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpriteQuad {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
}

/// A sprite and the quads it's drawn as.
#[derive(Debug)]
pub struct SpriteRenderer {
    settings: SpriteSettings,
    quads: [SpriteQuad; 9],
    count: u8,
}

impl SpriteRenderer {
    pub fn settings(&self) -> &SpriteSettings {
        &self.settings
    }

    /// Its quads: one, or up to nine when sliced.
    pub fn quads(&self) -> &[SpriteQuad] {
        &self.quads[..self.count as usize]
    }

    /// Lays out its quads for a texture of `width` × `height` pixels.
    fn lay_out(&mut self, width: u32, height: u32) {
        let s = &self.settings;
        let (w, h) = (s.size.x, s.size.y);
        let (tw, th) = (width.max(1) as f32, height.max(1) as f32);
        let [l, b, r, t] = s.slice;
        // The corners in metres, shrunk together when the sprite is too
        // small for them, as Unity's.
        let metres = |a: f32, c: f32, across: f32| {
            let (a, c) = (a / s.pixels_per_metre, c / s.pixels_per_metre);
            let fit = if a + c > across && a + c > 0.0 { across / (a + c) } else { 1.0 };
            (a * fit, c * fit)
        };
        let (ml, mr) = metres(l, r, w);
        let (mb, mt) = metres(b, t, h);
        let xs = [0.0, ml, w - mr, w];
        let us = [0.0, (l / tw).min(1.0), (1.0 - r / tw).max(0.0), 1.0];
        let ys = [0.0, mb, h - mt, h];
        // v runs down the texture: the sprite's bottom is its last row.
        let vs = [1.0, (1.0 - b / th).max(0.0), (t / th).min(1.0), 0.0];
        let (px, py) = (s.pivot.x * w, s.pivot.y * h);
        self.count = 0;
        for row in 0..3 {
            for column in 0..3 {
                let (x0, x1, y0, y1) = (xs[column], xs[column + 1], ys[row], ys[row + 1]);
                if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
                    continue;
                }
                let mut rect = [x0 - px, y0 - py, x1 - px, y1 - py];
                let mut uv = [us[column], vs[row + 1], us[column + 1], vs[row]];
                // Mirrored about the pivot, the texture with it.
                if s.flip_x {
                    rect = [-rect[2], rect[1], -rect[0], rect[3]];
                    uv = [uv[2], uv[1], uv[0], uv[3]];
                }
                if s.flip_y {
                    rect = [rect[0], -rect[3], rect[2], -rect[1]];
                    uv = [uv[0], uv[3], uv[2], uv[1]];
                }
                self.quads[self.count as usize] = SpriteQuad { rect, uv };
                self.count += 1;
            }
        }
    }
}

impl World {
    /// Gives `object` a sprite renderer with `settings`, or changes the one
    /// it has. Refused for a missing object or texture, or settings that
    /// can't be.
    pub fn set_sprite_renderer(&mut self, object: ObjectId, settings: SpriteSettings) -> Result<()> {
        self.entity(object)?;
        let image = self.texture(settings.texture)?;
        let (width, height) = (image.width(), image.height());
        if let Some(why) = settings.refused() {
            return Err(WorldError::Sprite(why));
        }
        let sprite = self.sprites.entry(object).or_insert_with(|| SpriteRenderer {
            settings: settings.clone(),
            quads: [SpriteQuad::default(); 9],
            count: 0,
        });
        sprite.settings = settings;
        sprite.lay_out(width, height);
        Ok(())
    }

    pub fn remove_sprite_renderer(&mut self, object: ObjectId) -> Result<()> {
        self.entity(object)?;
        self.sprites.remove(&object).ok_or(WorldError::NoComponent { object, component: "SpriteRenderer" })?;
        Ok(())
    }

    pub fn sprite_renderer(&self, object: ObjectId) -> Result<&SpriteRenderer> {
        self.entity(object)?;
        self.sprites.get(&object).ok_or(WorldError::NoComponent { object, component: "SpriteRenderer" })
    }

    pub fn has_sprites(&self) -> bool {
        !self.sprites.is_empty()
    }

    /// Every sprite renderer, by its object, in no particular order.
    pub fn sprite_renderers(&self) -> impl Iterator<Item = (ObjectId, &SpriteRenderer)> + '_ {
        self.sprites.iter().map(|(&id, s)| (id, s))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ImageData;

    fn world_with_texture(width: u32, height: u32) -> (World, TextureId) {
        let mut world = World::new();
        let pixels = vec![255; (width * height * 4) as usize];
        let texture = world.add_texture(Arc::new(ImageData::from_rgba(width, height, pixels).unwrap()));
        (world, texture)
    }

    #[test]
    fn a_sprite_is_one_quad_from_its_pivot_the_texture_across_it() {
        let (mut world, texture) = world_with_texture(64, 32);
        let id = world.spawn();
        let settings = SpriteSettings { size: Vec2::new(2.0, 1.0), ..SpriteSettings::new(texture) };
        world.set_sprite_renderer(id, settings.clone()).unwrap();
        let [q] = world.sprite_renderer(id).unwrap().quads() else { panic!() };
        assert_eq!(q.rect, [-1.0, -0.5, 1.0, 0.5]);
        assert_eq!(q.uv, [0.0, 0.0, 1.0, 1.0], "the whole texture, its top at the top");

        // Its pivot at its bottom middle: standing on its object's origin.
        let bottom = SpriteSettings { pivot: Vec2::new(0.5, 0.0), ..settings.clone() };
        world.set_sprite_renderer(id, bottom).unwrap();
        assert_eq!(world.sprite_renderer(id).unwrap().quads()[0].rect, [-1.0, 0.0, 1.0, 1.0]);

        // Flipped across: mirrored about the pivot, the texture with it.
        let flipped = SpriteSettings { flip_x: true, pivot: Vec2::new(0.0, 0.5), ..settings };
        world.set_sprite_renderer(id, flipped).unwrap();
        let [q] = world.sprite_renderer(id).unwrap().quads() else { panic!() };
        assert_eq!(q.rect, [-2.0, -0.5, 0.0, 0.5]);
        assert_eq!(q.uv, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_sliced_sprite_keeps_its_corners_however_big_it_is() {
        let (mut world, texture) = world_with_texture(100, 100);
        let id = world.spawn();
        // 20 px corners at 100 px a metre: 0.2 m, on a 3 × 1 m sprite.
        let settings = SpriteSettings {
            size: Vec2::new(3.0, 1.0),
            pivot: Vec2::ZERO,
            slice: [20.0, 20.0, 20.0, 20.0],
            ..SpriteSettings::new(texture)
        };
        world.set_sprite_renderer(id, settings.clone()).unwrap();
        let quads = world.sprite_renderer(id).unwrap().quads();
        assert_eq!(quads.len(), 9);
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
        assert!(close(quads[0].rect, [0.0, 0.0, 0.2, 0.2]), "bottom left corner: {:?}", quads[0].rect);
        assert!(close(quads[0].uv, [0.0, 0.8, 0.2, 1.0]), "{:?}", quads[0].uv);
        assert!(close(quads[4].rect, [0.2, 0.2, 2.8, 0.8]), "the middle stretches: {:?}", quads[4].rect);
        assert!(close(quads[4].uv, [0.2, 0.2, 0.8, 0.8]));
        assert!(close(quads[8].rect, [2.8, 0.8, 3.0, 1.0]), "top right corner");

        // Too small for its corners: they shrink together, the middle goes.
        let small = SpriteSettings { size: Vec2::new(0.2, 1.0), ..settings.clone() };
        world.set_sprite_renderer(id, small).unwrap();
        let quads = world.sprite_renderer(id).unwrap().quads();
        assert_eq!(quads.len(), 6, "no middle column");
        assert!(close(quads[0].rect, [0.0, 0.0, 0.1, 0.2]), "{:?}", quads[0].rect);

        // Only left and right: three quads, a bar.
        let bar = SpriteSettings { slice: [20.0, 0.0, 20.0, 0.0], ..settings };
        world.set_sprite_renderer(id, bar).unwrap();
        assert_eq!(world.sprite_renderer(id).unwrap().quads().len(), 3);
    }

    #[test]
    fn bad_settings_are_refused_and_a_texture_a_sprite_uses_stays() {
        let (mut world, texture) = world_with_texture(4, 4);
        let id = world.spawn();
        let bad = [
            SpriteSettings { size: Vec2::new(-1.0, 1.0), ..SpriteSettings::new(texture) },
            SpriteSettings { alpha_cutoff: 2.0, ..SpriteSettings::new(texture) },
            SpriteSettings { pixels_per_metre: 0.0, ..SpriteSettings::new(texture) },
            SpriteSettings { slice: [-1.0, 0.0, 0.0, 0.0], ..SpriteSettings::new(texture) },
        ];
        for settings in bad {
            assert!(matches!(world.set_sprite_renderer(id, settings), Err(WorldError::Sprite(_))));
        }
        assert!(world.sprite_renderer(id).is_err());
        world.set_sprite_renderer(id, SpriteSettings::new(texture)).unwrap();
        assert!(matches!(world.remove_texture(texture), Err(WorldError::SpriteTextureInUse { .. })));
        world.remove_sprite_renderer(id).unwrap();
        world.remove_texture(texture).unwrap();
        assert!(matches!(world.set_sprite_renderer(id, SpriteSettings::new(texture)), Err(WorldError::NoTexture(_))));
    }
}
