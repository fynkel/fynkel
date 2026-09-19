//! Authoring diagnostics (blueprint §14.2). One record shape for documents,
//! assets, projects and codegen so the `--json` CLI, the editor and agents
//! read the same thing. Codes are stable strings in four namespaces:
//! `FYN-D####` documents, `FYN-A####` assets, `FYN-P####` project,
//! `FYN-G####` codegen. Append only; never renumber.

/// A stable diagnostic code such as `FYN-D0002`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Code(pub &'static str);

impl Code {
    // Documents (FYN-D): strict JSON layer.
    pub const JSON_SYNTAX: Code = Code("FYN-D0001");
    pub const DUPLICATE_KEY: Code = Code("FYN-D0002");
    /// Non-finite, out of range, or an integer beyond 2^53.
    pub const NUMBER_NOT_REPRESENTABLE: Code = Code("FYN-D0003");
    pub const ROOT_NOT_OBJECT: Code = Code("FYN-D0004");
    pub const NESTING_TOO_DEEP: Code = Code("FYN-D0005");
    // Documents (FYN-D): envelope.
    pub const ENVELOPE_MISSING_FIELD: Code = Code("FYN-D0010");
    pub const ENVELOPE_WRONG_TYPE: Code = Code("FYN-D0011");
    pub const MALFORMED_ID: Code = Code("FYN-D0012");
    /// `formatVersion` newer than this tool supports: read-only, never rewritten.
    pub const UNSUPPORTED_FORMAT_VERSION: Code = Code("FYN-D0013");
    pub const SCHEMA_REFERENCE_MISMATCH: Code = Code("FYN-D0014");
    pub const UNKNOWN_DOCUMENT_KIND: Code = Code("FYN-D0015");
    // Documents (FYN-D): JSON Schema.
    pub const SCHEMA_VIOLATION: Code = Code("FYN-D0100");
    pub const SCHEMA_UNAVAILABLE: Code = Code("FYN-D0101");
    // Documents (FYN-D): I/O.
    pub const IO: Code = Code("FYN-D0900");

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// The §14.2 record. Every optional field is absent rather than empty when
/// unknown, so consumers can tell "no pointer" from "root".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    /// Document path as given to the tool (not canonicalised).
    pub document: Option<String>,
    /// RFC 6901 JSON pointer into the document.
    pub pointer: Option<String>,
    /// Persistent object / component / asset id the diagnostic is about.
    pub id: Option<String>,
    /// 1-based, when the source position is known.
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub suggestion: Option<String>,
    pub related: Vec<Diagnostic>,
}

impl Diagnostic {
    pub fn new(code: Code, severity: Severity, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code,
            severity,
            message: message.into(),
            document: None,
            pointer: None,
            id: None,
            line: None,
            column: None,
            suggestion: None,
            related: Vec::new(),
        }
    }

    pub fn error(code: Code, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(code, Severity::Error, message)
    }

    pub fn warning(code: Code, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(code, Severity::Warning, message)
    }

    pub fn with_document(mut self, document: impl Into<String>) -> Diagnostic {
        self.document = Some(document.into());
        self
    }

    pub fn with_pointer(mut self, pointer: impl Into<String>) -> Diagnostic {
        self.pointer = Some(pointer.into());
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Diagnostic {
        self.id = Some(id.into());
        self
    }

    pub fn at(mut self, line: u32, column: u32) -> Diagnostic {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Diagnostic {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn with_related(mut self, related: Diagnostic) -> Diagnostic {
        self.related.push(related);
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// True when any diagnostic is an error.
pub fn has_errors(list: &[Diagnostic]) -> bool {
    list.iter().any(Diagnostic::is_error)
}

impl std::fmt::Display for Diagnostic {
    /// `severity[code] document:line:col pointer: message` — the human
    /// form; the CLI emits JSON for tools.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}[{}]", self.severity.as_str(), self.code.as_str())?;
        if let Some(d) = &self.document {
            write!(f, " {d}")?;
            if let (Some(l), Some(c)) = (self.line, self.column) {
                write!(f, ":{l}:{c}")?;
            }
        }
        if let Some(p) = &self.pointer {
            write!(f, " {}", if p.is_empty() { "/" } else { p })?;
        }
        write!(f, ": {}", self.message)?;
        if let Some(s) = &self.suggestion {
            write!(f, " ({s})")?;
        }
        Ok(())
    }
}
