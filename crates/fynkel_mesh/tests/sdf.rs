//! Flat text (stage 15 phase 2): each letter drawn once into an atlas as
//! its distance from the outline, laid out as solid text is.

use fynkel_mesh::*;

fn roboto() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/assets/fonts/Roboto-Regular.ttf")).unwrap()
}

/// The atlas's distance at a point of a letter's square, (0, 0) its
/// top-left corner and (1, 1) its bottom-right.
fn at(atlas: &SdfAtlas, g: &AtlasGlyph, u: f32, v: f32) -> u8 {
    let [x, y, w, h] = g.rect.map(u32::from);
    let px = x + ((u * w as f32) as u32).min(w - 1);
    let py = y + ((v * h as f32) as u32).min(h - 1);
    atlas.pixels()[(py * atlas.size().0 + px) as usize]
}

/// The atlas's point for a point of the font's em, (0, 0) the pen's.
fn at_em(atlas: &SdfAtlas, g: &AtlasGlyph, x: f32, y: f32) -> u8 {
    let [left, bottom, right, top] = g.bounds;
    at(atlas, g, (x - left) / (right - left), (top - y) / (top - bottom))
}

#[test]
fn a_letter_is_inside_on_its_strokes_and_outside_in_its_hole() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let mut atlas = SdfAtlas::new();
    let o = font.glyph_id('O').unwrap();
    let g = atlas.glyph(&font, o).expect("an O has an outline");
    // An O is a ring: the middle is its hole, outside past its padding.
    assert!(at(&atlas, &g, 0.5, 0.5) < 100, "the hole is outside: {}", at(&atlas, &g, 0.5, 0.5));
    assert!(at(&atlas, &g, 0.0, 0.0) < 20, "the corner is far outside");
    // Its left stroke, a little in from its left edge, half way up.
    let [left, bottom, right, top] = g.bounds;
    let (x, y) = (left + (right - left) * 0.5, (bottom + top) * 0.5);
    let stroke = (0..100).map(|k| at_em(&atlas, &g, left + (x - left) * k as f32 / 100.0, y)).max().unwrap();
    assert!(stroke > 160, "the stroke is inside: {stroke}");
}

#[test]
fn the_edge_is_half_way() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let mut atlas = SdfAtlas::new();
    let l = font.glyph_id('l').unwrap();
    let g = atlas.glyph(&font, l).unwrap();
    // Across the middle of an l's stem: up from outside past 128, then down.
    let [left, bottom, right, top] = g.bounds;
    let y = (bottom + top) * 0.5;
    let row: Vec<u8> = (0..=200).map(|k| at_em(&atlas, &g, left + (right - left) * k as f32 / 200.0, y)).collect();
    let crossings = row.windows(2).filter(|w| (w[0] < 128) != (w[1] < 128)).count();
    assert_eq!(crossings, 2, "in once and out once: {row:?}");
    // And it falls off smoothly: no step bigger than the spread allows.
    let biggest = row.windows(2).map(|w| (w[0] as i32 - w[1] as i32).abs()).max().unwrap();
    assert!(biggest < 40, "smooth: {biggest}");
}

#[test]
fn each_letter_is_drawn_once() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let mut atlas = SdfAtlas::new();
    let a = font.glyph_id('a').unwrap();
    let first = atlas.glyph(&font, a).unwrap();
    let version = atlas.version();
    let again = atlas.glyph(&font, a).unwrap();
    assert_eq!(first, again);
    assert_eq!(atlas.version(), version, "nothing new drawn");
    let b = atlas.glyph(&font, font.glyph_id('b').unwrap()).unwrap();
    assert!(atlas.version() > version, "a new letter changes it");
    let overlap =
        |p: [u16; 4], q: [u16; 4]| p[0] < q[0] + q[2] && q[0] < p[0] + p[2] && p[1] < q[1] + q[3] && q[1] < p[1] + p[3];
    assert!(!overlap(first.rect, b.rect), "letters don't overlap");
}

#[test]
fn a_space_has_nothing_to_draw() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let mut atlas = SdfAtlas::new();
    let version = atlas.version();
    assert_eq!(atlas.glyph(&font, font.glyph_id(' ').unwrap()), None);
    assert_eq!(atlas.version(), version);
}

#[test]
fn the_atlas_grows_as_letters_are_added_and_keeps_those_it_has() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let mut atlas = SdfAtlas::new();
    let start = atlas.size();
    let a = atlas.glyph(&font, font.glyph_id('a').unwrap()).unwrap();
    let before: Vec<u8> = (0..20).map(|k| at(&atlas, &a, k as f32 / 20.0, 0.5)).collect();
    let mut drawn = 0;
    for c in (0x21u8..0x7f).map(char::from).chain("àáâãäåæçèéêëìíîïñòóôõöøùúûüýÿÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÑÒÓÔÕÖØÙÚÛÜÝ".chars())
    {
        drawn += atlas.glyph(&font, font.glyph_id(c).unwrap()).is_some() as usize;
    }
    assert!(drawn > 150);
    assert!(atlas.size().0 * atlas.size().1 > start.0 * start.1, "it grew: {:?}", atlas.size());
    assert_eq!(atlas.glyph(&font, font.glyph_id('a').unwrap()), Some(a), "a stays where it was");
    let after: Vec<u8> = (0..20).map(|k| at(&atlas, &a, k as f32 / 20.0, 0.5)).collect();
    assert_eq!(before, after, "and as it was");
}

#[test]
fn laid_out_letters_are_where_solid_text_puts_them() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let style = TextStyle { size: 2.0, align: TextAlign::Center, line_spacing: 1.5 };
    let mut placed = Vec::new();
    font.layout("TA\nlo", &style, &mut placed);
    assert_eq!(placed.len(), 4);
    let widths: Vec<f32> = ["TA", "lo"].iter().map(|l| font.measure(l, &style).x).collect();
    // Centred: each line's first letter starts at minus half its width.
    assert!((placed[0].x + widths[0] * 0.5).abs() < 1e-4);
    assert!((placed[2].x + widths[1] * 0.5).abs() < 1e-4);
    // The second line one line height times the spacing below.
    let step = font.line_height() * 1.5 * 2.0;
    assert_eq!(placed[0].y, 0.0);
    assert!((placed[2].y + step).abs() < 1e-4);
    // TA kerned closer than their advances.
    let advance = font.advance_of(placed[0].glyph) * 2.0;
    assert!(placed[1].x - placed[0].x < advance, "kerned");
}

#[test]
fn a_flat_font_lays_letters_out_where_solid_text_does() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let mut flat = SdfFont::new(data.clone().into()).unwrap();
    let style = TextStyle { size: 0.5, align: TextAlign::Right, line_spacing: 1.2 };
    let text = "Tab, -12\nVAT  ok";
    let mut placed = Vec::new();
    font.layout(text, &style, &mut placed);
    let mut quads = Vec::new();
    flat.quads(text, &style, &mut quads);
    // One square a letter with something to draw (no spaces).
    let drawn: Vec<&PlacedGlyph> = placed.iter().filter(|p| font.has_outline(p.glyph)).collect();
    assert_eq!(quads.len(), drawn.len());
    for (q, p) in quads.iter().zip(drawn) {
        let g = flat.atlas().glyph_known(p.glyph).expect("drawn into the atlas");
        let [l, b, r, t] = g.bounds.map(|v| v * style.size);
        let want = [p.x + l, p.y + b, p.x + r, p.y + t];
        for k in 0..4 {
            assert!((q.rect[k] - want[k]).abs() < 1e-5, "{:?} vs {want:?}", q.rect);
        }
        assert_eq!(q.pixels, g.rect);
    }
    assert_eq!(flat.line_height(), font.line_height());
}

#[test]
fn a_flat_font_reads_its_file_only_for_letters_it_hasnt_met() {
    let data = roboto();
    let mut flat = SdfFont::new(data.into()).unwrap();
    let style = TextStyle::default();
    let mut quads = Vec::new();
    flat.quads("-120", &style, &mut quads);
    let read = flat.reads();
    flat.quads("-120", &style, &mut quads);
    flat.quads("12\n0", &style, &mut quads);
    assert_eq!(flat.reads(), read, "the same letters and pairs: nothing read");
    flat.quads("x", &style, &mut quads);
    assert_eq!(flat.reads(), read + 1);
}

#[test]
fn a_flat_font_refuses_bytes_that_arent_a_font() {
    assert!(SdfFont::new(b"nope".to_vec().into()).is_err());
}

/// 200 numbers laid out again, as a game changing them every frame would,
/// held to a bar (the Mac's time twice): `cargo
/// test --release -p fynkel_mesh -- --ignored bench_`.
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_flat_numbers() {
    let mut font = SdfFont::new(roboto().into()).unwrap();
    let style = TextStyle::default();
    let numbers: Vec<String> = (0..200).map(|k| format!("-{}", k * 37 % 1000)).collect();
    let mut quads = Vec::new();
    for n in &numbers {
        font.quads(n, &style, &mut quads);
    }
    fynkel_bench::bench("200 numbers laid out", 200, 0.1, || {
        for n in &numbers {
            font.quads(n, &style, &mut quads);
            std::hint::black_box(&quads);
        }
    });
}
