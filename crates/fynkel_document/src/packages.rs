//! The package graph (ADR-0029 §1): the packages a game reaches through
//! its pubspec `dependencies` (never `dev_dependencies`), followed through
//! each package's own pubspec and located by the project's resolved
//! `.dart_tool/package_config.json`. The CLI (the editor host's parts,
//! ADR-0028), the authoring session and the cook read the same graph.
//!
//! A package with a [`PACKAGE_FILE`] at its root is a Fynkel package: its
//! scripts, data types and assets are the game's too (ADR-0029 §2). Where
//! a package comes from (`pubspec.lock`'s `source`) decides whether its
//! assets can be edited: a `path` package is the user's own, a hosted or
//! git one lives in pub's shared cache.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};
use serde_json::Value;

/// The file that makes a Dart package a Fynkel package.
pub const PACKAGE_FILE: &str = "fynkel.package.json";

/// The one `formatVersion` of [`PACKAGE_FILE`] so far.
pub const PACKAGE_FORMAT_VERSION: u64 = 1;

/// Where a package comes from, as `pubspec.lock` records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The game itself.
    Game,
    /// `path:` — a package on the user's disk, theirs to edit.
    Path,
    /// pub.dev or another pub server; in pub's shared cache.
    Hosted,
    /// `git:`; in pub's shared cache.
    Git,
    /// `sdk: flutter` and the like.
    Sdk,
}

/// What a package's [`PACKAGE_FILE`] says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    /// The asset folder, relative to the package root (`"assets"`).
    pub assets: String,
    /// The prefix of the package's type ids (the package name).
    pub namespace: String,
}

/// One package of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub root: PathBuf,
    pub source: Source,
    /// `Some` for a Fynkel package (one with a valid [`PACKAGE_FILE`]).
    pub manifest: Option<PackageManifest>,
}

impl Package {
    /// Whether its files are the user's to change: the game and `path`
    /// packages. Hosted, git and SDK packages are pub's (ADR-0029 §2).
    pub fn editable(&self) -> bool {
        matches!(self.source, Source::Game | Source::Path)
    }

    /// Whether it has an editor part, `lib/editor/editor.dart` (ADR-0028).
    pub fn has_editor_part(&self) -> bool {
        self.root
            .join("lib")
            .join(crate::editor_part::EDITOR_DIR)
            .join("editor.dart")
            .is_file()
    }

    /// A Fynkel package's asset folder; `None` for any other package.
    pub fn asset_root(&self) -> Option<PathBuf> {
        self.manifest.as_ref().map(|m| self.root.join(&m.assets))
    }
}

/// The game's package graph.
#[derive(Debug, Clone, Default)]
pub struct PackageGraph {
    /// Every package the game reaches, dependencies before their
    /// dependents, otherwise by name; the game last.
    pub packages: Vec<Package>,
    /// `FYN-P0019` for each [`PACKAGE_FILE`] that could not be read.
    pub diagnostics: Vec<Diagnostic>,
}

impl PackageGraph {
    /// The game's dependencies that are Fynkel packages, in graph order.
    pub fn fynkel_packages(&self) -> impl Iterator<Item = &Package> {
        self.packages
            .iter()
            .filter(|p| p.source != Source::Game && p.manifest.is_some())
    }

    /// The packages with an editor part, in the order their
    /// `registerEditor` runs (ADR-0028).
    pub fn editor_parts(&self) -> Vec<String> {
        self.packages
            .iter()
            .filter(|p| p.has_editor_part())
            .map(|p| p.name.clone())
            .collect()
    }
}

/// The package graph of the game at `project`, whose package is
/// `package`. Without a resolved `.dart_tool/package_config.json` (before
/// `flutter pub get`), only the game.
pub fn package_graph(project: &Path, package: &str) -> PackageGraph {
    let roots = package_roots(project);
    let sources = lock_sources(project);
    // Every package the game reaches, with its own dependencies.
    let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stack: Vec<String> = pubspec_dependencies(project)
        .into_iter()
        .filter(|d| roots.contains_key(d))
        .collect();
    while let Some(name) = stack.pop() {
        if name == package || deps.contains_key(&name) {
            continue;
        }
        let own: Vec<String> = pubspec_dependencies(&roots[&name])
            .into_iter()
            .filter(|d| roots.contains_key(d) && *d != package)
            .collect();
        stack.extend(own.iter().cloned());
        deps.insert(name, own);
    }
    // Dependencies first; among the packages ready, the first by name. A
    // cycle (pub allows none among regular dependencies) falls back to
    // the name order.
    let mut order = Vec::new();
    let mut left = deps;
    while !left.is_empty() {
        let next = left
            .iter()
            .find(|(_, d)| d.iter().all(|d| !left.contains_key(d)))
            .or_else(|| left.iter().next())
            .map(|(n, _)| n.clone())
            .expect("not empty");
        left.remove(&next);
        order.push(next);
    }

    let mut graph = PackageGraph::default();
    for name in order {
        let root = roots[&name].clone();
        let source = sources
            .get(&name)
            .copied()
            .unwrap_or_else(|| source_by_location(&root));
        let manifest = read_manifest(&name, &root, &mut graph.diagnostics);
        graph.packages.push(Package {
            name,
            root,
            source,
            manifest,
        });
    }
    graph.packages.push(Package {
        name: package.to_string(),
        root: project.to_path_buf(),
        source: Source::Game,
        manifest: None,
    });
    graph
}

/// A package's [`PACKAGE_FILE`]: `None` when it has none, or (with a
/// `FYN-P0019`) when it cannot be read.
fn read_manifest(
    name: &str,
    root: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PackageManifest> {
    let path = root.join(PACKAGE_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let document = format!("{name}/{PACKAGE_FILE}");
    let mut fail = |pointer: &str, message: String| {
        let d = Diagnostic::error(Code::PACKAGE_MANIFEST, message).with_document(document.clone());
        diagnostics.push(if pointer.is_empty() {
            d
        } else {
            d.with_pointer(pointer)
        });
        None
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            return fail(
                "",
                format!("{PACKAGE_FILE} of package '{name}' is not JSON: {e}"),
            )
        }
    };
    let Some(object) = value.as_object() else {
        return fail(
            "",
            format!("{PACKAGE_FILE} of package '{name}' is not a JSON object"),
        );
    };
    for key in object.keys() {
        if !matches!(key.as_str(), "formatVersion" | "assets" | "namespace") {
            return fail(
                &format!("/{key}"),
                format!("'{key}' is not a {PACKAGE_FILE} field (formatVersion, assets, namespace)"),
            );
        }
    }
    match object.get("formatVersion").and_then(Value::as_u64) {
        Some(PACKAGE_FORMAT_VERSION) => {}
        Some(v) => {
            return fail(
                "/formatVersion",
                format!(
                    "formatVersion {v} is newer than this Fynkel reads ({PACKAGE_FORMAT_VERSION})"
                ),
            )
        }
        None => {
            return fail(
                "/formatVersion",
                format!("formatVersion must be {PACKAGE_FORMAT_VERSION}"),
            )
        }
    }
    let assets = match object.get("assets") {
        None => "assets".to_string(),
        Some(Value::String(s)) if inside(s) => s.trim_end_matches('/').to_string(),
        Some(_) => {
            return fail(
                "/assets",
                "assets must be a relative folder inside the package".to_string(),
            )
        }
    };
    let namespace = match object.get("namespace") {
        None => name.to_string(),
        Some(Value::String(s)) if is_identifier(s) => s.clone(),
        Some(_) => {
            return fail(
                "/namespace",
                "namespace must be a lower-case identifier (letters, digits, '_')".to_string(),
            )
        }
    };
    if RESERVED_NAMESPACES.contains(&namespace.as_str()) {
        return fail(
            "/namespace",
            format!(
                "namespace '{namespace}' is the engine's or the game's; name the package's own"
            ),
        );
    }
    Some(PackageManifest { assets, namespace })
}

/// Namespaces no package may take: the engine's types and the game's.
pub const RESERVED_NAMESPACES: [&str; 2] = ["fynkel", "game"];

/// What a game's Fynkel packages add to it (ADR-0029 §2): their schemas
/// and the type ids those declare, with the graph's diagnostics and the
/// type checks — `FYN-P0016` for a type id two packages (or a package and
/// the game) declare, `FYN-P0017` for a package's type outside its
/// namespace.
#[derive(Debug, Clone, Default)]
pub struct PackageTypes {
    /// The Fynkel packages among the game's dependencies, in graph order.
    pub packages: Vec<Package>,
    /// Every behaviour and data type id they declare.
    pub types: BTreeSet<String>,
    pub diagnostics: Vec<Diagnostic>,
}

impl PackageTypes {
    /// Each package's `schemas/`, in graph order.
    pub fn schema_dirs(&self) -> Vec<PathBuf> {
        self.packages
            .iter()
            .map(|p| p.root.join(crate::runtime::PROJECT_SCHEMAS_DIR))
            .collect()
    }
}

/// Every Fynkel package's assets as one database of entries mounted at
/// `Packages/<name>/` (ADR-0029 §2), in graph order, with their scans'
/// diagnostics. A package without its asset folder adds nothing.
#[cfg(feature = "validate")]
pub fn package_assets(
    types: &PackageTypes,
    schemas: &crate::schema::SchemaSet,
) -> (crate::asset::AssetDatabase, Vec<Diagnostic>) {
    let mut db = crate::asset::AssetDatabase::default();
    let mut diags = Vec::new();
    for package in &types.packages {
        let Some(dir) = package.asset_root().filter(|d| d.is_dir()) else {
            continue;
        };
        let (scanned, scan) = crate::asset::AssetDatabase::scan(&dir, schemas);
        diags.extend(scan);
        diags.extend(db.mount(&package.name, &scanned));
    }
    (db, diags)
}

/// The package a mounted asset path (`Packages/<name>/…`) belongs to, or
/// `None` for the project's own.
pub fn package_of(path: &str) -> Option<&str> {
    let rest = path
        .strip_prefix(crate::asset::PACKAGES_MOUNT)?
        .strip_prefix('/')?;
    rest.split('/').next().filter(|n| !n.is_empty())
}

/// [`PackageTypes`] of the game at `project` (its pubspec names the
/// package; without one there are none).
pub fn package_types(project: &Path) -> PackageTypes {
    let Some(game) = crate::editor_part::package_name(project) else {
        return PackageTypes::default();
    };
    let graph = package_graph(project, &game);
    let mut out = PackageTypes {
        diagnostics: graph.diagnostics.clone(),
        ..PackageTypes::default()
    };
    // Type id → the package that declared it first.
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for package in graph.fynkel_packages() {
        let namespace = &package
            .manifest
            .as_ref()
            .expect("a Fynkel package")
            .namespace;
        for (file, id) in declared_types(&package.root) {
            let document = format!("{}/{}", package.name, file);
            if !id.starts_with(&format!("{namespace}.")) {
                out.diagnostics.push(outside_namespace(
                    &id,
                    &package.name,
                    namespace,
                    document.clone(),
                ));
            }
            clash(
                &mut owner,
                &id,
                &package.name,
                document,
                &mut out.diagnostics,
            );
            out.types.insert(id);
        }
        out.packages.push(package.clone());
    }
    for (file, id) in declared_types(project) {
        clash(&mut owner, &id, &game, file, &mut out.diagnostics);
    }
    out
}

/// The [`PACKAGE_FILE`] of the package at `dir` itself (for `fynkel
/// codegen` run in a package); `None` when it has none or it cannot be
/// read.
pub fn own_manifest(dir: &Path) -> Option<PackageManifest> {
    let name = crate::editor_part::package_name(dir)?;
    read_manifest(&name, dir, &mut Vec::new())
}

/// A Fynkel package's own types outside its namespace (`FYN-P0017`), for
/// `fynkel codegen` run in the package itself. Nothing for a directory
/// that is not a Fynkel package.
pub fn check_own_namespace(dir: &Path) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let Some(name) = crate::editor_part::package_name(dir) else {
        return out;
    };
    let Some(manifest) = read_manifest(&name, dir, &mut out) else {
        return out;
    };
    for (file, id) in declared_types(dir) {
        if !id.starts_with(&format!("{}.", manifest.namespace)) {
            out.push(outside_namespace(&id, &name, &manifest.namespace, file));
        }
    }
    out
}

fn outside_namespace(id: &str, package: &str, namespace: &str, document: String) -> Diagnostic {
    Diagnostic::error(
        Code::PACKAGE_TYPE_NAMESPACE,
        format!("type '{id}' of package '{package}' is outside its namespace '{namespace}'"),
    )
    .with_document(document)
    .with_suggestion(format!(
        "give it an id starting with '{namespace}.' (@FynkelBehaviour(id: '{namespace}.…'))"
    ))
}

fn clash(
    owner: &mut BTreeMap<String, String>,
    id: &str,
    package: &str,
    document: String,
    out: &mut Vec<Diagnostic>,
) {
    match owner.get(id) {
        Some(first) if first != package => out.push(
            Diagnostic::error(
                Code::PACKAGE_TYPE_CLASH,
                format!("type '{id}' is declared by both '{first}' and '{package}'"),
            )
            .with_document(document),
        ),
        Some(_) => {}
        None => {
            owner.insert(id.to_string(), package.to_string());
        }
    }
}

/// `(file relative to root, type id)` of every behaviour and data schema
/// under `<root>/schemas`, sorted by file.
fn declared_types(root: &Path) -> Vec<(String, String)> {
    let dir = root.join(crate::runtime::PROJECT_SCHEMAS_DIR);
    let mut files = Vec::new();
    schema_files(&dir, &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let Some(v) = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue; // the schema set reports it
        };
        let kind = v.pointer("/x-fynkel/kind").and_then(Value::as_str);
        if !matches!(kind, Some("behaviour") | Some("data")) {
            continue;
        }
        let Some(id) = v.get("title").and_then(Value::as_str) else {
            continue;
        };
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, id.to_string()));
    }
    out
}

fn schema_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            schema_files(&path, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".schema.json"))
        {
            out.push(path);
        }
    }
}

/// A non-empty relative path that stays inside the package.
fn inside(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Without a `pubspec.lock` entry: a package inside pub's cache is
/// hosted, anything else the user's own.
fn source_by_location(root: &Path) -> Source {
    let cache = std::env::var_os("PUB_CACHE").map(PathBuf::from);
    let in_cache = cache.as_deref().is_some_and(|c| root.starts_with(c))
        || root.components().any(|c| c.as_os_str() == ".pub-cache");
    if in_cache {
        Source::Hosted
    } else {
        Source::Path
    }
}

/// Each package's `source:` from the project's `pubspec.lock` (a line
/// reader, as for the pubspec).
fn lock_sources(project: &Path) -> BTreeMap<String, Source> {
    let Ok(text) = std::fs::read_to_string(project.join("pubspec.lock")) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    let mut inside = false;
    let mut current: Option<String> = None;
    for line in text.lines() {
        let content = line.split('#').next().unwrap_or("").trim_end();
        if content.trim().is_empty() {
            continue;
        }
        let depth = content.len() - content.trim_start().len();
        if depth == 0 {
            inside = content == "packages:";
            current = None;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((key, value)) = content.trim().split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches(['\'', '"']);
        if depth == 2 {
            current = Some(key.to_string());
        } else if depth == 4 && key == "source" {
            let source = match value.trim().trim_matches(['\'', '"']) {
                "path" => Source::Path,
                "hosted" => Source::Hosted,
                "git" => Source::Git,
                "sdk" => Source::Sdk,
                _ => continue,
            };
            if let Some(name) = &current {
                out.insert(name.clone(), source);
            }
        }
    }
    out
}

/// Each package's root directory, from the project's resolved
/// `.dart_tool/package_config.json` (empty without one).
fn package_roots(project: &Path) -> BTreeMap<String, PathBuf> {
    let config = project.join(".dart_tool").join("package_config.json");
    let Some(v) = std::fs::read_to_string(&config)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return BTreeMap::new();
    };
    let base = project.join(".dart_tool");
    let mut out = BTreeMap::new();
    for p in v["packages"].as_array().into_iter().flatten() {
        let (Some(name), Some(uri)) = (p["name"].as_str(), p["rootUri"].as_str()) else {
            continue;
        };
        let root = match uri.strip_prefix("file://") {
            Some(abs) => PathBuf::from(percent_decode(abs)),
            None if uri.contains(':') => continue,
            None => base.join(percent_decode(uri)),
        };
        out.insert(name.to_string(), normalize(&root));
    }
    out
}

/// `a/.dart_tool/../../b` → `b`, without touching the disk (a root need
/// not exist yet, and symlinks stay as the user made them).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// `%20` → ` ` (a package_config URI's escapes).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The keys of a `pubspec.yaml`'s top-level `dependencies:` block (a line
/// reader: pubspecs are plain enough, and there is no YAML parser here).
fn pubspec_dependencies(package: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(package.join("pubspec.yaml")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut inside = false;
    let mut indent: Option<usize> = None;
    for line in text.lines() {
        let content = line.split('#').next().unwrap_or("").trim_end();
        if content.trim().is_empty() {
            continue;
        }
        let depth = content.len() - content.trim_start().len();
        if depth == 0 {
            inside = content == "dependencies:";
            indent = None;
            continue;
        }
        if !inside {
            continue;
        }
        let at = *indent.get_or_insert(depth);
        if depth != at {
            continue;
        }
        if let Some((key, _)) = content.trim().split_once(':') {
            let key = key.trim().trim_matches(['\'', '"']);
            if !key.is_empty() {
                out.push(key.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("fynkel-packages-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn package(dir: &Path, name: &str, pubspec: &str) -> PathBuf {
        let root = dir.join(name);
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(
            root.join("pubspec.yaml"),
            format!("name: {name}\n{pubspec}"),
        )
        .unwrap();
        root
    }

    fn resolve(game: &Path, names: &[&str]) {
        std::fs::create_dir_all(game.join(".dart_tool")).unwrap();
        let mut packages = vec![json!({"name": "my_game", "rootUri": "../", "packageUri": "lib/"})];
        for n in names {
            packages
                .push(json!({"name": n, "rootUri": format!("../../{n}"), "packageUri": "lib/"}));
        }
        std::fs::write(
            game.join(".dart_tool/package_config.json"),
            json!({"configVersion": 2, "packages": packages}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn the_graph_orders_dependencies_first_and_the_game_last() {
        let dir = Scratch::new();
        let game = package(
            &dir.0,
            "my_game",
            "dependencies:\n  zeta: any # a tool\n  alpha:\n    path: ../alpha\ndev_dependencies:\n  dev_only: any\n",
        );
        package(&dir.0, "zeta", "");
        package(
            &dir.0,
            "alpha",
            "dependencies:\n  zeta: any\n  plain: any\n",
        );
        package(&dir.0, "plain", "dependencies:\n  beta: any\n");
        package(&dir.0, "beta", "");
        package(&dir.0, "dev_only", "");
        let names = |g: &PackageGraph| {
            g.packages
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
        };

        // Before `pub get`: only the game.
        assert_eq!(names(&package_graph(&game, "my_game")), ["my_game"]);

        resolve(&game, &["zeta", "alpha", "plain", "beta", "dev_only"]);
        let graph = package_graph(&game, "my_game");
        assert_eq!(names(&graph), ["beta", "plain", "zeta", "alpha", "my_game"]);
        assert!(graph.diagnostics.is_empty());
        assert_eq!(
            graph.fynkel_packages().count(),
            0,
            "no fynkel.package.json yet"
        );
    }

    #[test]
    fn a_package_file_makes_a_fynkel_package_and_the_lock_says_who_may_edit_it() {
        let dir = Scratch::new();
        let game = package(
            &dir.0,
            "my_game",
            "dependencies:\n  pickups:\n    path: ../pickups\n  props: ^1.0.0\n  plain: any\n",
        );
        let pickups = package(&dir.0, "pickups", "");
        std::fs::write(pickups.join(PACKAGE_FILE), r#"{"formatVersion": 1}"#).unwrap();
        let props = package(&dir.0, "props", "");
        std::fs::write(
            props.join(PACKAGE_FILE),
            r#"{"formatVersion": 1, "assets": "content/", "namespace": "props_kit"}"#,
        )
        .unwrap();
        package(&dir.0, "plain", "");
        resolve(&game, &["pickups", "props", "plain"]);
        std::fs::write(
            game.join("pubspec.lock"),
            "# Generated by pub\npackages:\n  pickups:\n    dependency: \"direct main\"\n    description:\n      path: \"../pickups\"\n      relative: true\n    source: path\n    version: \"0.1.0\"\n  props:\n    dependency: \"direct main\"\n    description:\n      name: props\n      url: \"https://pub.dev\"\n    source: hosted\n    version: \"1.0.2\"\nsdks:\n  dart: \">=3.9.0 <4.0.0\"\n",
        )
        .unwrap();

        let graph = package_graph(&game, "my_game");
        assert!(graph.diagnostics.is_empty(), "{:?}", graph.diagnostics);
        let by = |n: &str| graph.packages.iter().find(|p| p.name == n).unwrap();
        assert_eq!(
            by("pickups").manifest,
            Some(PackageManifest {
                assets: "assets".into(),
                namespace: "pickups".into()
            })
        );
        assert_eq!(by("pickups").source, Source::Path);
        assert!(by("pickups").editable());
        assert_eq!(by("props").asset_root(), Some(props.join("content")));
        assert_eq!(
            by("props").manifest.as_ref().unwrap().namespace,
            "props_kit"
        );
        assert_eq!(by("props").source, Source::Hosted);
        assert!(!by("props").editable(), "pub's cache is not the user's");
        assert_eq!(by("plain").manifest, None);
        assert!(by("my_game").editable());
        assert_eq!(
            graph
                .fynkel_packages()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["pickups", "props"]
        );
    }

    #[test]
    fn a_package_file_that_cannot_be_read_is_fyn_p0019_and_the_package_is_not_fynkel() {
        let dir = Scratch::new();
        let deps = ["a", "b", "c", "d", "e", "f", "g"];
        let game = package(
            &dir.0,
            "my_game",
            &format!(
                "dependencies:\n{}",
                deps.iter()
                    .map(|d| format!("  {d}: any\n"))
                    .collect::<String>()
            ),
        );
        let files = [
            ("a", "{not json"),
            ("b", "[]"),
            ("c", r#"{"formatVersion": 2}"#),
            ("d", r#"{"formatVersion": 1, "assets": "../elsewhere"}"#),
            ("e", r#"{"formatVersion": 1, "namespace": "Bad-Name"}"#),
            ("f", r#"{"formatVersion": 1, "sceneRoot": "x"}"#),
            ("g", r#"{"formatVersion": 1, "namespace": "game"}"#),
        ];
        for (name, text) in files {
            let root = package(&dir.0, name, "");
            std::fs::write(root.join(PACKAGE_FILE), text).unwrap();
        }
        resolve(&game, &deps);
        let graph = package_graph(&game, "my_game");
        assert_eq!(graph.fynkel_packages().count(), 0);
        let found: Vec<(String, String)> = graph
            .diagnostics
            .iter()
            .map(|d| {
                assert_eq!(d.code, Code::PACKAGE_MANIFEST);
                (
                    d.document.clone().unwrap(),
                    d.pointer.clone().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("a/fynkel.package.json".into(), "".into()),
                ("b/fynkel.package.json".into(), "".into()),
                ("c/fynkel.package.json".into(), "/formatVersion".into()),
                ("d/fynkel.package.json".into(), "/assets".into()),
                ("e/fynkel.package.json".into(), "/namespace".into()),
                ("f/fynkel.package.json".into(), "/sceneRoot".into()),
                ("g/fynkel.package.json".into(), "/namespace".into()),
            ]
        );
    }
}
