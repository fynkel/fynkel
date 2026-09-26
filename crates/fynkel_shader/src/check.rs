//! Compiling a `.shader` with naga, every error at its place in the file.
//!
//! The assembled module is the [`prelude`](crate::contract::prelude), then
//! the file itself with its header blanked to spaces (so every body line
//! and column is where it is in the file), then — for the check only —
//! two small entry points that call `surface` and `vertex` the way the
//! renderer's do, so a stage mistake (sampling a texture in `vertex`) is
//! caught here and not at draw time.

use std::error::Error;

use fynkel_contracts::{Code, Diagnostic};
use naga::{Module, Span};

use crate::contract::prelude;
use crate::{header, Shader};

/// A shader's WGSL as one module.
#[derive(Clone, PartialEq, Debug)]
pub struct Assembled {
    pub source: String,
    /// Where the file starts in `source` (bytes); the prelude is before it.
    pub file_start: usize,
    /// Where the file ends in `source` (bytes); what follows is the
    /// caller's (the renderer's entry points).
    pub file_end: usize,
}

/// The prelude, then the file with its header blanked, then `tail`.
pub fn assemble(shader: &Shader, tail: &str) -> Assembled {
    let mut source = prelude(&shader.interface, &shader.layout);
    let file_start = source.len();
    source.extend(
        shader.text[..shader.body_start]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' }),
    );
    source.push_str(shader.body());
    if !source.ends_with('\n') {
        source.push('\n');
    }
    let file_end = source.len();
    source.push_str(tail);
    Assembled {
        source,
        file_start,
        file_end,
    }
}

/// Reads the header and compiles the body. `Ok` is a shader the renderer
/// can build; `Err` holds every error found (the header's first — the
/// body is only compiled when the header reads).
pub fn check(path: Option<&str>, text: &str) -> Result<Shader, Vec<Diagnostic>> {
    let shader = header::parse(path, text)?;
    let doc = |d: Diagnostic| match path {
        Some(p) => d.with_document(p),
        None => d,
    };

    // The body on its own first: a parse error, then the signatures.
    let bare = assemble(&shader, "");
    let module = match naga::front::wgsl::parse_str(&bare.source) {
        Ok(m) => m,
        Err(e) => {
            let span = e.labels().next().map(|(s, _)| s);
            let mut message = e.message().to_string();
            if let Some((_, label)) = e.labels().next() {
                if !label.is_empty() && !message.contains(label) {
                    message = format!("{message}: {label}");
                }
            }
            return Err(vec![doc(place(
                Diagnostic::error(Code::SHADER_COMPILE, message),
                &bare,
                text,
                span,
            ))]);
        }
    };
    let has_vertex = signatures(&shader, &module, &bare, text).map_err(|d| vec![doc(d)])?;

    // Then the whole with the check's entry points, validated.
    let checked = assemble(&shader, &check_entries(has_vertex));
    let module = match naga::front::wgsl::parse_str(&checked.source) {
        Ok(m) => m,
        Err(e) => {
            let span = e.labels().next().map(|(s, _)| s);
            return Err(vec![doc(place(
                Diagnostic::error(Code::SHADER_COMPILE, e.message().to_string()),
                &checked,
                text,
                span,
            ))]);
        }
    };
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    );
    if let Err(e) = validator.validate(&module) {
        let message = validation_message(e.as_inner());
        // The innermost (shortest) span that lies in the file: naga puts
        // the function's own span first.
        let span = e
            .spans()
            .map(|(s, _)| *s)
            .filter(|s| in_file(&checked, *s))
            .min_by_key(|s| s.to_range().map_or(usize::MAX, |r| r.len()))
            .or_else(|| e.spans().next().map(|(s, _)| *s));
        return Err(vec![doc(place(
            Diagnostic::error(Code::SHADER_COMPILE, message),
            &checked,
            text,
            span,
        ))]);
    }
    Ok(shader)
}

/// `surface(in: SurfaceInput) -> Surface` must be there; `vertex`, when
/// there, is `vertex(in: VertexInput) -> VertexInput`. `Ok(true)` when
/// the shader has a `vertex`.
fn signatures(
    shader: &Shader,
    module: &Module,
    a: &Assembled,
    text: &str,
) -> Result<bool, Diagnostic> {
    let type_name = |h: naga::Handle<naga::Type>| module.types[h].name.clone().unwrap_or_default();
    let find = |name: &str| {
        module
            .functions
            .iter()
            .find(|(_, f)| f.name.as_deref() == Some(name))
    };
    let wants = |name: &str, arg: &str, ret: &str| -> Result<bool, Diagnostic> {
        let Some((h, f)) = find(name) else {
            return Ok(false);
        };
        let ok = f.arguments.len() == 1
            && type_name(f.arguments[0].ty) == arg
            && f.result.as_ref().is_some_and(|r| type_name(r.ty) == ret);
        if ok {
            return Ok(true);
        }
        let span = module.functions.get_span(h);
        Err(place(
            Diagnostic::error(
                Code::SHADER_ENTRY,
                format!("`{name}` must be `fn {name}(in: {arg}) -> {ret}`"),
            ),
            a,
            text,
            Some(span),
        ))
    };
    if !wants("surface", "SurfaceInput", "Surface")? {
        let line = line_col(text, shader.body_start).0;
        return Err(Diagnostic::error(Code::SHADER_ENTRY, "the shader has no `surface` function")
            .at(line, 1)
            .with_suggestion(
                "fn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    return s;\n}",
            ));
    }
    wants("vertex", "VertexInput", "VertexInput")
}

/// The check's entry points: `surface` in a fragment stage, `vertex` in a
/// vertex stage, the properties loaded as the renderer's entries do.
fn check_entries(has_vertex: bool) -> String {
    let vertex = if has_vertex {
        "    var v: VertexInput;\n    let o = vertex(v);\n    return vec4<f32>(o.position, 1.0);\n"
    } else {
        "    return vec4<f32>(0.0);\n"
    };
    format!(
        "\n@fragment\nfn fynkel_check_fragment() -> @location(0) vec4<f32> {{\n    material = fynkel_load_material();\n    \
         var i: SurfaceInput;\n    let s = surface(i);\n    return vec4<f32>(s.base_color + s.emission, s.alpha);\n}}\n\n\
         @vertex\nfn fynkel_check_vertex() -> @builtin(position) vec4<f32> {{\n    material = fynkel_load_material();\n{vertex}}}\n"
    )
}

/// naga's error chain as one line: "Function [3] 'surface' is invalid"
/// becomes "in `surface`", then each cause.
fn validation_message(e: &(dyn Error + 'static)) -> String {
    let mut parts = Vec::new();
    let mut next: Option<&(dyn Error + 'static)> = Some(e);
    while let Some(err) = next {
        let part = err.to_string();
        let part = match (part.find('\''), part.rfind("' is invalid")) {
            (Some(a), Some(b))
                if part.starts_with("Function [") || part.starts_with("Entry point") =>
            {
                format!("in `{}`", &part[a + 1..b])
            }
            _ => part,
        };
        if !parts.iter().any(|p: &String| p.contains(&part)) {
            parts.push(part);
        }
        next = err.source();
    }
    parts.join(": ")
}

fn in_file(a: &Assembled, span: Span) -> bool {
    span.to_range()
        .is_some_and(|r| r.start >= a.file_start && r.start < a.file_end)
}

/// Puts `d` at `span`'s line and column in the `.shader` file. A span in
/// the engine's code (the prelude, the entry points) has no place in the
/// file; the message says so.
fn place(d: Diagnostic, a: &Assembled, text: &str, span: Option<Span>) -> Diagnostic {
    match span.and_then(|s| s.to_range()) {
        Some(r) if r.start >= a.file_start && r.start < a.file_end => {
            let (line, column) = line_col(text, r.start - a.file_start);
            d.at(line, column)
        }
        Some(_) => {
            let message = format!("{} (in the engine's code around the body)", d.message);
            Diagnostic { message, ..d }
        }
        None => d,
    }
}

/// 1-based line and column (in characters) of byte `at` in `text`.
fn line_col(text: &str, at: usize) -> (u32, u32) {
    let at = at.min(text.len());
    let before = &text[..at];
    let line = before.matches('\n').count() as u32 + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = before[start..].chars().count() as u32 + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "shader \"Test\" {\n    properties {\n        tint color = (1, 0.5, 0.2, 1)\n        speed float = 1 range(0, 10)\n        on bool = true\n        steps int = 2\n        ripples texture = normal\n    }\n}\n";

    fn with_body(body: &str) -> String {
        format!("{HEADER}{body}")
    }

    #[test]
    fn a_good_shader_compiles() {
        let text = with_body(
            "\n// Uses everything the contract offers.\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    let n = sample_ripples(in.uv0 + globals.time * material.speed);\n    s.base_color = material.tint.rgb * in.color.rgb;\n    s.normal = n.xyz * 2.0 - 1.0;\n    if material.on { s.emission = vec3<f32>(f32(material.steps)); }\n    s.alpha = material.tint.a;\n    return s;\n}\n\nfn vertex(in: VertexInput) -> VertexInput {\n    var v = in;\n    v.position.y += sin(globals.time + in.position.x) * 0.1;\n    return v;\n}\n",
        );
        let shader = check(Some("t.shader"), &text).unwrap_or_else(|e| panic!("{e:#?}"));
        assert_eq!(shader.interface.properties.len(), 5);
    }

    fn one_error(text: &str) -> Diagnostic {
        let mut e = check(Some("t.shader"), text).expect_err("fails");
        assert_eq!(e.len(), 1, "{e:#?}");
        e.remove(0)
    }

    #[test]
    fn a_wgsl_mistake_is_at_its_line_in_the_file() {
        // Line 12 is `    s.base_colour = …` (the header is 9 lines, then a blank).
        let text = with_body(
            "\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.base_colour = vec3<f32>(1.0);\n    return s;\n}\n",
        );
        let d = one_error(&text);
        assert_eq!(d.code.as_str(), "FYN-D0313");
        assert_eq!(d.document.as_deref(), Some("t.shader"));
        assert_eq!(d.line, Some(13), "{d:#?}");
        assert!(d.message.contains("base_colour"), "{}", d.message);
    }

    #[test]
    fn a_syntax_error_is_at_its_line_and_column() {
        let text = with_body("\nfn surface(in: SurfaceInput) -> Surface {\n    let x = ;\n    return default_surface();\n}\n");
        let d = one_error(&text);
        assert_eq!(d.code.as_str(), "FYN-D0313");
        assert_eq!((d.line, d.column), (Some(12), Some(13)), "{d:#?}");
    }

    #[test]
    fn a_type_mistake_is_found_by_validation() {
        let text = with_body(
            "\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.roughness = material.tint;\n    return s;\n}\n",
        );
        let d = one_error(&text);
        assert_eq!(d.code.as_str(), "FYN-D0313");
        assert_eq!(d.line, Some(13), "{d:#?}");
        assert!(d.message.starts_with("in `surface`: "), "{}", d.message);
    }

    #[test]
    fn a_missing_surface_says_how_to_write_it() {
        let d = one_error(&with_body("\nfn helper() -> f32 { return 1.0; }\n"));
        assert_eq!(d.code.as_str(), "FYN-D0314");
        assert!(d.message.contains("no `surface`"));
        assert!(d
            .suggestion
            .as_deref()
            .unwrap()
            .contains("fn surface(in: SurfaceInput) -> Surface"));
    }

    #[test]
    fn a_wrong_signature_is_at_the_function() {
        let text = with_body(
            "\nfn surface(uv: vec2<f32>) -> vec4<f32> {\n    return vec4<f32>(uv, 0.0, 1.0);\n}\n",
        );
        let d = one_error(&text);
        assert_eq!(d.code.as_str(), "FYN-D0314");
        assert_eq!(d.line, Some(11), "{d:#?}");
        assert!(d
            .message
            .contains("fn surface(in: SurfaceInput) -> Surface"));

        let text = with_body(
            "\nfn surface(in: SurfaceInput) -> Surface { return default_surface(); }\nfn vertex(p: vec3<f32>) -> vec3<f32> { return p; }\n",
        );
        let d = one_error(&text);
        assert_eq!(d.code.as_str(), "FYN-D0314");
        assert!(d
            .message
            .contains("fn vertex(in: VertexInput) -> VertexInput"));
    }

    #[test]
    fn sampling_a_texture_in_vertex_is_a_stage_error() {
        let text = with_body(
            "\nfn surface(in: SurfaceInput) -> Surface { return default_surface(); }\n\nfn vertex(in: VertexInput) -> VertexInput {\n    var v = in;\n    v.position.y += sample_ripples(in.uv0).r;\n    return v;\n}\n",
        );
        let d = one_error(&text);
        assert_eq!(d.code.as_str(), "FYN-D0313");
    }

    #[test]
    fn header_errors_stop_before_the_body() {
        let e = check(None, "shader \"A\" {\n  lighting shiny\n}\nfn surface(in: SurfaceInput) -> Surface { nonsense }\n")
            .expect_err("fails");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].code.as_str(), "FYN-D0315");
    }

    #[test]
    fn line_and_column_count_characters() {
        assert_eq!(line_col("ab\ncé\nx", 0), (1, 1));
        assert_eq!(line_col("ab\ncé\nx", 3), (2, 1));
        assert_eq!(line_col("ab\ncé\nx", 7), (3, 1));
        assert_eq!(line_col("ab\ncéd", 6), (2, 3));
    }
}
