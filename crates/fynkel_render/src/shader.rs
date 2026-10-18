//! A material's own shader (stage 10): a game's WGSL with a `surface` (or
//! an `unlit`) function, and perhaps a `vertex` that moves its vertices,
//! joined to the engine's lighting and checked with
//! naga as it's made, so a mistake is found as the game loads it and named
//! by its line. Or a screen effect's: an `effect` function, run over the
//! finished picture (effect.wgsl).
//!
//! What a shader's file may have, besides its functions:
//! - `struct Values { … }`: the material's values, read as `values.name`.
//!   Numbers and vectors of `f32`, `i32` and `u32`, set from the game by
//!   name.
//! - `struct PerObject { … }`: up to four `f32`s each object sets on its
//!   own, read as `per_object.name`.
//! - Textures, as `var name: texture_2d<f32>;` (read as data, linear) or
//!   `var name: color_texture;` (an sRGB image: colours), up to
//!   [`MAX_TEXTURES`]. The engine gives them their bindings.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use naga::{ScalarKind, TypeInner, VectorSize};

use crate::ColorSpace;

/// The most textures a shader may have of its own.
pub const MAX_TEXTURES: usize = 4;
/// The most `PerObject` numbers.
pub const MAX_OWN: usize = 4;
/// The most bytes of `Values`.
pub const MAX_VALUES_BYTES: u32 = 512;
/// The binding of a shader's first texture in group 1, after the
/// material's five and its sampler.
pub(crate) const FIRST_TEXTURE: u32 = 6;

/// How a shader's surface is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lighting {
    /// `fn surface(in: SurfaceInput) -> Surface`: lit, shadowed and fogged
    /// like the built-in material.
    Lit,
    /// `fn unlit(in: SurfaceInput) -> vec4<f32>`: its colour as it is,
    /// fogged.
    Unlit,
}

/// What a value is: a number or a vector of 2 to 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueKind {
    F32,
    I32,
    U32,
}

/// One of a shader's `Values`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShaderValue {
    pub name: String,
    pub kind: ValueKind,
    /// 1 for a number, 2 to 4 for a vector.
    pub size: u32,
    /// Where it is in the values' bytes.
    pub offset: u32,
}

/// One of a shader's textures.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShaderTexture {
    pub name: String,
    pub space: ColorSpace,
}

/// A shader, checked and joined to the engine's code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomShader {
    /// The file's name, for messages.
    pub name: String,
    pub lighting: Lighting,
    pub values: Vec<ShaderValue>,
    /// How many bytes the values take.
    pub values_size: u32,
    pub textures: Vec<ShaderTexture>,
    /// The `PerObject` numbers' names, in order.
    pub own: Vec<String>,
    /// Whether it has `fn vertex(in: VertexInput) -> VertexInput`, which
    /// moves its vertices: its shadows and ambient occlusion's depth are
    /// then drawn with it too.
    pub moves: bool,
    /// Whether it reads what's behind it (`scene_depth`, `scene_color`):
    /// its surfaces are then drawn see-through, after the solid scene,
    /// and a frame they're in keeps that scene's depth and colour.
    pub reads_behind: bool,
    /// Whether it has `fn light(in: LightInput) -> vec3<f32>`, which lights
    /// it in place of the engine's lighting, light by light.
    pub lights: bool,
    /// Whether it's a screen effect's, `fn effect(in: EffectInput) ->
    /// vec4<f32>`, for a camera ([`crate::ScreenEffect`]) rather than a
    /// material. Its lighting is then [`Lighting::Unlit`], and it has no
    /// textures or `PerObject`.
    pub effect: bool,
    /// The whole module, the engine's code and the shader's.
    pub(crate) wgsl: Arc<str>,
    /// Of `wgsl`: pipelines made from the same code are shared.
    pub(crate) hash: u64,
}

/// Why a shader can't be used: where in its file, and what's wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderError {
    pub file: String,
    /// 1-based, in the shader's own file; `None` when it isn't in it.
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(f, "{}:{line}:{column}: {}", self.file, self.message),
            (Some(line), None) => write!(f, "{}:{line}: {}", self.file, self.message),
            _ => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

impl std::error::Error for ShaderError {}

/// The engine's code after a shader's glue: its shadow, drawn with its own
/// alpha where it's cut out, so what it cuts away casts none.
const SHADOW: &str = include_str!("own_shadow.wgsl");

/// The engine's code before a shader's: its lighting and the library.
const BEFORE: &str = concat!(include_str!("skin.wgsl"), include_str!("forward.wgsl"), include_str!("library.wgsl"));

/// The engine's code before a screen effect's.
const EFFECT: &str = include_str!("effect.wgsl");

impl CustomShader {
    /// Checks `source`, a shader's file called `name`, and joins it to the
    /// engine's code.
    pub fn new(name: &str, source: &str) -> Result<Self, ShaderError> {
        if has_fn(source, "effect") {
            return Self::new_effect(name, source);
        }
        let error = |line: Option<u32>, column: Option<u32>, message: String| ShaderError {
            file: name.to_owned(),
            line,
            column,
            message,
        };
        let (own_source, textures) = bind_textures(source).map_err(|(line, m)| error(Some(line), None, m))?;
        let has = |word: &str| has_struct(source, word);
        // First as it is, with what the glue declares, to find what it has.
        let mut declared = String::new();
        if has("Values") {
            declared.push_str("var<private> values: Values;\n");
        }
        if has("PerObject") {
            declared.push_str("var<private> per_object: PerObject;\n");
        }
        let probe = Joined::new(
            BEFORE,
            &declared,
            &own_source,
            "fn shade(in: VertexOut, front_facing: bool) -> vec4<f32> {\n    return vec4<f32>(0.0);\n}\n\n\
             fn fy_moved(in: VertexIn, model: array<vec4<f32>, 3>, out: VertexOut) -> VertexOut {\n    return out;\n}\n\n\
             fn fy_light(b: Brdf, s: Surface, p: vec3<f32>, l: vec3<f32>, color: vec3<f32>, shadow: f32, \
             distance: f32, highlights: bool) -> vec3<f32> {\n    return vec3<f32>(0.0);\n}\n",
        );
        let module = probe.parse().map_err(|e| e.into_error(name))?;

        let surface = module.functions.iter().find(|(_, f)| f.name.as_deref() == Some("surface"));
        let unlit = module.functions.iter().find(|(_, f)| f.name.as_deref() == Some("unlit"));
        let lighting = match (surface, unlit) {
            (Some(_), None) => Lighting::Lit,
            (None, Some(_)) => Lighting::Unlit,
            (Some(_), Some(_)) => {
                return Err(error(None, None, "has both `surface` and `unlit`: a shader has one".into()));
            }
            (None, None) => {
                return Err(error(
                    None,
                    None,
                    "has no `fn surface(in: SurfaceInput) -> Surface` (lit) or `fn unlit(in: SurfaceInput) -> vec4<f32>`"
                        .into(),
                ));
            }
        };
        let moves = match module.functions.iter().find(|(_, f)| f.name.as_deref() == Some("vertex")) {
            None => false,
            Some((_, f)) => {
                let named = |ty: naga::Handle<naga::Type>| module.types[ty].name.as_deref() == Some("VertexInput");
                if f.arguments.len() != 1
                    || !named(f.arguments[0].ty)
                    || !f.result.as_ref().is_some_and(|r| named(r.ty))
                {
                    return Err(error(
                        None,
                        None,
                        "`vertex` must be `fn vertex(in: VertexInput) -> VertexInput`".into(),
                    ));
                }
                true
            }
        };
        let lights = match module.functions.iter().find(|(_, f)| f.name.as_deref() == Some("light")) {
            None => false,
            Some((_, f)) => {
                let takes =
                    f.arguments.len() == 1 && module.types[f.arguments[0].ty].name.as_deref() == Some("LightInput");
                let gives = f.result.as_ref().is_some_and(|r| {
                    matches!(
                        module.types[r.ty].inner,
                        TypeInner::Vector {
                            size: VectorSize::Tri,
                            scalar: naga::Scalar { kind: ScalarKind::Float, width: 4 }
                        }
                    )
                });
                if !takes || !gives {
                    return Err(error(None, None, "`light` must be `fn light(in: LightInput) -> vec3<f32>`".into()));
                }
                if lighting == Lighting::Unlit {
                    return Err(error(None, None, "has `light` and `unlit`: an unlit shader isn't lit".into()));
                }
                true
            }
        };
        let (values, values_size) = shader_values(&module, name)?;
        let own = match find_struct(&module, "PerObject") {
            None => Vec::new(),
            Some(members) => {
                if members.len() > MAX_OWN {
                    return Err(error(
                        None,
                        None,
                        format!("`PerObject` has {} numbers; an object has {MAX_OWN}", members.len()),
                    ));
                }
                let mut own = Vec::new();
                for (name_of, ty, _) in members {
                    if value_kind(&module.types[ty].inner) != Some((ValueKind::F32, 1)) {
                        return Err(error(None, None, format!("`PerObject.{name_of}` isn't an f32, which they are")));
                    }
                    own.push(name_of);
                }
                own
            }
        };

        // Then with the glue that loads them and calls it.
        let mut glue = String::new();
        let mut load = String::new();
        if !values.is_empty() {
            glue.push_str(&values_loader(&values));
            load.push_str("    values = fy_values(in.values);\n");
        }
        if !own.is_empty() {
            let fields = ["x", "y", "z", "w"][..own.len()].iter().map(|c| format!("in.own.{c}")).collect::<Vec<_>>();
            load.push_str(&format!("    per_object = PerObject({});\n", fields.join(", ")));
        }
        glue.push_str("fn fy_moved(in: VertexIn, model: array<vec4<f32>, 3>, out: VertexOut) -> VertexOut {\n");
        if moves {
            // What `local` moved by goes where the vertex's matrix (its
            // object's, or its skin's) takes it.
            glue.push_str(&load);
            glue.push_str(
                "    let origin = vec3<f32>(in.model_0.w, in.model_1.w, in.model_2.w);\n    \
                 let v = vertex(VertexInput(out.world_position, normalize(out.normal), out.uv, in.position, origin));\n    \
                 let turn = mat3x3<f32>(vec3<f32>(model[0].x, model[1].x, model[2].x), \
                 vec3<f32>(model[0].y, model[1].y, model[2].y), vec3<f32>(model[0].z, model[1].z, model[2].z));\n    \
                 var moved = out;\n    moved.world_position = v.position + turn * (v.local - in.position);\n    \
                 moved.normal = v.normal;\n    moved.uv = v.uv;\n    return moved;\n}\n\n",
            );
        } else {
            glue.push_str("    return out;\n}\n\n");
        }
        glue.push_str(
            "fn fy_light(b: Brdf, s: Surface, p: vec3<f32>, l: vec3<f32>, color: vec3<f32>, shadow: f32, \
             distance: f32, highlights: bool) -> vec3<f32> {\n",
        );
        glue.push_str(if lights {
            "    fy_highlights = highlights;\n    return light(LightInput(l, color, shadow, distance, p, b.n, b.v, s));\n}\n\n"
        } else {
            "    return standard_lit(b, l, color, shadow, highlights);\n}\n\n"
        });
        glue.push_str("fn fy_begin(in: VertexOut, front_facing: bool) {\n");
        glue.push_str("    fy_in = in;\n    fy_front = front_facing;\n");
        glue.push_str(&load);
        glue.push_str(
            "}\n\nfn shade(in: VertexOut, front_facing: bool) -> vec4<f32> {\n    fy_begin(in, front_facing);\n",
        );
        glue.push_str(match lighting {
            Lighting::Lit => "    return lit(in, front_facing, surface(fy_surface_input(in, front_facing)));\n}\n",
            Lighting::Unlit => "    return unlit(fy_surface_input(in, front_facing));\n}\n",
        });
        glue.push_str("\nfn fy_alpha(in: VertexOut, front_facing: bool) -> f32 {\n    fy_begin(in, front_facing);\n");
        glue.push_str(match lighting {
            Lighting::Lit => "    return surface(fy_surface_input(in, front_facing)).alpha;\n}\n",
            Lighting::Unlit => "    return unlit(fy_surface_input(in, front_facing)).a;\n}\n",
        });
        glue.push_str(SHADOW);
        let joined = Joined::new(BEFORE, &declared, &own_source, &glue);
        let module = joined.parse().map_err(|e| e.into_error(name))?;
        let info = joined.validate(&module).map_err(|e| e.into_error(name))?;
        if moves {
            // The passes that draw only depth have no textures to give it.
            let (at, _) = module.entry_points.iter().enumerate().find(|(_, e)| e.name == "vs_fy_depth").expect("glued");
            let uses = info.get_entry_point(at);
            for (handle, global) in module.global_variables.iter() {
                if global.binding.as_ref().is_some_and(|b| b.group != 0) && !uses[handle].is_empty() {
                    let what = global.name.as_deref().unwrap_or("a texture");
                    return Err(error(
                        None,
                        None,
                        format!(
                            "`vertex` reads `{what}`: it can read its values, its object's numbers and the time, not textures"
                        ),
                    ));
                }
            }
        }

        // What's behind is there only for what's drawn over it.
        let (at, _) =
            module.entry_points.iter().enumerate().find(|(_, e)| e.name == "fs_transparent").expect("forward.wgsl's");
        let uses = info.get_entry_point(at);
        let reads_behind = module.global_variables.iter().any(|(handle, global)| {
            matches!(global.name.as_deref(), Some("fy_scene_depth" | "fy_scene_color")) && !uses[handle].is_empty()
        });

        let wgsl: Arc<str> = joined.source.into();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        wgsl.hash(&mut hasher);
        Ok(Self {
            name: name.to_owned(),
            lighting,
            values,
            values_size,
            textures,
            own,
            moves,
            reads_behind,
            lights,
            effect: false,
            wgsl,
            hash: hasher.finish(),
        })
    }

    /// A screen effect's: `fn effect(in: EffectInput) -> vec4<f32>`, and
    /// perhaps its `Values`.
    fn new_effect(name: &str, source: &str) -> Result<Self, ShaderError> {
        let error = |message: &str| ShaderError {
            file: name.to_owned(),
            line: None,
            column: None,
            message: message.to_owned(),
        };
        for unexpected in ["surface", "unlit", "vertex", "light"] {
            if has_fn(source, unexpected) {
                return Err(error(&format!("has `effect` and `{unexpected}`: a screen effect has only `effect`")));
            }
        }
        if has_struct(source, "PerObject") {
            return Err(error("a screen effect has no `PerObject`: it isn't drawn for an object"));
        }
        let (_, textures) = bind_textures(source).map_err(|(line, m)| ShaderError {
            file: name.to_owned(),
            line: Some(line),
            column: None,
            message: m,
        })?;
        if !textures.is_empty() {
            return Err(error("a screen effect can't have textures of its own: it reads the picture"));
        }
        let declared = if has_struct(source, "Values") { "var<private> values: Values;\n" } else { "" };
        let probe = Joined::new(EFFECT, declared, source, "");
        let module = probe.parse().map_err(|e| e.into_error(name))?;
        let (_, f) = module.functions.iter().find(|(_, f)| f.name.as_deref() == Some("effect")).expect("has_fn");
        let takes = f.arguments.len() == 1 && module.types[f.arguments[0].ty].name.as_deref() == Some("EffectInput");
        let gives = f.result.as_ref().is_some_and(|r| {
            matches!(
                module.types[r.ty].inner,
                TypeInner::Vector {
                    size: VectorSize::Quad,
                    scalar: naga::Scalar { kind: ScalarKind::Float, width: 4 }
                }
            )
        });
        if !takes || !gives {
            return Err(error("`effect` must be `fn effect(in: EffectInput) -> vec4<f32>`"));
        }
        let (values, values_size) = shader_values(&module, name)?;

        let mut glue = String::new();
        let mut load = "";
        if !values.is_empty() {
            glue.push_str(&effect_values_loader(&values));
            load = "    values = fy_values();\n";
        }
        glue.push_str(&format!(
            "@fragment\nfn fs_fy_effect(in: FyOut) -> @location(0) vec4<f32> {{\n{load}    \
             let seen = effect(EffectInput(in.uv, in.clip.xy, fy_effect.frame.xy, fy_effect.frame.z));\n    \
             return vec4<f32>(seen.rgb, 1.0);\n}}\n"
        ));
        let joined = Joined::new(EFFECT, declared, source, &glue);
        let module = joined.parse().map_err(|e| e.into_error(name))?;
        joined.validate(&module).map_err(|e| e.into_error(name))?;
        let wgsl: Arc<str> = joined.source.into();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        wgsl.hash(&mut hasher);
        Ok(Self {
            name: name.to_owned(),
            lighting: Lighting::Unlit,
            values,
            values_size,
            textures: Vec::new(),
            own: Vec::new(),
            moves: false,
            reads_behind: false,
            lights: false,
            effect: true,
            wgsl,
            hash: hasher.finish(),
        })
    }
}

/// `module`'s `Values`, if it has them, and how many bytes they take.
fn shader_values(module: &naga::Module, file: &str) -> Result<(Vec<ShaderValue>, u32), ShaderError> {
    let error = |message: String| ShaderError { file: file.to_owned(), line: None, column: None, message };
    let Some(members) = find_struct(module, "Values") else {
        return Ok((Vec::new(), 0));
    };
    let mut values = Vec::new();
    for (name_of, ty, offset) in members {
        let (kind, size) = value_kind(&module.types[ty].inner).ok_or_else(|| {
            error(format!("`Values.{name_of}` isn't a number or a vector of f32, i32 or u32, which values are"))
        })?;
        values.push(ShaderValue { name: name_of, kind, size, offset });
    }
    let size = struct_size(module, "Values");
    if size > MAX_VALUES_BYTES {
        return Err(error(format!("`Values` takes {size} bytes; it may take {MAX_VALUES_BYTES}")));
    }
    Ok((values, size))
}

/// A shader's file between the engine's code: where it starts, so a
/// message can say its own line.
struct Joined {
    source: String,
    /// The lines before the shader's.
    before: u32,
    /// The shader's lines.
    lines: u32,
}

/// What went wrong, and where in the joined module.
struct Failure {
    location: Option<naga::SourceLocation>,
    message: String,
    joined: Joined,
}

impl Failure {
    fn into_error(self, file: &str) -> ShaderError {
        let Joined { before, lines, .. } = self.joined;
        match self.location {
            Some(at) if at.line_number > before && at.line_number <= before + lines => ShaderError {
                file: file.to_owned(),
                line: Some(at.line_number - before),
                column: Some(at.line_position),
                message: self.message,
            },
            // Not in the shader's own lines: a name it uses that the
            // engine has too, most often.
            _ => ShaderError { file: file.to_owned(), line: None, column: None, message: self.message },
        }
    }
}

impl Joined {
    /// `own` after `engine` and what the glue `declared`, then `glue`.
    fn new(engine: &str, declared: &str, own: &str, glue: &str) -> Self {
        let mut source = String::with_capacity(engine.len() + declared.len() + own.len() + glue.len() + 64);
        source.push_str(engine);
        source.push_str(declared);
        let before = source.lines().count() as u32;
        source.push_str(own);
        if !own.ends_with('\n') {
            source.push('\n');
        }
        let lines = own.lines().count() as u32;
        source.push_str(glue);
        Self { source, before, lines }
    }

    fn parse(&self) -> Result<naga::Module, Failure> {
        naga::front::wgsl::parse_str(&self.source).map_err(|e| {
            let mut message = e.message().to_owned();
            // As in `validate`: the last place in the shader's own lines.
            let mut location = e.location(&self.source);
            for (span, label) in e.labels() {
                let at = span.location(&self.source);
                if self.owns(at.line_number) {
                    location = Some(at);
                    if !label.is_empty() && !message.contains(label) {
                        message = format!("{message} ({label})");
                    }
                }
            }
            Failure { location, message, joined: self.copy() }
        })
    }

    fn validate(&self, module: &naga::Module) -> Result<naga::valid::ModuleInfo, Failure> {
        let mut validator =
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty());
        validator.validate(module).map_err(|e| {
            let mut message = e.as_inner().to_string();
            let mut cause = std::error::Error::source(e.as_inner());
            while let Some(c) = cause {
                message.push_str(": ");
                message.push_str(&c.to_string());
                cause = c.source();
            }
            // The last place it names in the shader's own lines is the
            // nearest to the mistake (the first is the whole function).
            let mut location = e.location(&self.source);
            for (span, label) in e.spans() {
                let at = span.location(&self.source);
                if self.owns(at.line_number) {
                    location = Some(at);
                    if !label.is_empty() && !message.contains(label.as_str()) {
                        message = format!("{message} ({label})");
                    }
                }
            }
            Failure { location, message, joined: self.copy() }
        })
    }

    fn copy(&self) -> Self {
        Self { source: String::new(), before: self.before, lines: self.lines }
    }

    /// Whether the joined module's `line` is one of the shader's own.
    fn owns(&self, line: u32) -> bool {
        line > self.before && line <= self.before + self.lines
    }
}

/// `source` with its textures given their bindings, the lines kept where
/// they are, and the textures in order. A line that declares one is
/// `var name: texture_2d<f32>;` or `var name: color_texture;`.
fn bind_textures(source: &str) -> Result<(String, Vec<ShaderTexture>), (u32, String)> {
    let mut out = String::with_capacity(source.len() + 128);
    let mut textures = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let declared = line.trim().strip_prefix("var ").and_then(|rest| {
            let (name, ty) = rest.split_once(':')?;
            let ty: String = ty.trim().strip_suffix(';')?.chars().filter(|c| !c.is_whitespace()).collect();
            let space = match ty.as_str() {
                "texture_2d<f32>" => ColorSpace::Linear,
                "color_texture" => ColorSpace::Srgb,
                _ => return None,
            };
            Some((name.trim().to_owned(), space))
        });
        match declared {
            Some((name, space)) => {
                if textures.len() == MAX_TEXTURES {
                    return Err((i as u32 + 1, format!("a shader may have {MAX_TEXTURES} textures of its own")));
                }
                let binding = FIRST_TEXTURE + textures.len() as u32;
                out.push_str(&format!("@group(1) @binding({binding}) var {name}: texture_2d<f32>;"));
                textures.push(ShaderTexture { name, space });
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    Ok((out, textures))
}

/// Whether `source` declares `fn name`.
fn has_fn(source: &str, name: &str) -> bool {
    source.lines().any(|line| {
        line.trim_start().strip_prefix("fn").is_some_and(|rest| {
            rest.starts_with(char::is_whitespace)
                && rest.trim_start().strip_prefix(name).is_some_and(|r| r.trim_start().starts_with('('))
        })
    })
}

/// Whether `source` declares `struct name`.
fn has_struct(source: &str, name: &str) -> bool {
    source.lines().any(|line| {
        let mut words = line.split(|c: char| c.is_whitespace() || c == '{');
        words.next() == Some("struct") && words.find(|w| !w.is_empty()) == Some(name)
    })
}

/// A struct's members: name, type and offset.
fn find_struct(module: &naga::Module, name: &str) -> Option<Vec<(String, naga::Handle<naga::Type>, u32)>> {
    module.types.iter().find_map(|(_, ty)| match &ty.inner {
        TypeInner::Struct { members, .. } if ty.name.as_deref() == Some(name) => {
            Some(members.iter().map(|m| (m.name.clone().unwrap_or_default(), m.ty, m.offset)).collect())
        }
        _ => None,
    })
}

fn struct_size(module: &naga::Module, name: &str) -> u32 {
    module
        .types
        .iter()
        .find_map(|(_, ty)| match &ty.inner {
            TypeInner::Struct { span, .. } if ty.name.as_deref() == Some(name) => Some(*span),
            _ => None,
        })
        .unwrap_or(0)
}

fn value_kind(ty: &TypeInner) -> Option<(ValueKind, u32)> {
    let (scalar, size) = match ty {
        TypeInner::Scalar(s) => (*s, 1),
        TypeInner::Vector { size, scalar } => (
            *scalar,
            match size {
                VectorSize::Bi => 2,
                VectorSize::Tri => 3,
                VectorSize::Quad => 4,
            },
        ),
        _ => return None,
    };
    if scalar.width != 4 {
        return None;
    }
    let kind = match scalar.kind {
        ScalarKind::Float => ValueKind::F32,
        ScalarKind::Sint => ValueKind::I32,
        ScalarKind::Uint => ValueKind::U32,
        _ => return None,
    };
    Some((kind, size))
}

/// `fn fy_values(at: u32) -> Values`: the values read from where the
/// renderer put them among the joints (48 bytes, three rows, each).
fn values_loader(values: &[ShaderValue]) -> String {
    let mut s = String::from("fn fy_values(at: u32) -> Values {\n    var v: Values;\n");
    for value in values {
        let joint = value.offset / 48;
        let row = (value.offset % 48) / 16;
        let first = (value.offset % 16) / 4;
        let components = &"xyzw"[first as usize..(first + value.size) as usize];
        let read = format!("joints[at + {joint}u].row_{row}.{components}");
        let read = match value.kind {
            ValueKind::F32 => read,
            ValueKind::I32 => format!("bitcast<{}>({read})", wgsl_type("i32", value.size)),
            ValueKind::U32 => format!("bitcast<{}>({read})", wgsl_type("u32", value.size)),
        };
        s.push_str(&format!("    v.{} = {read};\n", value.name));
    }
    s.push_str("    return v;\n}\n");
    s
}

/// `fn fy_values() -> Values`: a screen effect's values, read from its
/// uniform (effect.wgsl), laid out as they are there.
fn effect_values_loader(values: &[ShaderValue]) -> String {
    let mut s = String::from("fn fy_values() -> Values {\n    var v: Values;\n");
    for value in values {
        let first = (value.offset % 16) / 4;
        let components = &"xyzw"[first as usize..(first + value.size) as usize];
        let read = format!("fy_effect.values[{}].{components}", value.offset / 16);
        let read = match value.kind {
            ValueKind::F32 => read,
            ValueKind::I32 => format!("bitcast<{}>({read})", wgsl_type("i32", value.size)),
            ValueKind::U32 => format!("bitcast<{}>({read})", wgsl_type("u32", value.size)),
        };
        s.push_str(&format!("    v.{} = {read};\n", value.name));
    }
    s.push_str("    return v;\n}\n");
    s
}

fn wgsl_type(scalar: &str, size: u32) -> String {
    if size == 1 { scalar.to_owned() } else { format!("vec{size}<{scalar}>") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shader_that_copies_the_standard_surface_is_lit() {
        let shader =
            CustomShader::new("same.wgsl", "fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n")
                .expect("it compiles");
        assert_eq!(shader.lighting, Lighting::Lit);
        assert!(shader.values.is_empty() && shader.textures.is_empty() && shader.own.is_empty());
    }

    #[test]
    fn values_textures_and_an_objects_numbers_are_found() {
        let source = "struct Values { flow: f32, hot: vec4<f32>, steps: i32, size: vec2<f32> }
struct PerObject { flash: f32, burn: f32 }
var noise_map: texture_2d<f32>;
var picture: color_texture;

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let n = sample(noise_map, in.uv * values.size).r + sample(picture, in.uv).g;
    s.emission = values.hot.rgb * n * values.flow * f32(values.steps) + per_object.flash + per_object.burn;
    return s;
}
";
        let shader = CustomShader::new("lava.wgsl", source).expect("it compiles");
        let names: Vec<_> = shader.values.iter().map(|v| (v.name.as_str(), v.kind, v.size, v.offset)).collect();
        assert_eq!(
            names,
            [
                ("flow", ValueKind::F32, 1, 0),
                ("hot", ValueKind::F32, 4, 16),
                ("steps", ValueKind::I32, 1, 32),
                ("size", ValueKind::F32, 2, 40)
            ]
        );
        assert_eq!(shader.values_size, 48);
        assert_eq!(
            shader.textures,
            [
                ShaderTexture { name: "noise_map".into(), space: ColorSpace::Linear },
                ShaderTexture { name: "picture".into(), space: ColorSpace::Srgb }
            ]
        );
        assert_eq!(shader.own, ["flash", "burn"]);
    }

    #[test]
    fn an_unlit_shader_is_unlit() {
        let shader = CustomShader::new(
            "glow.wgsl",
            "fn unlit(in: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(in.color.rgb * fresnel(in, 2.0), 1.0);\n}\n",
        )
        .expect("it compiles");
        assert_eq!(shader.lighting, Lighting::Unlit);
    }

    #[test]
    fn a_mistake_is_named_by_its_line_in_the_shaders_file() {
        let error = CustomShader::new(
            "broken.wgsl",
            "// a comment\nfn surface(in: SurfaceInput) -> Surface {\n    var s = standard(in);\n    s.colour = vec3<f32>(1.0);\n    return s;\n}\n",
        )
        .expect_err("there's no `colour`");
        assert_eq!((error.file.as_str(), error.line), ("broken.wgsl", Some(4)), "{error}");
        assert!(error.to_string().starts_with("broken.wgsl:4:"), "{error}");
    }

    #[test]
    fn a_shader_without_a_surface_says_what_it_needs() {
        let error = CustomShader::new("empty.wgsl", "fn helper() -> f32 { return 1.0; }\n").expect_err("no surface");
        assert!(error.message.contains("fn surface(in: SurfaceInput) -> Surface"), "{error}");
    }

    #[test]
    fn a_wrong_type_is_found_where_it_is() {
        let error = CustomShader::new(
            "types.wgsl",
            "fn surface(in: SurfaceInput) -> Surface {\n    var s = standard(in);\n    s.roughness = in.uv;\n    return s;\n}\n",
        )
        .expect_err("a vec2 isn't a roughness");
        assert_eq!(error.line, Some(3), "{error}");
    }

    #[test]
    fn a_vertex_that_moves_is_found_and_reads_values_and_the_time() {
        let source = "struct Values { strength: f32 }
struct PerObject { phase: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let sway = sin(frame_time() + in.origin.x + per_object.phase) * values.strength;
    v.position.x += sway * in.local.y;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    return standard(in);
}
";
        let shader = CustomShader::new("grass.wgsl", source).expect("it compiles");
        assert!(shader.moves);
        let still =
            CustomShader::new("still.wgsl", "fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n");
        assert!(!still.expect("it compiles").moves);
    }

    #[test]
    fn a_vertex_with_the_wrong_shape_says_what_it_must_be() {
        let error = CustomShader::new(
            "wave.wgsl",
            "fn vertex(p: vec3<f32>) -> vec3<f32> {\n    return p;\n}\nfn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n",
        )
        .expect_err("it takes a VertexInput");
        assert!(error.message.contains("fn vertex(in: VertexInput) -> VertexInput"), "{error}");
    }

    #[test]
    fn a_vertex_cant_read_a_texture() {
        let error = CustomShader::new(
            "bumps.wgsl",
            "var height: texture_2d<f32>;\nfn vertex(in: VertexInput) -> VertexInput {\n    var v = in;\n    \
             v.position.y += textureLoad(height, vec2<i32>(0, 0), 0).r;\n    return v;\n}\n\
             fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n",
        )
        .expect_err("no textures in the depth passes");
        assert!(error.message.contains("`height`"), "{error}");
    }

    #[test]
    fn values_that_arent_numbers_are_refused() {
        let error = CustomShader::new(
            "values.wgsl",
            "struct Values { m: mat4x4<f32> }\nfn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n",
        )
        .expect_err("a matrix isn't a value");
        assert!(error.message.contains("Values.m"), "{error}");
    }

    #[test]
    fn five_numbers_for_an_object_are_too_many() {
        let error = CustomShader::new(
            "own.wgsl",
            "struct PerObject { a: f32, b: f32, c: f32, d: f32, e: f32 }\nfn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n",
        )
        .expect_err("four at most");
        assert!(error.message.contains("PerObject"), "{error}");
    }
}
