//! A package's editor part (ADR-0028 §2): its editor code lives under
//! `lib/editor/` and is the only code that may import
//! `package:fynkel_editor_api` or `lib/editor/`. Anything else doing so
//! would reach editor code from the game's entry point and ship it, so
//! [`check_editor_imports`] reports each such directive as
//! `FYN-P0015`. `fynkel validate` (through `validate_project_with`) and
//! `fynkel codegen --check` run it.
//!
//! The scan reads `import`/`export` directives line by line — a directive
//! is one line in formatted Dart — and takes every quoted URI on the line,
//! so a conditional import's alternatives count too.

use std::path::{Component, Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};

/// The package the editor extensions import (ADR-0028).
pub const EDITOR_API_PACKAGE: &str = "fynkel_editor_api";

/// The editor part's directory under `lib/`.
pub const EDITOR_DIR: &str = "editor";

/// `name:` of the package's `pubspec.yaml`; `None` without one.
pub fn package_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("pubspec.yaml")).ok()?;
    text.lines().find_map(|l| {
        let rest = l.strip_prefix("name:")?;
        let name = rest.split('#').next()?.trim().trim_matches(['\'', '"']);
        (!name.is_empty()).then(|| name.to_string())
    })
}

/// Every import of editor code outside `<dir>/lib/editor/`, one error per
/// directive. A directory without `lib/` (or not a Dart package) has none.
pub fn check_editor_imports(dir: &Path) -> Vec<Diagnostic> {
    let lib = dir.join("lib");
    if !lib.is_dir() {
        return Vec::new();
    }
    let own = package_name(dir);
    let editor = lib.join(EDITOR_DIR);
    let mut files = Vec::new();
    dart_files(&lib, &editor, &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("import ") || trimmed.starts_with("export ")) {
                continue;
            }
            for (col, uri) in quoted(line) {
                if !reaches_editor(uri, own.as_deref(), &file, &editor) {
                    continue;
                }
                let rel = file
                    .strip_prefix(dir)
                    .unwrap_or(&file)
                    .display()
                    .to_string();
                out.push(
                    Diagnostic::error(
                        Code::PROJECT_EDITOR_IMPORT,
                        format!(
                            "{rel} imports editor code ('{uri}'): only files under lib/{EDITOR_DIR}/ may, or the game would ship it"
                        ),
                    )
                    .with_document(file.display().to_string())
                    .at(i as u32 + 1, col as u32 + 1)
                    .with_suggestion(format!(
                        "move this file under lib/{EDITOR_DIR}/, or move the editor code it needs out of lib/{EDITOR_DIR}/"
                    )),
                );
            }
        }
    }
    out
}

/// `.dart` files under [dir], leaving out [skip] (the editor part).
fn dart_files(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == skip {
            continue;
        }
        if path.is_dir() {
            dart_files(&path, skip, out);
        } else if path.extension().is_some_and(|e| e == "dart") {
            out.push(path);
        }
    }
}

/// The quoted strings of a line with their byte columns.
fn quoted(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut rest = line;
    let mut offset = 0;
    while let Some(start) = rest.find(['\'', '"']) {
        let q = rest.as_bytes()[start] as char;
        let after = &rest[start + 1..];
        let Some(end) = after.find(q) else {
            break;
        };
        out.push((offset + start, &after[..end]));
        let used = start + 1 + end + 1;
        offset += used;
        rest = &rest[used..];
    }
    out
}

/// Whether [uri], imported from [file], is editor code.
fn reaches_editor(uri: &str, own: Option<&str>, file: &Path, editor: &Path) -> bool {
    if let Some(rest) = uri.strip_prefix("package:") {
        let (package, path) = rest.split_once('/').unwrap_or((rest, ""));
        if package == EDITOR_API_PACKAGE {
            return true;
        }
        return Some(package) == own
            && Path::new(path).components().next() == Some(Component::Normal(EDITOR_DIR.as_ref()));
    }
    if uri.contains(':') {
        return false;
    }
    let Some(parent) = file.parent() else {
        return false;
    };
    normalize(&parent.join(uri)).starts_with(editor)
}

/// `a/b/../c` → `a/c`, without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("fynkel-editor-part-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn package(files: &[(&str, &str)]) -> Scratch {
        let dir = Scratch::new();
        std::fs::write(
            dir.path().join("pubspec.yaml"),
            "name: my_game # the game\n",
        )
        .unwrap();
        for (rel, text) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn found(dir: &Scratch) -> Vec<(String, u32, u32)> {
        check_editor_imports(dir.path())
            .into_iter()
            .map(|d| {
                assert_eq!(d.code, Code::PROJECT_EDITOR_IMPORT);
                let doc = d.document.unwrap();
                let rel = Path::new(&doc)
                    .strip_prefix(dir.path())
                    .unwrap()
                    .display()
                    .to_string();
                (rel, d.line.unwrap(), d.column.unwrap())
            })
            .collect()
    }

    #[test]
    fn the_editor_part_may_import_editor_code() {
        let dir = package(&[
            (
                "lib/editor/editor.dart",
                "import 'package:fynkel_editor_api/fynkel_editor_api.dart';\nimport 'tools.dart';\nimport '../game/coin.dart';\n",
            ),
            ("lib/editor/tools.dart", "import 'package:my_game/editor/editor.dart';\n"),
            ("lib/game/coin.dart", "import 'package:fynkel/fynkel.dart';\nimport 'package:my_game/game/state.dart';\n"),
            ("lib/editorial.dart", "import 'package:my_game/editorial/x.dart';\n"),
            ("test/editor_test.dart", "import 'package:my_game/editor/editor.dart';\n"),
        ]);
        assert_eq!(found(&dir), vec![]);
    }

    #[test]
    fn game_code_importing_editor_code_is_reported() {
        let dir = package(&[
            ("lib/editor/editor.dart", ""),
            (
                "lib/main.dart",
                "// A game.\nimport 'package:flutter/material.dart';\nimport \"package:fynkel_editor_api/fynkel_editor_api.dart\";\n",
            ),
            ("lib/game/a.dart", "export '../editor/editor.dart' show registerEditor;\n"),
            ("lib/game/b.dart", "  import 'package:my_game/editor/editor.dart' as e;\n"),
            (
                "lib/game/c.dart",
                "import 'stub.dart' if (dart.library.io) '../editor/editor.dart';\n",
            ),
        ]);
        assert_eq!(
            found(&dir),
            vec![
                ("lib/game/a.dart".into(), 1, 8),
                ("lib/game/b.dart".into(), 1, 10),
                ("lib/game/c.dart".into(), 1, 41),
                ("lib/main.dart".into(), 3, 8),
            ]
        );
    }

    #[test]
    fn a_directory_without_lib_has_nothing_to_check() {
        let dir = Scratch::new();
        assert!(check_editor_imports(dir.path()).is_empty());
        assert_eq!(package_name(dir.path()), None);
    }
}
