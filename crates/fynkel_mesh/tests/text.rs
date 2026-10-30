//! Text made solid: letters from a font, with their holes, laid out in
//! lines and aligned, each a closed slab as deep as asked.

mod common;

use common::*;
use fynkel_mesh::*;
use glam::Vec3;

fn roboto() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/assets/fonts/Roboto-Regular.ttf")).unwrap()
}

fn style(size: f32, align: TextAlign) -> TextStyle {
    TextStyle { size, align, line_spacing: 1.0 }
}

fn bounds(text: &MeshData) -> (Vec3, Vec3) {
    text.bounds()
}

#[test]
fn letters_have_the_holes_their_font_gives_them() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let pieces = |t: &str| {
        let shapes = font.shapes(t, &TextStyle::default());
        let mut holes: Vec<usize> = shapes.iter().map(|s| s.holes.len()).collect();
        holes.sort();
        holes
    };
    assert_eq!(pieces("o"), [1], "an o is one piece with a hole");
    assert_eq!(pieces("B"), [2], "a B has two");
    assert_eq!(pieces("i"), [0, 0], "an i is a stem and a dot");
    assert_eq!(pieces("%"), [0, 1, 1], "a % is two rings and a stroke");
    assert_eq!(pieces("l"), [0]);
}

#[test]
fn text_is_closed_and_as_deep_as_asked() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let text = MeshData::text(&font, "Bog%i", &TextStyle::default(), 0.3, 1.0).unwrap();
    check_winding(&text);
    check_tangents(&text);
    check_closed(&text);
    let (lo, hi) = bounds(&text);
    assert!(close(lo.z, -0.15, 1e-5) && close(hi.z, 0.15, 1e-5), "z from {} to {}", lo.z, hi.z);
    assert!(volume(&text) > 0.0);
}

#[test]
fn an_l_is_a_solid_bar_and_an_o_less_than_its_box() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let solid = |t: &str| {
        let m = MeshData::text(&font, t, &TextStyle::default(), 0.5, 1.0).unwrap();
        let (lo, hi) = bounds(&m);
        let size = hi - lo;
        (volume(&m), size.x * size.y * size.z)
    };
    let (l, l_box) = solid("l");
    assert!(close(l, l_box, 1e-3), "the l, a rectangle, fills its box: {l} of {l_box}");
    let (o, o_box) = solid("o");
    assert!(o > 0.2 * o_box && o < 0.6 * o_box, "the o's hole is cut out: {o} of {o_box}");
}

#[test]
fn letters_stand_on_the_origin_and_are_as_big_as_asked() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let small = MeshData::text(&font, "x", &style(1.0, TextAlign::Left), 0.1, 1.0).unwrap();
    let big = MeshData::text(&font, "x", &style(2.0, TextAlign::Left), 0.1, 1.0).unwrap();
    let ((lo, hi), (big_lo, big_hi)) = (bounds(&small), bounds(&big));
    assert!(lo.y.abs() < 1e-4, "an x sits on the line: {}", lo.y);
    assert!(hi.y > 0.4 && hi.y < 0.65, "and is about half an em tall: {}", hi.y);
    assert!((big_lo.truncate() - 2.0 * lo.truncate()).length() < 1e-4, "{big_lo} vs {lo}");
    assert!((big_hi.truncate() - 2.0 * hi.truncate()).length() < 1e-4, "{big_hi} vs {hi}");
}

#[test]
fn lines_are_aligned_on_the_origin() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let width = font.measure("HOH", &TextStyle::default()).x;
    assert!(width > 1.5 && width < 3.0, "three capitals are about 2 em: {width}");
    let at = |align| bounds(&MeshData::text(&font, "HOH", &style(1.0, align), 0.1, 1.0).unwrap()).0.x;
    let (left, center, right) = (at(TextAlign::Left), at(TextAlign::Center), at(TextAlign::Right));
    assert!((0.0..0.15).contains(&left), "left starts at the origin, less the H's own margin: {left}");
    assert!(close(center, left - width / 2.0, 1e-5), "{center} vs {left}");
    assert!(close(right, left - width, 1e-5), "{right} vs {left}");
}

#[test]
fn each_line_is_a_line_height_below_the_last() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let height = font.line_height();
    assert!(height > 1.0 && height < 1.5, "a line is a little more than an em: {height}");
    let foot = |text: &str, spacing| {
        let style = TextStyle { size: 2.0, align: TextAlign::Left, line_spacing: spacing };
        bounds(&MeshData::text(&font, text, &style, 0.1, 1.0).unwrap()).0.y
    };
    assert!(close(foot("x\nx", 1.0), -2.0 * height, 1e-5));
    assert!(close(foot("x\nx", 1.5), -3.0 * height, 1e-5));
    assert!(close(foot("x\n\nx", 1.0), -4.0 * height, 1e-5), "an empty line counts");
    let size = font.measure("xx\nx", &style(2.0, TextAlign::Left));
    assert!(close(size.y, 4.0 * height, 1e-5));
    assert!(close(size.x, font.measure("xx", &style(2.0, TextAlign::Left)).x, 1e-6), "the widest line");
}

#[test]
fn spaces_make_an_empty_mesh_and_take_room() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let empty = MeshData::text(&font, "   ", &TextStyle::default(), 0.1, 1.0).unwrap();
    assert!(empty.indices().is_empty());
    let one = font.measure("x", &TextStyle::default()).x;
    assert!(font.measure("x x", &TextStyle::default()).x > 2.0 * one);
    // A letter the font hasn't got (here, Hiragana) doesn't fail.
    MeshData::text(&font, "aあb", &TextStyle::default(), 0.1, 1.0).unwrap();
}

#[test]
fn a_file_that_isnt_a_font_says_so() {
    let err = Font::parse(b"not a font at all").expect_err("refused");
    assert!(err.to_string().contains("font"), "{err}");
}

#[test]
fn curves_are_smooth_at_any_size() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let points = |size| font.shapes("o", &style(size, TextAlign::Left))[0].outline.len();
    assert_eq!(points(0.1), points(10.0), "edges per curve depend on the letter, not its size");
    assert!(points(1.0) >= 24, "an o is round: {} points", points(1.0));
    // No two points closer than the tolerance allows to matter.
    let shape = &font.shapes("o", &style(1.0, TextAlign::Left))[0].outline;
    let shortest = (0..shape.len()).map(|i| shape[i].distance(shape[(i + 1) % shape.len()])).fold(f32::MAX, f32::min);
    assert!(shortest > 0.0);
}

#[test]
fn letters_the_font_kerns_go_closer() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let w = |t: &str| font.measure(t, &TextStyle::default()).x;
    assert!(w("LT") < w("L") + w("T") - 0.05, "LT {} vs L + T {}", w("LT"), w("L") + w("T"));
    assert!(w("TA") < w("T") + w("A") - 0.03, "TA {} vs T + A {}", w("TA"), w("T") + w("A"));
    assert!(close(w("HH"), 2.0 * w("H"), 1e-6), "H and H aren't kerned");
}

/// What making text takes, held to a bar (stage 14's GT1): `cargo test
/// --release -p fynkel_mesh -- --ignored bench_`.
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_making_text() {
    let data = roboto();
    let font = Font::parse(&data).unwrap();
    let (text, style) = ("Hello, world!\nScore: 12,345", TextStyle::default());
    let triangles = MeshData::text(&font, text, &style, 0.2, 1.0).unwrap().indices().len() / 3;
    fynkel_bench::bench(&format!("26 letters in two lines, {triangles} triangles"), 50, 0.9, || {
        drop(std::hint::black_box(MeshData::text(&font, text, &style, 0.2, 1.0).unwrap()))
    });
}
