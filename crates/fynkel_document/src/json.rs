//! Strict JSON with source spans (blueprint §6, ADR-0006).
//!
//! Why not `serde_json` alone: authoring diagnostics need a line/column for
//! any JSON pointer, duplicate keys must be rejected (serde silently keeps
//! the last), and unknown data must round-trip byte-for-byte, including
//! number lexemes (`1.0` stays `1.0`). This parser keeps every value's span
//! and number lexeme; [`Node::to_serde`] bridges to `serde_json::Value` for
//! JSON Schema validation.
//!
//! Rejected on purpose: comments, trailing commas, `NaN`/`Infinity`, leading
//! zeros, single quotes, unescaped control characters, lone surrogates, a
//! BOM, non-object roots, nesting deeper than [`MAX_DEPTH`], integers beyond
//! 2^53 (they would not survive a Dart `double` or a JS reader).

use fynkel_contracts::{Code, Diagnostic};

/// Maximum nesting (§7 import limits apply to documents too).
pub const MAX_DEPTH: usize = 128;

/// 1-based source position.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pos {
    pub line: u32,
    pub column: u32,
}

/// Start of the value and its key (for object members).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    /// The original lexeme and its parsed value; the lexeme is what is
    /// written back so unknown data stays byte-identical.
    Number {
        lexeme: String,
        value: f64,
    },
    String(String),
    Array(Vec<Node>),
    /// Members in document order.
    Object(Vec<Member>),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Member {
    pub key: String,
    pub key_span: Span,
    pub value: Node,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    pub span: Span,
    pub value: Value,
}

impl Node {
    /// A node with no source position (programmatic values).
    pub fn synthetic(value: Value) -> Node {
        Node {
            span: Span::default(),
            value,
        }
    }

    pub fn string(s: impl Into<String>) -> Node {
        Node::synthetic(Value::String(s.into()))
    }

    pub fn integer(v: i64) -> Node {
        Node::synthetic(Value::Number {
            lexeme: v.to_string(),
            value: v as f64,
        })
    }

    pub fn float(v: f64) -> Node {
        Node::synthetic(Value::Number {
            lexeme: canonical_float(v),
            value: v,
        })
    }

    pub fn object(members: Vec<(String, Node)>) -> Node {
        Node::synthetic(Value::Object(
            members
                .into_iter()
                .map(|(key, value)| Member {
                    key,
                    key_span: Span::default(),
                    value,
                })
                .collect(),
        ))
    }

    pub fn as_object(&self) -> Option<&[Member]> {
        match &self.value {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Vec<Member>> {
        match &mut self.value {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Node]> {
        match &self.value {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match &self.value {
            Value::Number { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Integer value when the number is integral (any lexeme form).
    pub fn as_i64(&self) -> Option<i64> {
        let v = self.as_f64()?;
        (v.fract() == 0.0 && v.abs() <= 9007199254740992.0).then_some(v as i64)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match &self.value {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Object member by key.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.as_object()?
            .iter()
            .find(|m| m.key == key)
            .map(|m| &m.value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        self.as_object_mut()?
            .iter_mut()
            .find(|m| m.key == key)
            .map(|m| &mut m.value)
    }

    /// Sets (or appends) an object member. No-op on non-objects.
    pub fn set(&mut self, key: &str, value: Node) {
        if let Some(members) = self.as_object_mut() {
            if let Some(m) = members.iter_mut().find(|m| m.key == key) {
                m.value = value;
            } else {
                members.push(Member {
                    key: key.to_string(),
                    key_span: Span::default(),
                    value,
                });
            }
        }
    }

    /// RFC 6901 lookup (`""` is the root; `~1` → `/`, `~0` → `~`).
    pub fn pointer(&self, pointer: &str) -> Option<&Node> {
        if pointer.is_empty() {
            return Some(self);
        }
        let mut cur = self;
        for raw in pointer.strip_prefix('/')?.split('/') {
            let token = raw.replace("~1", "/").replace("~0", "~");
            cur = match &cur.value {
                Value::Object(members) => &members.iter().find(|m| m.key == token)?.value,
                Value::Array(items) => {
                    if token != "0" && token.starts_with('0') {
                        return None;
                    }
                    items.get(token.parse::<usize>().ok()?)?
                }
                _ => return None,
            };
        }
        Some(cur)
    }

    /// Mutable RFC 6901 lookup.
    pub fn pointer_mut(&mut self, pointer: &str) -> Option<&mut Node> {
        if pointer.is_empty() {
            return Some(self);
        }
        let mut cur = self;
        for raw in pointer.strip_prefix('/')?.split('/') {
            let token = raw.replace("~1", "/").replace("~0", "~");
            cur = match &mut cur.value {
                Value::Object(members) => &mut members.iter_mut().find(|m| m.key == token)?.value,
                Value::Array(items) => {
                    if token != "0" && token.starts_with('0') {
                        return None;
                    }
                    items.get_mut(token.parse::<usize>().ok()?)?
                }
                _ => return None,
            };
        }
        Some(cur)
    }

    /// Like [`Node::pointer`] but returns the key's span for object members
    /// (better anchor for "unexpected property" diagnostics).
    pub fn pointer_span(&self, pointer: &str) -> Option<Span> {
        if pointer.is_empty() {
            return Some(self.span);
        }
        let (parent, last) = pointer.rsplit_once('/')?;
        let parent = self.pointer(parent)?;
        let token = last.replace("~1", "/").replace("~0", "~");
        match &parent.value {
            Value::Object(members) => members.iter().find(|m| m.key == token).map(|m| m.key_span),
            _ => self.pointer(pointer).map(|n| n.span),
        }
    }

    /// Bridge to `serde_json` (for JSON Schema validation). Integral
    /// lexemes become integers so `"type": "integer"` holds.
    pub fn to_serde(&self) -> serde_json::Value {
        match &self.value {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Number { lexeme, value } => {
                if !lexeme.contains(['.', 'e', 'E']) {
                    if let Ok(i) = lexeme.parse::<i64>() {
                        return serde_json::Value::from(i);
                    }
                }
                serde_json::Number::from_f64(*value)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            }
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(Node::to_serde).collect())
            }
            Value::Object(members) => serde_json::Value::Object(
                members
                    .iter()
                    .map(|m| (m.key.clone(), m.value.to_serde()))
                    .collect(),
            ),
        }
    }

    /// From `serde_json` (programmatic construction); numbers get canonical
    /// lexemes, no spans.
    pub fn from_serde(v: &serde_json::Value) -> Node {
        match v {
            serde_json::Value::Null => Node::synthetic(Value::Null),
            serde_json::Value::Bool(b) => Node::synthetic(Value::Bool(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Node::integer(i)
                } else {
                    Node::float(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => Node::string(s.clone()),
            serde_json::Value::Array(items) => {
                Node::synthetic(Value::Array(items.iter().map(Node::from_serde).collect()))
            }
            serde_json::Value::Object(map) => Node::object(
                map.iter()
                    .map(|(k, v)| (k.clone(), Node::from_serde(v)))
                    .collect(),
            ),
        }
    }
}

/// Shortest round-trip representation, always with a fraction or exponent
/// so it is not mistaken for an integer.
pub fn canonical_float(v: f64) -> String {
    let s = format!("{v}");
    if s.contains(['.', 'e', 'E', 'n', 'i']) {
        s
    } else {
        format!("{s}.0")
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parses strict JSON. The root must be an object. Returns the first error
/// only: a syntax error makes everything after it meaningless.
pub fn parse(text: &str) -> Result<Node, Diagnostic> {
    let mut p = Parser {
        bytes: text.as_bytes(),
        i: 0,
        line: 1,
        col: 1,
        depth: 0,
    };
    if p.bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(p.err(Code::JSON_SYNTAX, "byte order mark is not allowed"));
    }
    p.ws();
    let root = p.value()?;
    p.ws();
    if p.i < p.bytes.len() {
        return Err(p.err(Code::JSON_SYNTAX, "unexpected content after the document"));
    }
    if !matches!(root.value, Value::Object(_)) {
        return Err(Diagnostic::error(
            Code::ROOT_NOT_OBJECT,
            "the document root must be an object",
        )
        .with_pointer("")
        .at(root.span.start.line, root.span.start.column));
    }
    Ok(root)
}

struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
    line: u32,
    col: u32,
    depth: usize,
}

impl Parser<'_> {
    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            column: self.col,
        }
    }

    fn err(&self, code: Code, msg: impl Into<String>) -> Diagnostic {
        Diagnostic::error(code, msg).at(self.line, self.col)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    /// Advances one byte; columns count characters, so continuation bytes
    /// do not advance the column.
    fn bump(&mut self) {
        let b = self.bytes[self.i];
        self.i += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else if b & 0xC0 != 0x80 {
            self.col += 1;
        }
    }

    fn ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), Diagnostic> {
        if self.peek() == Some(b) {
            self.bump();
            Ok(())
        } else {
            Err(self.unexpected(&format!("expected `{}`", b as char)))
        }
    }

    fn unexpected(&self, what: &str) -> Diagnostic {
        match self.peek() {
            None => self.err(Code::JSON_SYNTAX, format!("{what}, found end of input")),
            Some(b) => {
                let shown = if b.is_ascii_graphic() || b == b' ' {
                    format!("`{}`", b as char)
                } else {
                    format!("byte 0x{b:02x}")
                };
                self.err(Code::JSON_SYNTAX, format!("{what}, found {shown}"))
            }
        }
    }

    fn value(&mut self) -> Result<Node, Diagnostic> {
        let start = self.pos();
        let value = match self.peek() {
            Some(b'{') => self.object()?,
            Some(b'[') => self.array()?,
            Some(b'"') => Value::String(self.string()?),
            Some(b't') => self.literal("true", Value::Bool(true))?,
            Some(b'f') => self.literal("false", Value::Bool(false))?,
            Some(b'n') => self.literal("null", Value::Null)?,
            Some(b'-') | Some(b'0'..=b'9') => self.number()?,
            Some(b'\'') => {
                return Err(self.err(
                    Code::JSON_SYNTAX,
                    "single-quoted strings are not JSON; use double quotes",
                ))
            }
            Some(b'/') => return Err(self.err(Code::JSON_SYNTAX, "comments are not allowed")),
            Some(b'N') | Some(b'I') => {
                return Err(self.err(
                    Code::NUMBER_NOT_REPRESENTABLE,
                    "NaN and Infinity are not JSON numbers",
                ))
            }
            _ => return Err(self.unexpected("expected a value")),
        };
        Ok(Node {
            span: Span {
                start,
                end: self.pos(),
            },
            value,
        })
    }

    fn enter(&mut self) -> Result<(), Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(
                Code::NESTING_TOO_DEEP,
                format!("nesting deeper than {MAX_DEPTH} levels"),
            ));
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Value, Diagnostic> {
        self.enter()?;
        self.bump(); // {
        let mut members: Vec<Member> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.bump();
            self.depth -= 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.unexpected("expected a string key"));
            }
            let key_start = self.pos();
            let key = self.string()?;
            let key_span = Span {
                start: key_start,
                end: self.pos(),
            };
            if members.iter().any(|m| m.key == key) {
                return Err(Diagnostic::error(
                    Code::DUPLICATE_KEY,
                    format!("duplicate key {key:?}"),
                )
                .at(key_start.line, key_start.column)
                .with_suggestion("keep one of the two members"));
            }
            self.ws();
            self.expect(b':')?;
            self.ws();
            let value = self.value()?;
            members.push(Member {
                key,
                key_span,
                value,
            });
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.ws();
                    if self.peek() == Some(b'}') {
                        return Err(self.err(Code::JSON_SYNTAX, "trailing comma before `}`"));
                    }
                }
                Some(b'}') => {
                    self.bump();
                    self.depth -= 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err(self.unexpected("expected `,` or `}`")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, Diagnostic> {
        self.enter()?;
        self.bump(); // [
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.bump();
            self.depth -= 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.ws();
                    if self.peek() == Some(b']') {
                        return Err(self.err(Code::JSON_SYNTAX, "trailing comma before `]`"));
                    }
                }
                Some(b']') => {
                    self.bump();
                    self.depth -= 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.unexpected("expected `,` or `]`")),
            }
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, Diagnostic> {
        if self.bytes[self.i..].starts_with(word.as_bytes()) {
            for _ in 0..word.len() {
                self.bump();
            }
            Ok(value)
        } else {
            Err(self.unexpected("expected a value"))
        }
    }

    fn number(&mut self) -> Result<Value, Diagnostic> {
        let start_i = self.i;
        let start = self.pos();
        if self.peek() == Some(b'-') {
            self.bump();
        }
        match self.peek() {
            Some(b'0') => {
                self.bump();
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.err(Code::JSON_SYNTAX, "leading zeros are not allowed"));
                }
            }
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.unexpected("expected a digit")),
        }
        let mut integral = true;
        if self.peek() == Some(b'.') {
            integral = false;
            self.bump();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.unexpected("expected a digit after `.`"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            integral = false;
            self.bump();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.unexpected("expected a digit in the exponent"));
            }
            self.digits();
        }
        let lexeme = std::str::from_utf8(&self.bytes[start_i..self.i])
            .expect("ascii")
            .to_string();
        let value: f64 = lexeme.parse().map_err(|_| {
            Diagnostic::error(Code::JSON_SYNTAX, format!("invalid number {lexeme}"))
                .at(start.line, start.column)
        })?;
        if !value.is_finite() {
            return Err(Diagnostic::error(
                Code::NUMBER_NOT_REPRESENTABLE,
                format!("{lexeme} is out of range for a double"),
            )
            .at(start.line, start.column));
        }
        let unsafe_integer = integral
            && lexeme
                .parse::<i128>()
                .map(|i| i.abs() > 9007199254740992)
                .unwrap_or(true);
        if unsafe_integer {
            return Err(Diagnostic::error(
                Code::NUMBER_NOT_REPRESENTABLE,
                format!("integer {lexeme} exceeds 2^53 and would lose precision"),
            )
            .at(start.line, start.column)
            .with_suggestion("store large identifiers as strings"));
        }
        Ok(Value::Number { lexeme, value })
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
    }

    fn string(&mut self) -> Result<String, Diagnostic> {
        self.bump(); // "
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.err(Code::JSON_SYNTAX, "unterminated string"));
            };
            match b {
                b'"' => {
                    self.bump();
                    return Ok(out);
                }
                b'\\' => {
                    self.bump();
                    let Some(e) = self.peek() else {
                        return Err(self.err(Code::JSON_SYNTAX, "unterminated escape"));
                    };
                    self.bump();
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&hi) {
                                if !self.bytes[self.i..].starts_with(b"\\u") {
                                    return Err(self.err(Code::JSON_SYNTAX, "lone high surrogate"));
                                }
                                self.bump();
                                self.bump();
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(
                                        self.err(Code::JSON_SYNTAX, "invalid low surrogate")
                                    );
                                }
                                char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(self.err(Code::JSON_SYNTAX, "lone low surrogate"));
                            } else {
                                char::from_u32(hi)
                            };
                            out.push(ch.ok_or_else(|| {
                                self.err(Code::JSON_SYNTAX, "invalid code point")
                            })?);
                        }
                        other => {
                            return Err(self.err(
                                Code::JSON_SYNTAX,
                                format!("invalid escape `\\{}`", other as char),
                            ))
                        }
                    }
                }
                0x00..=0x1F => {
                    return Err(self.err(
                        Code::JSON_SYNTAX,
                        "control characters must be escaped inside strings",
                    ))
                }
                _ => {
                    // Copy one UTF-8 scalar (input is &str, so it is valid).
                    let s = &self.bytes[self.i..];
                    let len = utf8_len(s[0]);
                    out.push_str(std::str::from_utf8(&s[..len]).expect("valid utf-8"));
                    for _ in 0..len {
                        self.bump();
                    }
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, Diagnostic> {
        let mut v = 0u32;
        for _ in 0..4 {
            let Some(b) = self.peek() else {
                return Err(self.err(Code::JSON_SYNTAX, "truncated \\u escape"));
            };
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| self.err(Code::JSON_SYNTAX, "invalid hex digit in \\u escape"))?;
            v = v * 16 + d;
            self.bump();
        }
        Ok(v)
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

// ---------------------------------------------------------------------------
// Canonical writer
// ---------------------------------------------------------------------------

/// Canonical form (§6): two-space indent, one member or item per line,
/// `{}`/`[]` for empties, minimal string escaping, number lexemes as
/// parsed, LF line endings, trailing newline. Key order is the node's
/// member order (callers hoist envelope keys; see `Document::write`).
pub fn write(node: &Node) -> String {
    let mut out = String::new();
    write_node(node, 0, &mut out);
    out.push('\n');
    out
}

fn write_node(node: &Node, indent: usize, out: &mut String) {
    match &node.value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number { lexeme, .. } => out.push_str(lexeme),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                push_indent(indent + 1, out);
                write_node(item, indent + 1, out);
            }
            out.push('\n');
            push_indent(indent, out);
            out.push(']');
        }
        Value::Object(members) => {
            if members.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, m) in members.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                push_indent(indent + 1, out);
                write_string(&m.key, out);
                out.push_str(": ");
                write_node(&m.value, indent + 1, out);
            }
            out.push('\n');
            push_indent(indent, out);
            out.push('}');
        }
    }
}

fn push_indent(n: usize, out: &mut String) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_pointers() {
        let n = parse("{\n  \"a\": [1, {\"b\": 2.50}],\n  \"c\": \"x\"\n}\n").unwrap();
        let b = n.pointer("/a/1/b").unwrap();
        assert_eq!(
            b.span.start,
            Pos {
                line: 2,
                column: 18
            }
        );
        assert!(matches!(&b.value, Value::Number { lexeme, .. } if lexeme == "2.50"));
        assert_eq!(
            n.pointer_span("/c").unwrap().start,
            Pos { line: 3, column: 3 }
        );
        assert!(n.pointer("/a/01").is_none());
        assert!(n.pointer("/zz").is_none());
    }

    #[test]
    fn rejections() {
        let cases = [
            ("{\"a\":1,\"a\":2}", Code::DUPLICATE_KEY),
            ("{\"a\":1,}", Code::JSON_SYNTAX),
            ("{\"a\":01}", Code::JSON_SYNTAX),
            ("{\"a\":NaN}", Code::NUMBER_NOT_REPRESENTABLE),
            ("{\"a\":1e999}", Code::NUMBER_NOT_REPRESENTABLE),
            ("{\"a\":9007199254740993}", Code::NUMBER_NOT_REPRESENTABLE),
            ("[1]", Code::ROOT_NOT_OBJECT),
            ("{\"a\":\"\t\"}", Code::JSON_SYNTAX),
            ("{\"a\":\"\\ud800\"}", Code::JSON_SYNTAX),
            ("{} //", Code::JSON_SYNTAX),
            ("\u{feff}{}", Code::JSON_SYNTAX),
        ];
        for (text, code) in cases {
            let e = parse(text).unwrap_err();
            assert_eq!(e.code, code, "{text}: {e}");
        }
        let deep = format!(
            "{}{}",
            "{\"a\":".repeat(130),
            "1".to_string() + &"}".repeat(130)
        );
        assert_eq!(parse(&deep).unwrap_err().code, Code::NESTING_TOO_DEEP);
    }

    #[test]
    fn canonical_is_a_fixed_point_and_keeps_lexemes() {
        let src =
            "{\"n\": 1.0, \"e\": 1e3, \"s\": \"a\\u00e9\\n\", \"o\": {}, \"l\": [true, null]}";
        let once = write(&parse(src).unwrap());
        assert_eq!(
            once,
            "{\n  \"n\": 1.0,\n  \"e\": 1e3,\n  \"s\": \"aé\\n\",\n  \"o\": {},\n  \"l\": [\n    true,\n    null\n  ]\n}\n"
        );
        assert_eq!(write(&parse(&once).unwrap()), once);
    }

    #[test]
    fn serde_bridge_types() {
        let n = parse("{\"i\": 3, \"f\": 3.0, \"big\": 1e20}")
            .unwrap()
            .to_serde();
        assert!(n["i"].is_i64());
        assert!(n["f"].is_f64());
        assert!(n["big"].is_f64());
    }
}
