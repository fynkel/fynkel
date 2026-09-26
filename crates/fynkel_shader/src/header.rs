//! The header of a `.shader` file: `shader "<name>" { … }` up to its
//! closing brace. Everything after it is the body, left to naga.
//!
//! ```text
//! shader "Examples/Water" {
//!     lighting lit | unlit            (default lit)
//!     alpha opaque, mask, blend       (default all three)
//!     shadows on | off                (default on)
//!     properties {
//!         <name> <type> [= <default>] [range(<min>, <max>)] [srgb | linear] [required] ["tooltip"]
//!     }
//! }
//! ```
//!
//! One statement or property per line; `//` and `/* */` comments
//! anywhere. A bad line is reported and skipped so one mistake is one
//! diagnostic, and every error is at its line and column.

use fynkel_contracts::{Code, Diagnostic};

use crate::layout::Layout;
use crate::{
    AlphaMode, ColorSpace, Fallback, Interface, Lighting, Property, PropertyKind, Shader,
    ValueType, MAX_BLOCK_BYTES, MAX_TEXTURES,
};

/// Reads a `.shader` file's header into its [`Interface`] and places the
/// properties ([`Layout`]). The body is not compiled here — that is
/// [`crate::check`].
pub fn parse(path: Option<&str>, text: &str) -> Result<Shader, Vec<Diagnostic>> {
    let mut p = Parser {
        lexer: Lexer {
            text,
            pos: 0,
            line: 1,
            column: 1,
        },
        peeked: None,
        diags: Vec::new(),
        path,
    };
    let result = p.file();
    let mut diags = p.diags;
    let (interface, body_start) = match result {
        Some(ok) => ok,
        None => return Err(diags),
    };
    let layout = Layout::of(&interface);
    let textures = interface.textures().count();
    if textures > MAX_TEXTURES {
        let extra = interface.textures().nth(MAX_TEXTURES).expect("counted");
        diags.push(
            err(
                path,
                Code::SHADER_HEADER,
                format!("{textures} texture properties; a shader has at most {MAX_TEXTURES}"),
            )
            .at(extra.line, extra.column),
        );
    }
    if layout.size > MAX_BLOCK_BYTES {
        let last = layout
            .fields
            .iter()
            .find(|f| f.offset + f.size() > MAX_BLOCK_BYTES)
            .expect("over the limit");
        let at = interface
            .property(&last.name)
            .expect("a field is a property");
        diags.push(
            err(
                path,
                Code::SHADER_HEADER,
                format!(
                    "the properties take {} bytes; a shader has at most {MAX_BLOCK_BYTES} (`{}` is past it)",
                    layout.size, last.name
                ),
            )
            .at(at.line, at.column)
            .with_suggestion("a vec3 or color3 takes 16 bytes; fewer or smaller properties fit"),
        );
    }
    if diags.iter().any(Diagnostic::is_error) {
        return Err(diags);
    }
    Ok(Shader {
        path: path.map(str::to_string),
        interface,
        layout,
        text: text.to_string(),
        body_start,
    })
}

fn err(path: Option<&str>, code: Code, message: impl Into<String>) -> Diagnostic {
    let d = Diagnostic::error(code, message);
    match path {
        Some(p) => d.with_document(p),
        None => d,
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Tok {
    Ident(String),
    Number(f64),
    Str(String),
    Punct(char),
    Newline,
    Eof,
    /// Something no header has: a stray character or an unclosed string.
    Bad(String),
}

impl Tok {
    fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Number(n) => format!("`{n}`"),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Punct(c) => format!("`{c}`"),
            Tok::Newline => "the end of the line".into(),
            Tok::Eof => "the end of the file".into(),
            Tok::Bad(s) => s.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct Spanned {
    tok: Tok,
    line: u32,
    column: u32,
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    line: u32,
    column: u32,
}

impl Lexer<'_> {
    fn peek_char(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn next(&mut self) -> Spanned {
        loop {
            // Blanks and comments; a newline (or a comment across one) is a token.
            let (line, column) = (self.line, self.column);
            match self.peek_char() {
                None => {
                    return Spanned {
                        tok: Tok::Eof,
                        line,
                        column,
                    }
                }
                Some('\n') => {
                    self.bump();
                    return Spanned {
                        tok: Tok::Newline,
                        line,
                        column,
                    };
                }
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.text[self.pos..].starts_with("//") => {
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.text[self.pos..].starts_with("/*") => {
                    let start_line = self.line;
                    self.bump();
                    self.bump();
                    let mut closed = false;
                    while let Some(c) = self.bump() {
                        if c == '*' && self.peek_char() == Some('/') {
                            self.bump();
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        return Spanned {
                            tok: Tok::Bad("an unclosed `/*` comment".into()),
                            line,
                            column,
                        };
                    }
                    if self.line != start_line {
                        return Spanned {
                            tok: Tok::Newline,
                            line,
                            column,
                        };
                    }
                }
                Some(_) => break,
            }
        }
        let (line, column) = (self.line, self.column);
        let c = self.peek_char().expect("not at the end");
        let tok = if c.is_alphabetic() || c == '_' {
            let start = self.pos;
            while self
                .peek_char()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                self.bump();
            }
            Tok::Ident(self.text[start..self.pos].to_string())
        } else if c.is_ascii_digit()
            || c == '.'
            || ((c == '-' || c == '+') && self.number_follows())
        {
            let start = self.pos;
            self.bump();
            while self
                .peek_char()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
                || (self.peek_char().is_some_and(|c| c == '-' || c == '+')
                    && self.text[..self.pos].ends_with(['e', 'E']))
            {
                self.bump();
            }
            let raw = &self.text[start..self.pos];
            match raw.parse::<f64>() {
                Ok(n) if n.is_finite() => Tok::Number(n),
                _ => Tok::Bad(format!("`{raw}` is not a number")),
            }
        } else if c == '"' {
            self.bump();
            let mut s = String::new();
            loop {
                match self.bump() {
                    Some('"') => break Tok::Str(s),
                    Some('\\') => match self.bump() {
                        Some('n') => s.push('\n'),
                        Some(c) => s.push(c),
                        None => break Tok::Bad("an unclosed string".into()),
                    },
                    Some('\n') | None => break Tok::Bad("an unclosed string".into()),
                    Some(c) => s.push(c),
                }
            }
        } else {
            self.bump();
            if "{}(),=;".contains(c) {
                Tok::Punct(c)
            } else {
                Tok::Bad(format!("`{c}`"))
            }
        };
        Spanned { tok, line, column }
    }

    fn number_follows(&self) -> bool {
        let mut rest = self.text[self.pos..].chars();
        rest.next();
        rest.next().is_some_and(|c| c.is_ascii_digit() || c == '.')
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    peeked: Option<Spanned>,
    diags: Vec<Diagnostic>,
    path: Option<&'a str>,
}

/// A statement failed; the parser has reported it and skips its line.
struct Skip;

impl Parser<'_> {
    fn peek(&mut self) -> &Spanned {
        if self.peeked.is_none() {
            self.peeked = Some(self.lexer.next());
        }
        self.peeked.as_ref().expect("just filled")
    }

    fn next(&mut self) -> Spanned {
        self.peek();
        self.peeked.take().expect("peeked")
    }

    fn error(&mut self, code: Code, at: &Spanned, message: impl Into<String>) {
        self.diags
            .push(err(self.path, code, message).at(at.line, at.column));
    }

    fn expected(&mut self, what: &str, got: &Spanned) -> Skip {
        let message = format!("expected {what}, found {}", got.tok.describe());
        self.error(Code::SHADER_HEADER, got, message);
        Skip
    }

    fn skip_newlines(&mut self) {
        while self.peek().tok == Tok::Newline {
            self.next();
        }
    }

    /// Skips the rest of a bad line, stopping before a `}` so the block
    /// still closes.
    fn recover(&mut self) {
        loop {
            match self.peek().tok {
                Tok::Newline => {
                    self.next();
                    return;
                }
                Tok::Eof | Tok::Punct('}') => return,
                _ => {
                    self.next();
                }
            }
        }
    }

    /// The end of a statement: a newline, `;`, or the block's `}` (not consumed).
    fn end_of_statement(&mut self) -> Result<(), Skip> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Newline | Tok::Punct(';') => {
                self.next();
                Ok(())
            }
            Tok::Punct('}') | Tok::Eof => Ok(()),
            _ => Err(self.expected("the end of the line", &t)),
        }
    }

    fn file(&mut self) -> Option<(Interface, usize)> {
        self.skip_newlines();
        let first = self.next();
        if first.tok != Tok::Ident("shader".into()) {
            let message = format!(
                "a .shader file starts with `shader \"<name>\" {{ … }}`, found {}",
                first.tok.describe()
            );
            self.diags.push(
                err(self.path, Code::SHADER_HEADER, message)
                    .at(first.line, first.column)
                    .with_suggestion("shader \"My/Shader\" {\n    properties {\n    }\n}\n\nfn surface(in: SurfaceInput) -> Surface { … }"),
            );
            return None;
        }
        let name_tok = self.next();
        let name = match name_tok.tok {
            Tok::Str(ref s) if !s.trim().is_empty() => s.trim().to_string(),
            Tok::Str(_) => {
                self.error(Code::SHADER_HEADER, &name_tok, "the shader's name is empty");
                String::new()
            }
            _ => {
                self.expected(
                    "the shader's name in quotes (`shader \"Examples/Water\" {`)",
                    &name_tok,
                );
                return None;
            }
        };
        self.skip_newlines();
        let open = self.next();
        if open.tok != Tok::Punct('{') {
            self.expected("`{`", &open);
            return None;
        }

        let mut lighting: Option<Lighting> = None;
        let mut alpha: Option<Vec<AlphaMode>> = None;
        let mut shadows: Option<bool> = None;
        let mut properties: Option<Vec<Property>> = None;
        loop {
            self.skip_newlines();
            let t = self.next();
            let word = match &t.tok {
                Tok::Punct('}') => break,
                Tok::Eof => {
                    self.error(Code::SHADER_HEADER, &t, "the header's `{` is never closed");
                    return None;
                }
                Tok::Ident(w) => w.clone(),
                _ => {
                    self.expected("`lighting`, `alpha`, `shadows`, `properties` or `}`", &t);
                    self.recover();
                    continue;
                }
            };
            let result = match word.as_str() {
                "lighting" => self
                    .once(&t, lighting.is_some())
                    .and_then(|_| self.lighting())
                    .map(|l| lighting = Some(l)),
                "alpha" => self
                    .once(&t, alpha.is_some())
                    .and_then(|_| self.alpha())
                    .map(|a| alpha = Some(a)),
                "shadows" => self
                    .once(&t, shadows.is_some())
                    .and_then(|_| self.shadows())
                    .map(|s| shadows = Some(s)),
                "properties" => {
                    let dup = self.once(&t, properties.is_some());
                    let list = self.properties();
                    dup.map(|_| properties = Some(list))
                }
                other => {
                    let message = format!("unknown header statement `{other}`");
                    self.diags.push(
                        err(self.path, Code::SHADER_HEADER, message)
                            .at(t.line, t.column)
                            .with_suggestion(
                                "the header takes `lighting`, `alpha`, `shadows` and `properties`",
                            ),
                    );
                    Err(Skip)
                }
            };
            if result.is_err() {
                self.recover();
            }
        }
        Some((
            Interface {
                name,
                lighting: lighting.unwrap_or(Lighting::Lit),
                alpha: alpha.unwrap_or_else(|| AlphaMode::ALL.to_vec()),
                shadows: shadows.unwrap_or(true),
                properties: properties.unwrap_or_default(),
            },
            self.lexer.pos,
        ))
    }

    fn once(&mut self, t: &Spanned, seen: bool) -> Result<(), Skip> {
        if seen {
            let message = format!("{} is given twice", t.tok.describe());
            self.error(Code::SHADER_HEADER, t, message);
            return Err(Skip);
        }
        Ok(())
    }

    fn lighting(&mut self) -> Result<Lighting, Skip> {
        let t = self.next();
        let l = match &t.tok {
            Tok::Ident(w) if w == "lit" => Lighting::Lit,
            Tok::Ident(w) if w == "unlit" => Lighting::Unlit,
            _ => return Err(self.expected("`lit` or `unlit`", &t)),
        };
        self.end_of_statement()?;
        Ok(l)
    }

    fn alpha(&mut self) -> Result<Vec<AlphaMode>, Skip> {
        let mut modes = Vec::new();
        loop {
            let t = self.next();
            let mode = match &t.tok {
                Tok::Ident(w) => AlphaMode::parse(w),
                _ => None,
            };
            let Some(mode) = mode else {
                return Err(self.expected("`opaque`, `mask` or `blend`", &t));
            };
            if modes.contains(&mode) {
                let message = format!("`{}` is listed twice", mode.name());
                self.error(Code::SHADER_HEADER, &t, message);
                return Err(Skip);
            }
            modes.push(mode);
            if self.peek().tok == Tok::Punct(',') {
                self.next();
                continue;
            }
            self.end_of_statement()?;
            return Ok(modes);
        }
    }

    fn shadows(&mut self) -> Result<bool, Skip> {
        let t = self.next();
        let on = match &t.tok {
            Tok::Ident(w) if w == "on" => true,
            Tok::Ident(w) if w == "off" => false,
            _ => return Err(self.expected("`on` or `off`", &t)),
        };
        self.end_of_statement()?;
        Ok(on)
    }

    /// `properties { … }`: reads every line it can; a bad one is reported
    /// and left out.
    fn properties(&mut self) -> Vec<Property> {
        let mut list: Vec<Property> = Vec::new();
        self.skip_newlines();
        let open = self.next();
        if open.tok != Tok::Punct('{') {
            self.expected("`{` after `properties`", &open);
            return list;
        }
        loop {
            self.skip_newlines();
            if matches!(self.peek().tok, Tok::Punct('}')) {
                self.next();
                break;
            }
            if self.peek().tok == Tok::Eof {
                let t = self.peek().clone();
                self.error(Code::SHADER_HEADER, &t, "`properties {` is never closed");
                break;
            }
            match self.property() {
                Ok(p) => {
                    if list.iter().any(|q| q.name == p.name) {
                        let message = format!("the property `{}` is declared twice", p.name);
                        self.diags.push(
                            err(self.path, Code::SHADER_HEADER, message).at(p.line, p.column),
                        );
                    } else {
                        list.push(p);
                    }
                }
                Err(Skip) => self.recover(),
            }
        }
        // The statement ends after the block's `}`.
        let _ = self.end_of_statement();
        list
    }

    fn property(&mut self) -> Result<Property, Skip> {
        let name_tok = self.next();
        let name = match &name_tok.tok {
            Tok::Ident(n) => n.clone(),
            _ => return Err(self.expected("a property name", &name_tok)),
        };
        if let Some(reason) = bad_name(&name) {
            let message = format!("`{name}` cannot name a property: {reason}");
            self.error(Code::SHADER_HEADER, &name_tok, message);
            return Err(Skip);
        }
        let type_tok = self.next();
        let is_texture = matches!(&type_tok.tok, Tok::Ident(t) if t == "texture");
        let ty = match &type_tok.tok {
            Tok::Ident(t) if is_texture => {
                let _ = t;
                None
            }
            Tok::Ident(t) => match ValueType::parse(t) {
                Some(ty) => Some(ty),
                None => {
                    let message = format!(
                        "unknown property type `{t}`; the types are float, int, bool, vec2, vec3, vec4, color3, color and texture"
                    );
                    self.error(Code::SHADER_HEADER, &type_tok, message);
                    return Err(Skip);
                }
            },
            _ => return Err(self.expected("the property's type", &type_tok)),
        };

        let mut default: Option<(Vec<f64>, Spanned)> = None;
        let mut fallback: Option<Fallback> = None;
        let mut range: Option<(f64, f64, Spanned)> = None;
        let mut color_space: Option<ColorSpace> = None;
        let mut required = false;
        let mut tooltip: Option<String> = None;
        loop {
            let t = self.peek().clone();
            match &t.tok {
                Tok::Newline | Tok::Punct(';') | Tok::Punct('}') | Tok::Eof => break,
                Tok::Punct('=') => {
                    self.next();
                    if default.is_some() || fallback.is_some() {
                        self.error(Code::SHADER_HEADER, &t, "the default is given twice");
                        return Err(Skip);
                    }
                    if is_texture {
                        let v = self.next();
                        match &v.tok {
                            Tok::Ident(w) if Fallback::parse(w).is_some() => {
                                fallback = Fallback::parse(w)
                            }
                            _ => {
                                return Err(
                                    self.expected("`white`, `black`, `normal` or `gray`", &v)
                                )
                            }
                        }
                    } else {
                        let at = self.peek().clone();
                        let v = self.value()?;
                        default = Some((v, at));
                    }
                }
                Tok::Ident(w) if w == "range" => {
                    self.next();
                    if ty.is_none() || ty == Some(ValueType::Bool) {
                        self.error(
                            Code::SHADER_HEADER,
                            &t,
                            "`range` is for number, vector and colour properties",
                        );
                        return Err(Skip);
                    }
                    let open = self.next();
                    if open.tok != Tok::Punct('(') {
                        return Err(self.expected("`(` after `range`", &open));
                    }
                    let min = self.number()?;
                    let comma = self.next();
                    if comma.tok != Tok::Punct(',') {
                        return Err(self.expected("`,` between the range's ends", &comma));
                    }
                    let max = self.number()?;
                    let close = self.next();
                    if close.tok != Tok::Punct(')') {
                        return Err(self.expected("`)`", &close));
                    }
                    range = Some((min, max, t.clone()));
                }
                Tok::Ident(w) if w == "srgb" || w == "linear" || w == "required" => {
                    self.next();
                    if !is_texture {
                        let message = format!("`{w}` is for texture properties");
                        self.error(Code::SHADER_HEADER, &t, message);
                        return Err(Skip);
                    }
                    match w.as_str() {
                        "srgb" => color_space = Some(ColorSpace::Srgb),
                        "linear" => color_space = Some(ColorSpace::Linear),
                        _ => required = true,
                    }
                }
                Tok::Str(s) => {
                    self.next();
                    tooltip = Some(s.clone());
                }
                _ => {
                    self.next();
                    return Err(self.expected(
                        "`= <default>`, `range(min, max)`, `srgb`, `linear`, `required`, a \"tooltip\" or the end of the line",
                        &t,
                    ));
                }
            }
        }
        if matches!(self.peek().tok, Tok::Newline | Tok::Punct(';')) {
            self.next();
        }

        let kind = match ty {
            None => {
                let fallback = fallback.unwrap_or(Fallback::White);
                PropertyKind::Texture {
                    fallback,
                    // A normal map is data; everything else defaults to colour.
                    color_space: color_space.unwrap_or(if fallback == Fallback::Normal {
                        ColorSpace::Linear
                    } else {
                        ColorSpace::Srgb
                    }),
                    required,
                }
            }
            Some(ty) => {
                let (min, max) = match &range {
                    Some((min, max, at)) => {
                        if min > max {
                            let message = format!("the range's min {min} is above its max {max}");
                            self.error(Code::SHADER_BAD_RANGE, at, message);
                        }
                        (Some(*min), Some(*max))
                    }
                    None => (None, None),
                };
                let value = match default {
                    Some((v, at)) => match check_default(ty, &v, min, max) {
                        Ok(v) => v,
                        Err(reason) => {
                            let message = format!("the default of `{name}` {reason}");
                            self.error(Code::SHADER_BAD_DEFAULT, &at, message);
                            ty.zero()
                        }
                    },
                    None => clamp_zero(ty, min, max),
                };
                PropertyKind::Value {
                    ty,
                    default: value,
                    min,
                    max,
                }
            }
        };
        Ok(Property {
            name,
            kind,
            tooltip,
            line: name_tok.line,
            column: name_tok.column,
        })
    }

    fn number(&mut self) -> Result<f64, Skip> {
        let t = self.next();
        match t.tok {
            Tok::Number(n) => Ok(n),
            _ => Err(self.expected("a number", &t)),
        }
    }

    /// `1.5`, `true`, `(0.1, 0.2, 0.3)`.
    fn value(&mut self) -> Result<Vec<f64>, Skip> {
        let t = self.next();
        match &t.tok {
            Tok::Number(n) => Ok(vec![*n]),
            Tok::Ident(w) if w == "true" => Ok(vec![1.0]),
            Tok::Ident(w) if w == "false" => Ok(vec![0.0]),
            Tok::Punct('(') => {
                let mut out = vec![self.number()?];
                loop {
                    let t = self.next();
                    match t.tok {
                        Tok::Punct(',') => out.push(self.number()?),
                        Tok::Punct(')') => return Ok(out),
                        _ => return Err(self.expected("`,` or `)`", &t)),
                    }
                }
            }
            _ => Err(self.expected(
                "a default value: a number, `true`/`false` or `(x, y, …)`",
                &t,
            )),
        }
    }
}

/// A default checked against its type and range; the error completes
/// "the default of `x` …".
fn check_default(
    ty: ValueType,
    v: &[f64],
    min: Option<f64>,
    max: Option<f64>,
) -> Result<Vec<f64>, String> {
    let n = ty.components();
    if v.len() != n {
        return Err(match n {
            1 => format!("must be one value for a {}, not {}", ty.name(), v.len()),
            _ => format!("must be {n} numbers for a {}, not {}", ty.name(), v.len()),
        });
    }
    match ty {
        ValueType::Int if v[0].fract() != 0.0 => {
            return Err("must be a whole number for an int".into())
        }
        ValueType::Int if v[0].abs() > i32::MAX as f64 => return Err("does not fit an int".into()),
        ValueType::Bool if v[0] != 0.0 && v[0] != 1.0 => {
            return Err("must be `true` or `false`".into())
        }
        _ => {}
    }
    if let Some(min) = min {
        if let Some(x) = v.iter().find(|x| **x < min) {
            return Err(format!("{x} is below its range's min {min}"));
        }
    }
    if let Some(max) = max {
        if let Some(x) = v.iter().find(|x| **x > max) {
            return Err(format!("{x} is above its range's max {max}"));
        }
    }
    Ok(v.to_vec())
}

/// With no default, the type's zero pulled into the range.
fn clamp_zero(ty: ValueType, min: Option<f64>, max: Option<f64>) -> Vec<f64> {
    ty.zero()
        .into_iter()
        .map(|x| {
            let x = min.map_or(x, |m| x.max(m));
            max.map_or(x, |m| x.min(m))
        })
        .collect()
}

/// Why a name cannot be a property, or `None` when it can.
fn bad_name(name: &str) -> Option<&'static str> {
    if name.starts_with("fynkel_") || name.starts_with("__") {
        return Some("names starting with `fynkel_` or `__` are the engine's");
    }
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Some("a property name starts with a lowercase letter (`baseColor`, `base_color`)");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some("a property name is ASCII letters, digits and `_`");
    }
    if WGSL_WORDS.contains(&name) {
        return Some("it is a WGSL word");
    }
    None
}

/// WGSL keywords and the type names a property would shadow.
const WGSL_WORDS: &[&str] = &[
    "alias",
    "array",
    "atomic",
    "bitcast",
    "bool",
    "break",
    "case",
    "const",
    "const_assert",
    "continue",
    "continuing",
    "default",
    "diagnostic",
    "discard",
    "else",
    "enable",
    "f16",
    "f32",
    "false",
    "fn",
    "for",
    "i32",
    "if",
    "let",
    "loop",
    "mat2x2",
    "mat3x3",
    "mat4x4",
    "override",
    "ptr",
    "requires",
    "return",
    "sampler",
    "self",
    "struct",
    "switch",
    "true",
    "u32",
    "var",
    "vec2",
    "vec3",
    "vec4",
    "while",
];

#[cfg(test)]
mod tests {
    use super::*;

    const WATER: &str = r#"// A pond.
shader "Examples/Water" {
    lighting lit
    alpha opaque, blend
    properties {
        deep      color    = (0.02, 0.10, 0.20, 1)   "Colour of deep water"
        flow      vec2     = (0.05, 0.02)
        roughness float    = 0.1  range(0, 1)
        steps     int      = 3
        foam      bool     = true
        ripples   texture  = normal
        mask      texture  linear required
    }
}

fn surface(in: SurfaceInput) -> Surface { return default_surface(); }
"#;

    #[test]
    fn a_header_reads_into_its_interface() {
        let s = parse(Some("water.shader"), WATER).expect("parses");
        let i = &s.interface;
        assert_eq!(i.name, "Examples/Water");
        assert_eq!(i.lighting, Lighting::Lit);
        assert_eq!(i.alpha, vec![AlphaMode::Opaque, AlphaMode::Blend]);
        assert!(i.shadows);
        let names: Vec<_> = i.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "deep",
                "flow",
                "roughness",
                "steps",
                "foam",
                "ripples",
                "mask"
            ]
        );
        let deep = i.property("deep").unwrap();
        assert_eq!(
            deep.kind,
            PropertyKind::Value {
                ty: ValueType::Color,
                default: vec![0.02, 0.10, 0.20, 1.0],
                min: None,
                max: None
            }
        );
        assert_eq!(deep.tooltip.as_deref(), Some("Colour of deep water"));
        assert_eq!((deep.line, deep.column), (6, 9));
        assert_eq!(
            i.property("roughness").unwrap().kind,
            PropertyKind::Value {
                ty: ValueType::Float,
                default: vec![0.1],
                min: Some(0.0),
                max: Some(1.0)
            }
        );
        assert_eq!(
            i.property("ripples").unwrap().kind,
            PropertyKind::Texture {
                fallback: Fallback::Normal,
                color_space: ColorSpace::Linear,
                required: false
            }
        );
        assert_eq!(
            i.property("mask").unwrap().kind,
            PropertyKind::Texture {
                fallback: Fallback::White,
                color_space: ColorSpace::Linear,
                required: true
            }
        );
        assert!(s.body().trim_start().starts_with("fn surface"));
    }

    #[test]
    fn an_empty_header_takes_the_defaults() {
        let s = parse(None, "shader \"Plain\" {}\nfn surface(in: SurfaceInput) -> Surface { return default_surface(); }")
            .expect("parses");
        assert_eq!(s.interface.lighting, Lighting::Lit);
        assert_eq!(s.interface.alpha, AlphaMode::ALL.to_vec());
        assert!(s.interface.shadows);
        assert!(s.interface.properties.is_empty());
    }

    #[test]
    fn a_default_left_out_is_zero_or_white_inside_the_range() {
        let s = parse(None, "shader \"A\" {\n properties {\n  a float range(0.5, 1)\n  c color\n  t texture\n }\n}\n").unwrap();
        let i = &s.interface;
        assert!(
            matches!(&i.property("a").unwrap().kind, PropertyKind::Value { default, .. } if default == &vec![0.5])
        );
        assert!(
            matches!(&i.property("c").unwrap().kind, PropertyKind::Value { default, .. } if default == &vec![1.0; 4])
        );
        assert!(matches!(
            i.property("t").unwrap().kind,
            PropertyKind::Texture {
                fallback: Fallback::White,
                color_space: ColorSpace::Srgb,
                ..
            }
        ));
    }

    fn errors(text: &str) -> Vec<(String, u32, u32, String)> {
        parse(Some("x.shader"), text)
            .expect_err("fails")
            .into_iter()
            .map(|d| {
                (
                    d.code.as_str().to_string(),
                    d.line.unwrap_or(0),
                    d.column.unwrap_or(0),
                    d.message,
                )
            })
            .collect()
    }

    #[test]
    fn every_mistake_is_one_diagnostic_at_its_place() {
        let e = errors(
            "shader \"A\" {\n  lighting bright\n  properties {\n    speed floot = 1\n    ok float = 2\n    ok float\n    n int = 1.5\n    r float = 2 range(0, 1)\n    q float range(1, 0)\n    b bool range(0, 1)\n    t texture = red\n    s float srgb\n  }\n  colour red\n}\n",
        );
        let codes: Vec<_> = e
            .iter()
            .map(|(c, l, col, _)| (c.as_str(), *l, *col))
            .collect();
        assert_eq!(
            codes,
            [
                ("FYN-D0315", 2, 12),  // lighting bright
                ("FYN-D0315", 4, 11),  // floot
                ("FYN-D0315", 6, 5),   // ok twice
                ("FYN-D0310", 7, 13),  // 1.5 for an int
                ("FYN-D0310", 8, 15),  // 2 outside 0..1
                ("FYN-D0311", 9, 13),  // range(1, 0)
                ("FYN-D0315", 10, 12), // range on a bool
                ("FYN-D0315", 11, 17), // red is no fallback
                ("FYN-D0315", 12, 13), // srgb on a float
                ("FYN-D0315", 14, 3),  // colour
            ],
            "{e:#?}"
        );
        assert!(e[0].3.contains("`lit` or `unlit`"), "{}", e[0].3);
        assert!(
            e[1].3.contains("unknown property type `floot`"),
            "{}",
            e[1].3
        );
    }

    #[test]
    fn engine_and_wgsl_names_are_refused() {
        let e = errors("shader \"A\" {\n properties {\n  fynkel_x float\n  let float\n  Tint color\n  _x float\n }\n}\n");
        assert_eq!(e.len(), 4, "{e:?}");
        assert!(e[0].3.contains("the engine's"));
        assert!(e[1].3.contains("WGSL word"));
        assert!(e[2].3.contains("lowercase"));
        assert!(e[3].3.contains("lowercase"));
    }

    #[test]
    fn a_file_without_a_header_says_how_one_starts() {
        let e = parse(Some("x.shader"), "fn surface() {}").expect_err("fails");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].code.as_str(), "FYN-D0315");
        assert!(e[0].message.contains("starts with `shader"));
        assert!(e[0].suggestion.is_some());
        let e = errors("shader \"A\" {\n  properties {\n");
        assert!(e.iter().any(|x| x.3.contains("never closed")), "{e:?}");
    }

    #[test]
    fn the_limits_are_errors() {
        let mut text = String::from("shader \"A\" {\n properties {\n");
        for i in 0..9 {
            text.push_str(&format!("  t{i} texture\n"));
        }
        text.push_str(" }\n}\n");
        let e = errors(&text);
        assert_eq!(e.len(), 1);
        assert!(e[0].3.contains("at most 8"), "{}", e[0].3);
        assert_eq!(e[0].1, 11);

        let mut text = String::from("shader \"A\" {\n properties {\n");
        for i in 0..17 {
            text.push_str(&format!("  v{i} vec4\n"));
        }
        text.push_str(" }\n}\n");
        let e = errors(&text);
        assert_eq!(e.len(), 1);
        assert!(e[0].3.contains("272 bytes"), "{}", e[0].3);
        assert!(e[0].3.contains("`v16`"), "{}", e[0].3);
    }

    #[test]
    fn comments_work_in_the_header() {
        let s = parse(
            None,
            "// top\nshader \"A\" { // name\n  /* a block\n  comment */\n  properties {\n    a float = 1 // one\n  }\n}\n",
        )
        .expect("parses");
        assert_eq!(s.interface.properties.len(), 1);
    }
}
