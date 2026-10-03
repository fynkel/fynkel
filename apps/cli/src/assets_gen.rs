//! `lib/generated/assets.g.dart` (ADR-0026 §3): every asset of a project as
//! a Dart constant, so game code names `Assets.prefabs.coin` instead of a
//! UUID. Written by `fynkel codegen --project <dir>` beside the behaviour
//! bindings, from the same asset database `fynkel validate` scans.
//!
//! The shape follows the folders under `assets/`: a folder is a getter
//! returning a const object, a file a getter of its typed asset (ADR-0036
//! §3: `MaterialAsset`, `PrefabAsset`, `AudioClipAsset`…, or a plain
//! `AssetRef` for a kind without a class), and a file with
//! sub-assets (a model, a rig document) a group whose `asset` is the file,
//! `prefab` its hierarchy, and `meshes`, `materials`, `images`, `skins`,
//! `clips` its parts by name. Names are lowerCamel identifiers from the
//! file stem (`step_left.ogg` → `stepLeft`, `Fast Run.fbx` → `fastRun`);
//! a Dart keyword or a leading digit gets a `$` (`switch$`, `$01Intro`);
//! two members of a folder that would share a name keep it only for a
//! folder, the files get their extension (`hero/` and `hero.glb` →
//! `hero` and `heroGlb`), and a name still taken gets a number.
//!
//! The constants hold ids, never paths (ADR-0006): renaming a file keeps
//! its constant working until codegen renames the constant, which is where
//! the compiler then points.
//!
//! A project that lists `persistentObjects` (ADR-0039) also gets
//! `Persistent`: one accessor per entry, `Persistent.gameManager(world)`
//! for `world.persistentObject('GameManager')`, so a misspelled name fails
//! to compile.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use fynkel_document::asset::{AssetDatabase, AssetEntry, AssetKind};
use fynkel_document::project::Project;
use fynkel_document::schema::SchemaSet;

pub const DART_OUT: &str = "lib/generated/assets.g.dart";

/// The Dart source for the project at `project` (the directory holding
/// `fynkel.project.json`), or `None` when the Dart package is not a
/// project's (no `fynkel.project.json` beside its pubspec). A project
/// without an assets directory gives an empty `Assets`.
pub fn generate(project: &Path) -> Result<Option<String>, String> {
    let Some(dir) = assets_dir(project)? else {
        return Ok(None);
    };
    let entries = scan(&dir);
    let persistent = persistent_objects(project, &entries);
    Ok(Some(render(&entries, &persistent)))
}

/// The project's `persistentObjects` (ADR-0039): each name with its
/// prefab's path (its id when the prefab is not among `entries`); none for
/// a Fynkel package or a project file that does not read.
fn persistent_objects(project: &Path, entries: &[AssetEntry]) -> Vec<(String, String)> {
    if !project.join("fynkel.project.json").is_file() {
        return Vec::new();
    }
    let Ok(p) = Project::read(project) else {
        return Vec::new();
    };
    p.persistent_objects
        .iter()
        .map(|o| {
            let id = o.prefab.asset.to_string();
            let path = entries
                .iter()
                .find(|e| e.id.to_string() == id)
                .map(|e| e.path.clone())
                .unwrap_or(id);
            (o.name.clone(), path)
        })
        .collect()
}

/// The directory holding the assets of the project (or Fynkel package) at
/// `project`; `None` when the Dart package is neither.
pub(crate) fn assets_dir(project: &Path) -> Result<Option<std::path::PathBuf>, String> {
    if !project.join("fynkel.project.json").is_file() {
        // A Fynkel package names its own assets the same way (ADR-0029 §2).
        let Some(manifest) = fynkel_document::packages::own_manifest(project) else {
            return Ok(None);
        };
        return Ok(Some(project.join(manifest.assets)));
    }
    let project_file = Project::read(project).map_err(|d| {
        format!(
            "{}: {}",
            project.display(),
            d.iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ")
        )
    })?;
    Ok(Some(project_file.assets_dir(project)))
}

/// Every asset under `assets` (none when it is missing).
pub(crate) fn scan(assets: &Path) -> Vec<AssetEntry> {
    if !assets.is_dir() {
        return Vec::new();
    }
    let schemas = SchemaSet::embedded();
    // Diagnostics are `fynkel validate`'s to report; a duplicate id keeps
    // its first path here as it does there.
    let (db, _) = AssetDatabase::scan(assets, &schemas);
    db.entries().to_vec()
}

/// One folder of the tree: sub-folders and files, by path segment.
#[derive(Default)]
struct Folder<'a> {
    folders: BTreeMap<String, Folder<'a>>,
    files: Vec<&'a AssetEntry>,
}

fn render(entries: &[AssetEntry], persistent: &[(String, String)]) -> String {
    let mut root = Folder::default();
    for e in entries {
        let mut at = &mut root;
        let mut parts: Vec<&str> = e.path.split('/').collect();
        parts.pop();
        for p in parts {
            at = at.folders.entry(p.to_string()).or_default();
        }
        at.files.push(e);
    }

    let mut o = String::new();
    let _ = writeln!(
        o,
        "// GENERATED by `fynkel codegen` from the project's assets (ADR-0026 §3)."
    );
    let _ = writeln!(o, "// Do not edit.");
    let _ = writeln!(o, "//");
    let _ = writeln!(o, "// ignore_for_file: library_private_types_in_public_api");
    let _ = writeln!(o);
    let mut used = BTreeSet::new();
    let mut body = String::new();
    let mut classes = Vec::new();
    members(&root, "Assets", true, &mut body, &mut classes, &mut used);
    if used.is_empty() {
        used.insert("AssetRef");
    }
    if !persistent.is_empty() {
        used.insert("GameObject");
        used.insert("World");
    }
    let names: Vec<&str> = used.into_iter().collect();
    let _ = writeln!(
        o,
        "import 'package:fynkel/gameplay.dart' show {};",
        names.join(", ")
    );
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "/// Every asset under `assets/`, by folder and name: `Assets.audio.coin`,"
    );
    let _ = writeln!(
        o,
        "/// `Assets.prefabs.burst`, `Assets.models.kit.meshes.cube`. Each is the"
    );
    let _ = writeln!(
        o,
        "/// asset's id (never its path), so a renamed file keeps working until the"
    );
    let _ = writeln!(o, "/// next `fynkel codegen` renames its constant.");
    let _ = writeln!(o, "abstract final class Assets {{");
    o.push_str(&body);
    let _ = writeln!(o, "}}");
    if !persistent.is_empty() {
        o.push('\n');
        o.push_str(&persistent_class(persistent));
    }
    for c in classes {
        o.push('\n');
        o.push_str(&c);
    }
    o
}

/// `Persistent`: one accessor per persistent object (ADR-0039), a name
/// that lowerCamels into one already taken numbered.
fn persistent_class(persistent: &[(String, String)]) -> String {
    let mut o = String::new();
    let _ = writeln!(
        o,
        "/// The project's persistent objects (ADR-0039; Project › Persistent Objects…),"
    );
    let _ = writeln!(
        o,
        "/// made once when the project loads and kept across scene loads. Each is"
    );
    let _ = writeln!(
        o,
        "/// `world.persistentObject(name)`: null once destroyed."
    );
    let _ = writeln!(o, "abstract final class Persistent {{");
    let mut taken = BTreeSet::new();
    for (name, path) in persistent {
        let base = ident(name);
        let mut id = base.clone();
        let mut n = 2;
        while !taken.insert(id.clone()) {
            id = match base.strip_suffix('$') {
                Some(stem) => format!("{stem}{n}"),
                None => format!("{base}{n}"),
            };
            n += 1;
        }
        let _ = writeln!(o, "  /// `{name}`: `{path}`");
        let _ = writeln!(
            o,
            "  static GameObject? {id}(World world) => world.persistentObject({});",
            dart_string(name)
        );
    }
    let _ = writeln!(o, "}}");
    o
}

/// Writes the members of `folder` into `o` (static at the top, getters
/// below) and the classes they need into `classes`.
fn members(
    folder: &Folder,
    class: &str,
    top: bool,
    o: &mut String,
    classes: &mut Vec<String>,
    used: &mut BTreeSet<&'static str>,
) {
    for (name, item) in named(folder) {
        match item {
            Item::Folder(seg, f) => {
                let cls = format!("_{class}{}", upper_first(&name.replace('$', "")));
                let _ = writeln!(o, "  /// `{seg}/`");
                if top {
                    let _ = writeln!(o, "  static const {name} = {cls}();");
                } else {
                    let _ = writeln!(o, "  {cls} get {name} => const {cls}();");
                }
                let mut body = String::new();
                members(
                    f,
                    cls.trim_start_matches('_'),
                    false,
                    &mut body,
                    classes,
                    used,
                );
                classes.push(format!(
                    "final class {cls} {{\n  const {cls}();\n\n{body}}}\n"
                ));
            }
            Item::File(e) if e.sub_assets.is_empty() => {
                let _ = writeln!(o, "  /// `{}` ({})", e.path, e.kind.name());
                let ty = file_class(e);
                used.insert(ty);
                let value = typed(ty, &e.id.to_string(), None, Some(&stem(e).0));
                if top {
                    let _ = writeln!(o, "  static const {ty} {name} = {value};");
                } else {
                    let _ = writeln!(o, "  {ty} get {name} => const {value};");
                }
            }
            Item::File(e) => {
                let cls = format!("_{class}{}", upper_first(&name.replace('$', "")));
                let _ = writeln!(o, "  /// `{}` ({}) and its parts", e.path, e.kind.name());
                if top {
                    let _ = writeln!(o, "  static const {name} = {cls}();");
                } else {
                    let _ = writeln!(o, "  {cls} get {name} => const {cls}();");
                }
                group(e, &cls, classes, used);
            }
        }
    }
}

/// The class of a file with sub-assets: `asset`, `prefab`, then one
/// sub-group per kind of part.
fn group(e: &AssetEntry, cls: &str, classes: &mut Vec<String>, used: &mut BTreeSet<&'static str>) {
    let id = e.id.to_string();
    let mut body = String::new();
    let ty = file_class(e);
    used.insert(ty);
    let _ = writeln!(body, "  /// The file itself.");
    let _ = writeln!(
        body,
        "  {ty} get asset => const {};",
        typed(ty, &id, None, Some(&stem(e).0))
    );
    let mut kinds: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for sub in e.sub_assets.keys() {
        if sub == "prefab" {
            let _ = writeln!(
                body,
                "\n  /// Its hierarchy as a prefab (`world.instantiate`)."
            );
            used.insert("PrefabAsset");
            let _ = writeln!(
                body,
                "  PrefabAsset get prefab => const PrefabAsset('{id}', sub: 'prefab');"
            );
            continue;
        }
        if let Some((kind, name)) = sub.split_once('/') {
            kinds.entry(kind).or_default().push(name);
        }
    }
    for (kind, names) in kinds {
        let plural = match kind {
            "mesh" => "meshes".to_string(),
            "anim" => "clips".to_string(),
            k => format!("{}s", camel(k)),
        };
        let sub_cls = format!("{cls}{}", upper_first(&plural));
        let _ = writeln!(body, "\n  /// The `{kind}/…` parts.");
        let _ = writeln!(body, "  {sub_cls} get {plural} => const {sub_cls}();");
        let ty = sub_class(kind);
        used.insert(ty);
        let mut sub_body = String::new();
        let idents = unique(names.iter().map(|n| (ident(n), None)).collect());
        for (n, i) in names.iter().zip(idents) {
            let _ = writeln!(sub_body, "  /// `{kind}/{n}`");
            let _ = writeln!(
                sub_body,
                "  {ty} get {i} => const {};",
                typed(ty, &id, Some(&format!("{kind}/{n}")), None)
            );
        }
        classes.push(format!(
            "final class {sub_cls} {{\n  const {sub_cls}();\n\n{sub_body}}}\n"
        ));
    }
    classes.push(format!(
        "final class {cls} {{\n  const {cls}();\n\n{body}}}\n"
    ));
}

/// The Dart class of a whole file (ADR-0036 §3); a kind without one (a
/// model, a data asset, a blend) stays an `AssetRef`.
fn file_class(e: &AssetEntry) -> &'static str {
    match &e.kind {
        AssetKind::Document(k) => match k.name() {
            "material" => "MaterialAsset",
            "prefab" => "PrefabAsset",
            "scene" => "SceneAsset",
            "shader" => "ShaderAsset",
            "controller" => "AnimatorControllerAsset",
            "volume" => "VolumeProfileAsset",
            "physics_material" => "PhysicsMaterialAsset",
            "rig" => "RigAsset",
            _ => "AssetRef",
        },
        AssetKind::Source { importer, .. } => match importer.as_str() {
            "fynkel.audio" => "AudioClipAsset",
            "fynkel.image" => "TextureAsset",
            "fynkel.shader" => "ShaderAsset",
            _ => "AssetRef",
        },
    }
}

/// The Dart class of a sub-asset by its key's head (`mesh/…`).
fn sub_class(kind: &str) -> &'static str {
    match kind {
        "mesh" => "MeshAsset",
        "material" => "MaterialAsset",
        "image" => "TextureAsset",
        "skin" => "SkinAsset",
        "anim" => "AnimationClipAsset",
        _ => "AssetRef",
    }
}

/// `MaterialAsset('<id>', name: 'gold')`, `MeshAsset('<id>', sub:
/// 'mesh/cube')`; a plain `AssetRef` takes no name.
fn typed(ty: &str, id: &str, sub: Option<&str>, name: Option<&str>) -> String {
    let mut o = format!("{ty}('{id}'");
    if let Some(sub) = sub {
        let _ = write!(o, ", sub: {}", dart_string(sub));
    }
    if let (Some(name), false) = (name, ty == "AssetRef") {
        let _ = write!(o, ", name: {}", dart_string(name));
    }
    o.push(')');
    o
}

enum Item<'f, 'a> {
    Folder(&'f str, &'f Folder<'a>),
    File(&'a AssetEntry),
}

/// The folder's members with their final identifiers, folders first,
/// then files, each in path order.
fn named<'f, 'a>(folder: &'f Folder<'a>) -> Vec<(String, Item<'f, 'a>)> {
    let mut items: Vec<(String, Option<String>, Item)> = Vec::new();
    for (seg, f) in &folder.folders {
        items.push((ident(seg), None, Item::Folder(seg, f)));
    }
    for e in &folder.files {
        let (stem, ext) = stem(e);
        items.push((ident(&stem), Some(ext), Item::File(e)));
    }
    let names = unique(
        items
            .iter()
            .map(|(n, x, _)| (n.clone(), x.clone()))
            .collect(),
    );
    names
        .into_iter()
        .zip(items)
        .map(|(n, (_, _, item))| (n, item))
        .collect()
}

/// Makes the names distinct: in a clash a folder keeps its name and a file
/// takes its extension (`heroGlb`); anything still taken gets a number.
fn unique(items: Vec<(String, Option<String>)>) -> Vec<String> {
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for (n, _) in &items {
        *count.entry(n).or_default() += 1;
    }
    let mut taken = BTreeSet::new();
    let mut out = Vec::new();
    for (n, ext) in &items {
        let mut name = match ext {
            Some(ext) if count[n.as_str()] > 1 => {
                format!("{}{}", n.trim_end_matches('$'), upper_first(&camel(ext)))
            }
            _ => n.clone(),
        };
        if taken.contains(&name) {
            let mut i = 2;
            while taken.contains(&format!("{name}{i}")) {
                i += 1;
            }
            name = format!("{name}{i}");
        }
        taken.insert(name.clone());
        out.push(name);
    }
    out
}

/// A file's stem and the extension that tells it apart: `coin.prefab.json`
/// → (`coin`, `prefab`), `kit.glb` → (`kit`, `glb`).
pub(crate) fn stem(e: &AssetEntry) -> (String, String) {
    let file = e.path.rsplit('/').next().unwrap_or(&e.path);
    let base = match e.kind {
        AssetKind::Document(_) => file.strip_suffix(".json").unwrap_or(file),
        AssetKind::Source { .. } => file,
    };
    match base.rsplit_once('.') {
        Some((s, x)) if !s.is_empty() => (s.to_string(), x.to_string()),
        _ => (base.to_string(), String::new()),
    }
}

pub(crate) const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "assert",
    "async",
    "await",
    "base",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "covariant",
    "default",
    "deferred",
    "do",
    "dynamic",
    "else",
    "enum",
    "export",
    "extends",
    "extension",
    "external",
    "factory",
    "false",
    "final",
    "finally",
    "for",
    "function",
    "get",
    "hide",
    "if",
    "implements",
    "import",
    "in",
    "interface",
    "is",
    "late",
    "library",
    "mixin",
    "new",
    "null",
    "of",
    "on",
    "operator",
    "part",
    "required",
    "rethrow",
    "return",
    "sealed",
    "set",
    "show",
    "static",
    "super",
    "switch",
    "sync",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typedef",
    "var",
    "void",
    "when",
    "while",
    "with",
    "yield",
    // Members every object has.
    "hashCode",
    "runtimeType",
    "toString",
    "noSuchMethod",
];

/// A lowerCamel Dart identifier for a file or folder name.
pub(crate) fn ident(name: &str) -> String {
    let c = camel(name);
    let c = if c.is_empty() { "asset".to_string() } else { c };
    if c.starts_with(|ch: char| ch.is_ascii_digit()) {
        format!("${c}")
    } else if KEYWORDS.contains(&c.as_str()) {
        format!("{c}$")
    } else {
        c
    }
}

/// `step_left` → `stepLeft`, `Fast Run` → `fastRun`, `HDR sky` → `hdrSky`,
/// `SomethingFun2` → `somethingFun2`; characters that are not ASCII
/// letters or digits separate words.
pub(crate) fn camel(name: &str) -> String {
    let mut out = String::new();
    for (i, w) in name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .enumerate()
    {
        if i == 0 {
            if w.chars().all(|c| !c.is_ascii_lowercase()) {
                out.push_str(&w.to_ascii_lowercase());
            } else {
                let mut cs = w.chars();
                out.extend(cs.next().map(|c| c.to_ascii_lowercase()));
                out.push_str(cs.as_str());
            }
        } else {
            out.push_str(&upper_first(w));
        }
    }
    out
}

pub(crate) fn upper_first(s: &str) -> String {
    let mut cs = s.chars();
    match cs.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
        None => String::new(),
    }
}

pub(crate) fn dart_string(s: &str) -> String {
    let mut o = String::from("'");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\'' => o.push_str("\\'"),
            '$' => o.push_str("\\$"),
            c => o.push(c),
        }
    }
    o.push('\'');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(ident("step_left"), "stepLeft");
        assert_eq!(ident("Fast Run"), "fastRun");
        assert_eq!(ident("HDR sky"), "hdrSky");
        assert_eq!(ident("SomethingFun2"), "somethingFun2");
        assert_eq!(ident("switch"), "switch$");
        assert_eq!(ident("01 intro"), "$01Intro");
        assert_eq!(ident("..."), "asset");
    }

    /// ADR-0039: the project's persistent objects as typed accessors, only
    /// when it lists some; a name codegen would lowercase into a clash or a
    /// keyword still gets an accessor of its own.
    #[test]
    fn persistent_objects_get_accessors() {
        assert!(!render(&[], &[]).contains("Persistent"));
        let out = render(
            &[],
            &[
                ("GameManager".into(), "prefabs/manager.prefab.json".into()),
                ("gameManager".into(), "prefabs/other.prefab.json".into()),
                ("Switch".into(), "prefabs/switch.prefab.json".into()),
            ],
        );
        assert!(out.contains("show AssetRef, GameObject, World;"), "{out}");
        assert!(out.contains("abstract final class Persistent {"), "{out}");
        assert!(out.contains(
            "  static GameObject? gameManager(World world) => world.persistentObject('GameManager');"
        ), "{out}");
        assert!(out.contains(
            "  static GameObject? gameManager2(World world) => world.persistentObject('gameManager');"
        ), "{out}");
        assert!(
            out.contains(
                "  static GameObject? switch$(World world) => world.persistentObject('Switch');"
            ),
            "{out}"
        );
        assert!(
            out.contains("/// `GameManager`: `prefabs/manager.prefab.json`"),
            "{out}"
        );
    }

    #[test]
    fn a_folder_keeps_its_name_and_clashing_files_take_their_extension() {
        let got = unique(vec![
            ("hero".into(), None),
            ("hero".into(), Some("glb".into())),
            ("coin".into(), Some("prefab".into())),
            ("coin".into(), Some("material".into())),
            ("kit".into(), Some("glb".into())),
        ]);
        assert_eq!(
            got,
            ["hero", "heroGlb", "coinPrefab", "coinMaterial", "kit"]
        );
    }
}
