//! The open project: everything the session knows about a project
//! directory besides its open documents — the project file, the lock
//! (and whether it makes the project read-only), the schema set with the
//! project's behaviour schemas, the asset database with the references
//! every document makes, the shader manifests a material may name, the
//! committed behaviour metadata and the raw schema sources the Inspector
//! reads.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use fynkel_document::asset::{asset_refs, entry_json, sidecar_path, source_path};
use fynkel_document::migrate::migrated_json;
use fynkel_document::{
    diagnostics_json, AssetDatabase, AssetKind, Document, DocumentKind, Lock, Migrations, Node,
    Project, SchemaSet, ShaderLibrary,
};
use serde_json::{json, Value};
use uuid::Uuid;

/// `<project>/lib/generated/behaviours.g.json`: what `fynkel codegen`
/// writes about the project's behaviours (ADR-0012), read here as data.
pub const METADATA_FILE: &str = "lib/generated/behaviours.g.json";

/// One `assetRef` inside a document on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    /// The referencing document's id.
    pub document: Uuid,
    /// The referencing document's path, relative to the asset root.
    pub path: String,
    /// RFC 6901 pointer of the reference inside the document.
    pub pointer: String,
    pub asset: AssetRef,
}

impl Reference {
    pub fn json(&self) -> Value {
        let mut v = json!({
            "document": self.document.to_string(),
            "path": self.path,
            "pointer": self.pointer,
            "asset": self.asset.asset.to_string(),
        });
        if !self.asset.sub.is_empty() {
            v["sub"] = self.asset.sub.clone().into();
        }
        v
    }
}

pub struct OpenProject {
    pub dir: PathBuf,
    pub project: Project,
    /// The lock when it parsed.
    pub lock: Option<Lock>,
    /// Why the project is read-only (the lock was written by a newer
    /// Fynkel), or `None` when it is editable.
    pub read_only: Option<String>,
    /// The embedded schemas plus the project's own.
    pub schemas: SchemaSet,
    pub assets: AssetDatabase,
    /// The game's package assets, mounted at `Packages/<name>/`
    /// (ADR-0029 §2): scanned when the project opens (and again on every
    /// reopen), not on every rescan of the project's own.
    pub package_assets: AssetDatabase,
    /// [`OpenProject::assets`] with the package assets and the built-in
    /// assets the cook ships to a project that has not installed its own
    /// copy (the primitives, ADR-0026 §4); `None` when there are none to
    /// add. What a document's references resolve against
    /// ([`OpenProject::resolving_assets`]).
    with_builtins: Option<AssetDatabase>,
    /// Every reference every document under the asset root makes.
    pub references: Vec<Reference>,
    /// `behaviours.g.json` when the project has one.
    pub metadata: Option<Value>,
    /// Schema JSON by type id: the embedded component schemas and the
    /// project's `schemas/` files whose `x-fynkel.kind` is `component` or
    /// `behaviour`.
    pub schema_sources: HashMap<String, Value>,
    /// What opening the project reported: the project file's own
    /// diagnostics, the lock check, the metadata and schema reads, then
    /// the asset scan and the project's checks against the database (the
    /// last two replaced on every rescan).
    pub diagnostics: Vec<Diagnostic>,
    /// The sidecars the last [`OpenProject::rescan`] wrote for source
    /// files that had none (absolute paths).
    pub sidecars_written: Vec<PathBuf>,
    /// The files the last [`OpenProject::rescan`] gave a fresh id because
    /// they were copies of another asset (absolute paths; a source's
    /// sidecar).
    pub ids_renewed: Vec<PathBuf>,
    /// The part of `diagnostics` a rescan keeps.
    base_diagnostics: Vec<Diagnostic>,
    /// The asset scan's diagnostics, one set per file: an incremental
    /// rescan replaces the ones anchored at the files it re-read.
    scan_diagnostics: Vec<Diagnostic>,
}

impl OpenProject {
    /// Opens the project at `dir`. `Err` only when there is no usable
    /// project file or asset root (`FYN-P` diagnostics); everything else
    /// — a missing or mismatched lock, sidecar problems, unreadable
    /// metadata — is reported in `diagnostics` and the project opens.
    pub fn open(dir: &Path) -> Result<OpenProject, Vec<Diagnostic>> {
        let dir = dir
            .canonicalize()
            .map_err(|e| vec![io_error(dir, format!("cannot open project directory: {e}"))])?;
        let project = Project::read(&dir)?;
        let base = SchemaSet::embedded();
        // `validate` includes the read problems (the packages' too).
        let mut diagnostics: Vec<Diagnostic> = project.validate(&base);
        let schemas = project.schemas(&base, &dir).map_err(|d| vec![d])?;

        let expected = fynkel_import::expected_lock();
        let lock_path = dir.join(fynkel_document::project::LOCK_FILE);
        let mut lock = None;
        let mut read_only = None;
        match Lock::read(&dir, &schemas) {
            Ok(l) => {
                diagnostics.extend(l.check(&expected, Some(&lock_path.display().to_string())));
                read_only = newer_than_this_build(&l, &expected);
                lock = Some(l);
            }
            Err(diags) => diagnostics.extend(diags),
        }

        let assets_dir = project.assets_dir(&dir);
        if !assets_dir.is_dir() {
            return Err(vec![Diagnostic::error(
                Code::PROJECT_ASSET_ROOT,
                format!("asset root {} is not a directory", assets_dir.display()),
            )
            .with_document(
                dir.join(fynkel_document::project::PROJECT_FILE)
                    .display()
                    .to_string(),
            )
            .with_pointer("/assets")]);
        }
        let mut this = OpenProject {
            dir: dir.clone(),
            project,
            lock,
            read_only,
            schemas,
            assets: AssetDatabase::default(),
            package_assets: AssetDatabase::default(),
            with_builtins: None,
            references: Vec::new(),
            metadata: None,
            schema_sources: HashMap::new(),
            diagnostics: Vec::new(),
            sidecars_written: Vec::new(),
            ids_renewed: Vec::new(),
            base_diagnostics: diagnostics,
            scan_diagnostics: Vec::new(),
        };
        let (packaged, scan) =
            fynkel_document::packages::package_assets(&this.project.packages, &this.schemas);
        this.package_assets = packaged;
        this.base_diagnostics.extend(scan);
        this.read_metadata();
        this.read_schema_sources();
        this.rescan(&[]);
        Ok(this)
    }

    /// The database a document's references resolve against: the
    /// project's assets, its packages' and the built-in ones the cook ships (a code-built
    /// scene's primitives resolve as they do in the cook and `fynkel
    /// validate`).
    pub fn resolving_assets(&self) -> &AssetDatabase {
        self.with_builtins.as_ref().unwrap_or(&self.assets)
    }

    /// Why the file at `path` cannot be changed when it belongs to a
    /// package pub keeps — hosted or git, in pub's shared cache (ADR-0029
    /// §2); `None` for the project's own files and a `path` package's.
    pub fn package_read_only(&self, path: &Path) -> Option<String> {
        let package = self.project.packages.packages.iter().find(|p| {
            let root = p.root.canonicalize().unwrap_or_else(|_| p.root.clone());
            path.starts_with(&root)
        })?;
        (!package.editable()).then(|| {
            format!(
                "{} is part of the package '{}', which pub keeps and updates: Copy into Project to change a copy",
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                package.name
            )
        })
    }

    /// The asset root.
    pub fn assets_dir(&self) -> PathBuf {
        self.project.assets_dir(&self.dir)
    }

    /// The project's cooked cache directory (`.fynkel/cache`), where
    /// [`OpenProject::cook`] writes and the edit-preview world reads.
    pub fn cache_dir(&self) -> PathBuf {
        fynkel_import::project_cache_dir(&self.dir)
    }

    /// Cooks the project into its cache — `fynkel cook` in process, over
    /// the session's database and schemas (M4-06): what the edit-preview
    /// world needs for meshes, materials, textures and skins. The project
    /// (name, startup scene, quality, input actions, physics) is recorded
    /// in the index as the CLI records it, so a viewport opening the same
    /// cache sees the same project. Only stale objects cook; a cook with
    /// failed assets is a report whose `has_errors()` says so — the
    /// preview draws the others.
    pub fn cook(&self) -> fynkel_import::CookReport {
        self.cook_live(std::collections::HashMap::new())
    }

    /// [`cook`](Self::cook) with some documents cooked as the given text
    /// instead of their file (canonical path → text): the editor's unsaved
    /// documents, when Play cooks what the editor holds (ADR-0021 P5).
    pub fn cook_live(
        &self,
        live: std::collections::HashMap<PathBuf, String>,
    ) -> fynkel_import::CookReport {
        let startup_scene = self
            .project
            .startup_scene
            .as_ref()
            .and_then(|r| self.assets.resolve(r).ok())
            .map(|r| r.entry.id);
        let opts = fynkel_import::CookOptions {
            project: Some(fynkel_import::CookProject {
                name: self.project.name().to_string(),
                startup_scene,
                quality: self.project.quality.clone(),
                input_actions: self.project.input_actions.clone(),
                physics: self.project.physics.clone(),
                layers_and_tags: self.project.layers_and_tags.clone(),
                modules: self.project.modules,
                persistent_objects: fynkel_import::CookProject::persistent_objects(
                    &self.project,
                    &self.assets,
                ),
            }),
            live,
            ..fynkel_import::CookOptions::default()
        };
        fynkel_import::cook(
            &self.assets,
            &self.schemas,
            &fynkel_import::Cache::at(&self.cache_dir()),
            &opts,
        )
    }

    /// Writes [texts] (canonical path → the editor's unsaved text) under
    /// `.fynkel/recovery/`, at their paths relative to the project, after
    /// emptying it: the copy a Play keeps of the work it runs unsaved, so
    /// a game that hangs the editor cannot lose it (ADR-0021 P5). A file
    /// outside the project keeps its name only. Returns how many were
    /// written; a failure to write is not Play's failure.
    pub fn write_recovery(&self, texts: &std::collections::HashMap<PathBuf, String>) -> usize {
        let root = self.dir.join(".fynkel").join("recovery");
        let _ = std::fs::remove_dir_all(&root);
        let mut written = 0;
        for (path, text) in texts {
            let rel = path
                .strip_prefix(&self.dir)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| PathBuf::from(path.file_name().unwrap_or_default()));
            let target = root.join(rel);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&target, text).is_ok() {
                written += 1;
            }
        }
        written
    }

    /// The shader library a material document is validated against: the
    /// built-ins plus every `.shader` file under the asset root that
    /// compiles (ADR-0031).
    pub fn shaders(&self) -> ShaderLibrary {
        let mut shaders = ShaderLibrary::builtin();
        for (e, file) in self.shader_files() {
            if let Ok(m) = fynkel_document::material::read_shader_file(e.id, &e.path, &file) {
                shaders.add(m);
            }
        }
        shaders
    }

    /// Every `.shader` asset (ADR-0031) with its file: the project's, then
    /// its packages' that no project asset shadows.
    fn shader_files(&self) -> Vec<(&fynkel_document::AssetEntry, PathBuf)> {
        let packaged = self
            .package_assets
            .entries()
            .iter()
            .filter(|e| self.assets.get(e.id).is_none())
            .map(|e| (e, e.file(self.package_assets.root())));
        self.assets
            .entries()
            .iter()
            .map(|e| (e, e.file(self.assets.root())))
            .chain(packaged)
            .filter(|(e, _)| {
                matches!(&e.kind, AssetKind::Source { importer, .. } if importer == fynkel_document::asset::SHADER_IMPORTER)
            })
            .collect()
    }

    /// The `shaders` query (ADR-0031 § 7): what the material Inspector's
    /// Shader dropdown lists and a shader's Inspector page shows. The
    /// built-in Lit first, then every `.shader` asset, the project's before
    /// its packages'. Each is `{ shader, name, builtin, path?, file?,
    /// package?, ok, lighting, alpha, shadows, vertex, properties,
    /// diagnostics }`; `shader` is what a material's `shader` names (the
    /// built-in's `fynkel.pbr@1`, else the asset id). A shader that does
    /// not compile keeps its path and diagnostics and has no properties.
    pub fn shaders_json(&self) -> Value {
        let mut out = vec![builtin_shader_json()];
        for (e, file) in self.shader_files() {
            let mut v = json!({
                "shader": e.id.to_string(),
                "name": e.name,
                "builtin": false,
                "path": e.path,
                "file": file.display().to_string(),
            });
            if let Some(package) = fynkel_document::packages::package_of(&e.path) {
                v["package"] = package.into();
            }
            let checked = std::fs::read_to_string(&file)
                .map_err(|err| {
                    vec![
                        Diagnostic::error(Code::IO, format!("cannot read {}: {err}", e.path))
                            .with_document(e.path.clone()),
                    ]
                })
                .and_then(|text| fynkel_shader::check(Some(&e.path), &text));
            match checked {
                Ok(shader) => {
                    let i = &shader.interface;
                    v["name"] = i.name.clone().into();
                    v["ok"] = true.into();
                    v["lighting"] = i.lighting.name().into();
                    v["alpha"] = json!(i.alpha.iter().map(|m| m.name()).collect::<Vec<_>>());
                    v["shadows"] = i.shadows.into();
                    v["vertex"] = shader.has_vertex().into();
                    v["properties"] =
                        Value::Array(i.properties.iter().map(property_json).collect());
                    v["diagnostics"] = json!([]);
                }
                Err(diags) => {
                    v["ok"] = false.into();
                    v["properties"] = json!([]);
                    v["diagnostics"] =
                        fynkel_document::diagnostics_json(&diags)["diagnostics"].clone();
                }
            }
            out.push(v);
        }
        Value::Array(out)
    }

    /// Refreshes the asset database and the reference index. With `paths`
    /// empty the whole asset root is scanned again; otherwise only the
    /// named files are re-read ([`AssetDatabase::update`] — a source or its
    /// sidecar means the pair, a folder everything under it, a path that is
    /// gone drops what it held) and only the documents at those paths are
    /// re-indexed, which is what keeps a change in a project of thousands
    /// of assets under a few milliseconds. Paths are absolute, or relative
    /// to the project directory. Returns the diagnostics of what was
    /// scanned; [`OpenProject::diagnostics`] is rebuilt from the kept
    /// base, the per-file scan diagnostics and the project's checks
    /// against the database.
    ///
    /// A source file found without a sidecar (dropped into `assets/` from
    /// Finder, say) gets a fresh one — a new id, the importer from its
    /// extension — and is read again, as Unity writes a `.meta`: no
    /// `FYN-A0001` for the editor's user to act on. Not in a read-only
    /// project. The sidecars written are in
    /// [`OpenProject::sidecars_written`].
    pub fn rescan(&mut self, paths: &[PathBuf]) -> Vec<Diagnostic> {
        self.rescan_keeping(paths, &[])
    }

    /// [`OpenProject::rescan`] that never gives the files at `open` a
    /// fresh id: documents the session has open own their bytes (a Save
    /// As keeps the document's id in memory).
    pub fn rescan_keeping(&mut self, paths: &[PathBuf], open: &[PathBuf]) -> Vec<Diagnostic> {
        let mut scan = self.rescan_files(paths);
        self.sidecars_written.clear();
        self.ids_renewed.clear();
        if self.read_only.is_some() {
            return scan;
        }
        self.renew_copied_ids(&mut scan, open);
        let mut written = Vec::new();
        for d in &scan {
            if d.code != Code::ASSET_MISSING_SIDECAR {
                continue;
            }
            let Some(source) = d.document.as_deref().map(PathBuf::from) else {
                continue;
            };
            let sidecar = sidecar_path(&source);
            // Never over a sidecar that appeared meanwhile.
            if !source.is_file() || sidecar.exists() {
                continue;
            }
            let meta = fynkel_document::asset::Meta::new(&source);
            if let Ok(text) = meta.to_document().write() {
                if std::fs::write(&sidecar, text).is_ok() {
                    written.push(sidecar);
                }
            }
        }
        if !written.is_empty() {
            scan.retain(|d| {
                d.code != Code::ASSET_MISSING_SIDECAR
                    || !d
                        .document
                        .as_deref()
                        .is_some_and(|doc| written.contains(&sidecar_path(Path::new(doc))))
            });
            scan.extend(self.rescan_files(&written));
            self.sidecars_written = written;
        }
        scan
    }

    /// Foundation 5.2: a file whose id is already another file's
    /// (`FYN-A0003`) while that file is still there is a copy — made in
    /// Finder, say — and gets a fresh id, as Unity gives a copied asset a
    /// new GUID: a document its `id`, a sidecar its `id` and fresh
    /// sub-asset ids under the same keys. Nothing refers to the copy yet;
    /// the original keeps every reference. A file whose id's owner is
    /// gone from disk was moved, not copied, and keeps it (the old path's
    /// rescan admits it). The files renewed are in
    /// [`OpenProject::ids_renewed`].
    fn renew_copied_ids(&mut self, scan: &mut Vec<Diagnostic>, open: &[PathBuf]) {
        let root = self.assets.root().to_path_buf();
        let mut renewed: Vec<PathBuf> = Vec::new();
        for d in scan.iter() {
            if d.code != Code::ASSET_DUPLICATE_ID {
                continue;
            }
            let (Some(file), Some(id)) = (
                d.document.as_deref().map(PathBuf::from),
                d.id.as_deref().and_then(|i| Uuid::parse_str(i).ok()),
            ) else {
                continue;
            };
            let owner_there = self
                .assets
                .owner(id)
                .is_some_and(|e| e.file(&root).is_file());
            if !owner_there || !file.is_file() || renewed.contains(&file) || open.contains(&file) {
                continue;
            }
            if renew_id(&file).is_ok() {
                renewed.push(file);
            }
        }
        if renewed.is_empty() {
            return;
        }
        scan.retain(|d| {
            !d.document
                .as_deref()
                .is_some_and(|doc| renewed.iter().any(|r| r == Path::new(doc)))
        });
        scan.extend(self.rescan_files(&renewed));
        self.ids_renewed = renewed;
    }

    fn rescan_files(&mut self, paths: &[PathBuf]) -> Vec<Diagnostic> {
        let assets_dir = self.assets_dir();
        let scan = if paths.is_empty() {
            // The paths known before keep their ids when a copy claims
            // one too (foundation 5.2): this session's, or at open the
            // last cook's.
            let known = if self.assets.is_empty() {
                fynkel_import::indexed_paths(&self.cache_dir())
            } else {
                self.assets.paths()
            };
            let (assets, scan) = AssetDatabase::scan_preferring(&assets_dir, &self.schemas, &known);
            self.assets = assets;
            self.scan_diagnostics = scan.clone();
            scan
        } else {
            let abs: Vec<PathBuf> = paths
                .iter()
                .map(|p| {
                    canonical_or_parent(&if p.is_absolute() {
                        p.clone()
                    } else {
                        self.dir.join(p)
                    })
                })
                .collect();
            // The files whose earlier diagnostics no longer stand: each
            // path, its sidecar or its source, and everything under a
            // folder — and the files a duplicate id kept out, which the
            // update reads again.
            let touched: Vec<PathBuf> = abs
                .iter()
                .flat_map(|p| match source_path(p) {
                    Some(source) => vec![p.clone(), source],
                    None => vec![p.clone(), sidecar_path(p)],
                })
                .chain(self.assets.rejected().iter().cloned())
                .collect();
            self.scan_diagnostics.retain(|d| {
                let Some(doc) = &d.document else { return true };
                let doc = canonical_or_parent(Path::new(doc));
                !touched.iter().any(|t| doc.starts_with(t))
            });
            let fresh = self.assets.update(&abs, &self.schemas);
            self.scan_diagnostics.extend(fresh.iter().cloned());
            fresh
        };
        // Diagnostics of writing the built-ins show when the cook runs.
        let packaged = (!self.package_assets.is_empty())
            .then(|| self.assets.merged_with(&self.package_assets));
        let base = packaged.as_ref().unwrap_or(&self.assets);
        self.with_builtins = fynkel_import::builtin::with_builtins(
            base,
            &self.cache_dir(),
            &self.schemas,
            &mut Vec::new(),
        )
        .or(packaged);
        let mut all = self.base_diagnostics.clone();
        all.extend(self.scan_diagnostics.iter().cloned());
        all.extend(self.project.validate_against(&self.assets, &self.schemas));
        self.diagnostics = all;
        if paths.is_empty() {
            self.index_documents(None);
        } else {
            // Documents that are gone (deleted, moved away) lose their
            // references; the named paths — and, for a folder, every entry
            // under it — are indexed afresh.
            self.references
                .retain(|r| self.assets.by_path(&r.path).is_some());
            let mut rel: Vec<String> = Vec::new();
            for p in paths {
                let Some(r) = self.relative_to_assets(p) else {
                    continue;
                };
                if r.is_empty() {
                    continue;
                }
                let prefix = format!("{r}/");
                for e in self.assets.entries() {
                    if e.path.starts_with(&prefix) {
                        rel.push(e.path.clone());
                    }
                }
                rel.push(r);
            }
            rel.sort();
            rel.dedup();
            self.index_documents(Some(&rel));
        }
        scan
    }

    /// `path` (absolute, or relative to the project directory) relative to
    /// the asset root, `/`-separated; `None` outside it.
    pub fn relative_to_assets(&self, path: &Path) -> Option<String> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.dir.join(path)
        };
        let abs = canonical_or_parent(&abs);
        let root = self.assets_dir();
        let root = root.canonicalize().unwrap_or(root);
        let rel = abs.strip_prefix(&root).ok()?;
        Some(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// The documents that reference `asset` (the id of an asset or of a
    /// sub-asset), from the files on disk.
    pub fn dependents(&self, asset: Uuid) -> Vec<&Reference> {
        let sub_of = self
            .assets
            .resolve(&AssetRef::new(to_contract(asset)))
            .ok()
            .and_then(|r| r.sub.map(|(sub, _)| (r.entry.id, sub)));
        self.references
            .iter()
            .filter(|r| {
                let id = from_contract(r.asset.asset);
                id == asset
                    || sub_of
                        .as_ref()
                        .is_some_and(|(entry, sub)| id == *entry && r.asset.sub == *sub)
            })
            .collect()
    }

    /// The database entries as `fynkel asset list --json` prints them,
    /// optionally only one kind (`"document"` / `"source"`) or one document
    /// kind (`"scene"`, `"prefab"`, ...).
    pub fn assets_json(&self, kind: Option<&str>, document: Option<&str>) -> Vec<Value> {
        // The project's own, then its packages' (`Packages/<name>/…`,
        // ADR-0029 §2) that no project asset shadows.
        let packaged = self
            .package_assets
            .entries()
            .iter()
            .filter(|e| self.assets.get(e.id).is_none());
        self.assets
            .entries()
            .iter()
            .chain(packaged)
            .filter(|e| {
                matches!(
                    (kind, &e.kind),
                    (None, _)
                        | (Some("document"), AssetKind::Document(_))
                        | (Some("source"), AssetKind::Source { .. })
                )
            })
            .filter(|e| match (document, &e.kind) {
                (None, _) => true,
                (Some(name), AssetKind::Document(k)) => k.name() == name,
                _ => false,
            })
            .map(|e| {
                let mut v = entry_json(e);
                if let Some(why) = e
                    .origin
                    .as_deref()
                    .filter(|_| v.get("package").is_some())
                    .and_then(|f| {
                        self.package_read_only(
                            &f.canonicalize().unwrap_or_else(|_| f.to_path_buf()),
                        )
                    })
                {
                    v["readOnly"] = why.into();
                }
                v
            })
            .collect()
    }

    /// `fynkel migrate --dry-run --json` over every document under the
    /// asset root, without writing anything: the documents behind their
    /// kind's `formatVersion` (or pointing into a model whose sidecar moves
    /// to name keys), the steps each would take, the unified diff and the
    /// diagnostics of a step that cannot run. The files are read from disk,
    /// not the asset database: a sidecar behind its format is not in it.
    /// Documents that fail to read are validation's business and are
    /// skipped here.
    pub fn migrations_json(&self) -> Value {
        let migrations = Migrations::builtin();
        let root = self.assets_dir();
        let files = fynkel_import::keys_migration::document_files(&root).unwrap_or_default();
        let (planned, _) =
            fynkel_import::keys_migration::migrate_documents(&files, &migrations, &self.schemas);
        let mut documents = Vec::new();
        for p in planned {
            let m = &p.migrated;
            if !m.changed() && !m.has_errors() {
                continue;
            }
            let mut v = migrated_json(m);
            v["path"] = p.path.display().to_string().into();
            v["name"] = p
                .path
                .strip_prefix(&root)
                .unwrap_or(&p.path)
                .display()
                .to_string()
                .into();
            v["ok"] = (!m.has_errors()).into();
            v["diagnostics"] = diagnostics_json(&m.diagnostics);
            documents.push(v);
        }
        json!({
            "outdated": documents.len(),
            "steps": migrations.steps().iter().map(|step| json!({
                "id": step.id(),
                "kind": step.kind.name(),
                "from": step.from,
                "to": step.to(),
                "description": step.description,
            })).collect::<Vec<_>>(),
            "documents": documents,
        })
    }

    /// The `fynkel project describe --json` shape plus what the session
    /// adds: the directory, read-only state, counts and the type lists.
    pub fn json(&self) -> Value {
        let mut v = fynkel_document::project::describe_json(
            &self.project,
            Some(&self.assets),
            self.lock.as_ref(),
        );
        v["dir"] = self.dir.display().to_string().into();
        v["assetsDir"] = self.assets_dir().display().to_string().into();
        v["readOnly"] = self.read_only.clone().into();
        v["hasMetadata"] = self.metadata.is_some().into();
        v["referenceCount"] = self.references.len().into();
        v["componentTypes"] = self
            .schemas
            .component_types()
            .iter()
            .map(|c| c.type_id.clone())
            .collect::<Vec<_>>()
            .into();
        v["behaviourTypes"] = self
            .schemas
            .behaviour_types()
            .iter()
            .map(|b| b.type_id.clone())
            .collect::<Vec<_>>()
            .into();
        v["diagnostics"] =
            fynkel_document::diagnostics_json(&self.diagnostics)["diagnostics"].clone();
        v["errors"] = self
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .count()
            .into();
        v
    }

    /// Component, behaviour and data types with their schemas, for the
    /// Inspector (a data type's `menu` and `icon` are in its schema's
    /// `x-fynkel`).
    pub fn schemas_json(&self) -> Value {
        json!({
            "components": self.schemas.component_types().iter().map(|c| json!({
                "type": c.type_id,
                "id": c.numeric_id,
                "authored": c.authored,
                "wireSize": c.wire_size,
                "allowed": self.project.allows_component(&c.type_id),
                "schema": self.schema_sources.get(&c.type_id),
            })).collect::<Vec<_>>(),
            "behaviours": self.schemas.behaviour_types().iter().map(|b| json!({
                "type": b.type_id,
                "version": b.version,
                "dart": b.dart,
                "hasSchema": b.has_schema,
                "allowed": self.project.allows_component(&b.type_id),
                "schema": self.schema_sources.get(&b.type_id),
            })).collect::<Vec<_>>(),
            "data": self.schemas.data_types().iter().map(|d| json!({
                "type": d.type_id,
                "version": d.version,
                "dart": d.dart,
                "hasSchema": d.has_schema,
                "schema": self.schema_sources.get(&d.type_id),
            })).collect::<Vec<_>>(),
        })
    }

    /// One type's schema JSON (`title` = the type id), with its `x-fynkel`
    /// metadata and per-field hints as generated; `None` for an unknown
    /// type or a behaviour declared without a schema.
    pub fn schema(&self, type_id: &str) -> Option<&Value> {
        self.schema_sources.get(type_id)
    }

    // ------------------------------------------------------------ indexing

    /// Reads the documents under the asset root for their references — all
    /// of them, or the ones at the given relative
    /// paths (their previous entries dropped first).
    fn index_documents(&mut self, only: Option<&[String]>) {
        let only: Option<HashSet<&str>> = only.map(|p| p.iter().map(String::as_str).collect());
        let only = only.as_ref();
        match only {
            None => self.references.clear(),
            Some(paths) => self.references.retain(|r| !paths.contains(r.path.as_str())),
        }
        let root = self.assets.root().to_path_buf();
        let entries: Vec<(Uuid, String, DocumentKind)> = self
            .assets
            .entries()
            .iter()
            .filter_map(|e| match &e.kind {
                AssetKind::Document(k) => Some((e.id, e.path.clone(), *k)),
                AssetKind::Source { .. } => None,
            })
            .filter(|(_, path, _)| only.is_none_or(|paths| paths.contains(path.as_str())))
            .collect();
        for (id, path, kind) in entries {
            let Ok(doc) = Document::read(&root.join(&path), Some(kind)) else {
                // The scan already reported it.
                continue;
            };
            for (pointer, asset) in asset_refs(&doc.root) {
                if asset.is_nil() {
                    continue;
                }
                self.references.push(Reference {
                    document: id,
                    path: path.clone(),
                    pointer,
                    asset,
                });
            }
        }
        self.references
            .sort_by(|a, b| (&a.path, &a.pointer).cmp(&(&b.path, &b.pointer)));
    }

    fn read_metadata(&mut self) {
        self.read_game_metadata();
        self.merge_package_metadata();
    }

    /// Each Fynkel package's `behaviours.g.json` entries ahead of the
    /// game's, so the Inspector has every package type's metadata too
    /// (ADR-0029 §2). A package without the file adds nothing.
    fn merge_package_metadata(&mut self) {
        let mut behaviours = Vec::new();
        let mut data_types = Vec::new();
        for package in &self.project.packages.packages {
            let Some(v) = std::fs::read_to_string(package.root.join(METADATA_FILE))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            else {
                continue;
            };
            behaviours.extend(v["behaviours"].as_array().cloned().unwrap_or_default());
            data_types.extend(v["dataTypes"].as_array().cloned().unwrap_or_default());
        }
        if behaviours.is_empty() && data_types.is_empty() {
            return;
        }
        let metadata = self
            .metadata
            .get_or_insert_with(|| serde_json::json!({ "behaviours": [], "dataTypes": [] }));
        for (key, mut theirs) in [("behaviours", behaviours), ("dataTypes", data_types)] {
            theirs.extend(metadata[key].as_array().cloned().unwrap_or_default());
            metadata[key] = Value::Array(theirs);
        }
    }

    fn read_game_metadata(&mut self) {
        let diagnostics = &mut self.base_diagnostics;
        let path = self.dir.join(METADATA_FILE);
        let declares_behaviours = self
            .project
            .components
            .iter()
            .flatten()
            .any(|c| c.kind == "behaviour");
        if !path.is_file() {
            if declares_behaviours {
                diagnostics.push(
                    Diagnostic::warning(
                        Code::SESSION_IO,
                        format!(
                            "no {METADATA_FILE}: the Inspector has no metadata for the project's behaviours"
                        ),
                    )
                    .with_document(path.display().to_string())
                    .with_suggestion("run `fynkel codegen --project .` to generate it"),
                );
            }
            return;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
        {
            Ok(v) if v.is_object() => self.metadata = Some(v),
            Ok(_) => diagnostics.push(
                Diagnostic::warning(Code::SESSION_IO, "behaviour metadata is not a JSON object")
                    .with_document(path.display().to_string())
                    .with_suggestion("run `fynkel codegen --project .` to regenerate it"),
            ),
            Err(why) => diagnostics.push(
                Diagnostic::warning(
                    Code::SESSION_IO,
                    format!("cannot read behaviour metadata: {why}"),
                )
                .with_document(path.display().to_string())
                .with_suggestion("run `fynkel codegen --project .` to regenerate it"),
            ),
        }
    }

    fn read_schema_sources(&mut self) {
        for (_, text) in fynkel_document::generated::EMBEDDED_SCHEMAS {
            if let Ok(v) = serde_json::from_str::<Value>(text) {
                self.take_schema_source(v);
            }
        }
        // The packages' schemas, then the game's (ADR-0029 §2).
        let mut dirs = self.project.packages.schema_dirs();
        dirs.push(self.dir.join(fynkel_document::runtime::PROJECT_SCHEMAS_DIR));
        let mut files = Vec::new();
        for dir in &dirs {
            let mut own = Vec::new();
            collect_schema_files(dir, &mut own);
            own.sort();
            files.extend(own);
        }
        for file in files {
            match std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
            {
                Ok(v) => self.take_schema_source(v),
                Err(why) => self.base_diagnostics.push(
                    Diagnostic::warning(Code::SESSION_IO, format!("cannot read schema: {why}"))
                        .with_document(file.display().to_string()),
                ),
            }
        }
    }

    fn take_schema_source(&mut self, v: Value) {
        let kind = v
            .get("x-fynkel")
            .and_then(|m| m.get("kind"))
            .and_then(Value::as_str);
        if !matches!(kind, Some("component") | Some("behaviour") | Some("data")) {
            return;
        }
        let type_id = v
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| {
                v.get("x-fynkel")
                    .and_then(|m| m.get("id"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string);
        if let Some(type_id) = type_id {
            self.schema_sources.insert(type_id, v);
        }
    }
}

fn collect_schema_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_schema_files(&path, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".schema.json"))
        {
            out.push(path);
        }
    }
}

fn io_error(path: &Path, message: String) -> Diagnostic {
    Diagnostic::error(Code::SESSION_IO, message).with_document(path.display().to_string())
}

/// The reason the lock makes the project read-only: it names a runtime,
/// tool or lock format newer than this build (§6: a newer document is
/// read or refused, never rewritten). A lock that is merely *different*
/// (an older build, another importer set) is a `FYN-P0011` mismatch and
/// leaves the project editable.
fn newer_than_this_build(lock: &Lock, expected: &Lock) -> Option<String> {
    if lock.format_version > expected.format_version {
        return Some(format!(
            "the lock's formatVersion {} is newer than the supported {}",
            lock.format_version, expected.format_version
        ));
    }
    if lock.runtime.abi_version > expected.runtime.abi_version {
        return Some(format!(
            "the lock names ABI version {}; this build speaks {}",
            lock.runtime.abi_version, expected.runtime.abi_version
        ));
    }
    if version_newer(&lock.runtime.version, &expected.runtime.version) {
        return Some(format!(
            "the lock names runtime {}; this build is {}",
            lock.runtime.version, expected.runtime.version
        ));
    }
    if version_newer(&lock.tools.version, &expected.tools.version) {
        return Some(format!(
            "the lock names tools {}; this build is {}",
            lock.tools.version, expected.tools.version
        ));
    }
    None
}

/// `a` is a strictly newer dotted version than `b` (numeric segments,
/// missing segments 0; a segment that is not a number compares as text).
pub fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<(u64, String)> {
        s.split(['.', '-', '+'])
            .map(|seg| match seg.parse::<u64>() {
                Ok(n) => (n, String::new()),
                Err(_) => (u64::MAX, seg.to_string()),
            })
            .collect()
    };
    let (a, b) = (parse(a), parse(b));
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).cloned().unwrap_or((0, String::new()));
        let y = b.get(i).cloned().unwrap_or((0, String::new()));
        if x != y {
            return x > y;
        }
    }
    false
}

/// The canonical path of `path`, or — for a path that is gone — its
/// canonical parent joined with its name, so a deleted file under a
/// symlinked root (macOS's temp directories) still compares with the root.
pub(crate) fn canonical_or_parent(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => canonical_or_parent(parent).join(name),
        _ => path.to_path_buf(),
    }
}

pub(crate) fn to_contract(id: Uuid) -> fynkel_contracts::Uuid {
    fynkel_contracts::Uuid::from_bytes(*id.as_bytes())
}

pub(crate) fn from_contract(id: fynkel_contracts::Uuid) -> Uuid {
    Uuid::from_bytes(id.0)
}

/// One header property as the `shaders` query lists it.
fn property_json(p: &fynkel_shader::Property) -> Value {
    use fynkel_shader::{PropertyKind, ValueType};
    let mut v = json!({
        "name": p.name,
        "label": p.label(),
        "line": p.line,
    });
    if let Some(t) = &p.tooltip {
        v["tooltip"] = t.clone().into();
    }
    match &p.kind {
        PropertyKind::Value {
            ty,
            default,
            min,
            max,
        } => {
            v["type"] = ty.name().into();
            v["default"] = match ty {
                ValueType::Float => json!(default[0]),
                ValueType::Int => json!(default[0] as i64),
                ValueType::Bool => json!(default[0] != 0.0),
                _ => json!(default),
            };
            if let Some(m) = min {
                v["min"] = json!(m);
            }
            if let Some(m) = max {
                v["max"] = json!(m);
            }
        }
        PropertyKind::Texture {
            fallback,
            color_space,
            required,
        } => {
            v["type"] = "texture".into();
            v["default"] = fallback.name().into();
            v["colorSpace"] = color_space.name().into();
            v["required"] = (*required).into();
        }
    }
    v
}

/// The built-in `fynkel.pbr@1` as the `shaders` query lists it: Unity's
/// Lit, drawn by the engine's own shader. Its properties carry the
/// manifest's names (`baseColor`); a material holds them as the
/// `PbrMaterial` fields (`base_color`, `base_color_texture`).
fn builtin_shader_json() -> Value {
    let library = ShaderLibrary::builtin();
    let pbr = library
        .get(fynkel_document::material::PBR_SHADER_ID)
        .expect("the built-in PBR shader");
    let mut properties = Vec::new();
    for p in &pbr.parameters {
        let mut v = json!({
            "name": p.name,
            "label": fynkel_shader::nicify(&p.name),
            "type": match p.ty.name() { "color4" => "color", other => other },
            "default": p.default.as_ref().map(|d| d.to_json()),
        });
        if let Some(m) = p.min {
            v["min"] = json!(m);
        }
        if let Some(m) = p.max {
            v["max"] = json!(m);
        }
        if !p.description.is_empty() {
            v["tooltip"] = p.description.clone().into();
        }
        properties.push(v);
    }
    for t in &pbr.textures {
        let mut v = json!({
            "name": t.name,
            "label": fynkel_shader::nicify(&t.name),
            "type": "texture",
            "default": t.default.clone().unwrap_or_else(|| "white".into()),
            "colorSpace": t.color_space.name(),
            "required": t.required,
        });
        if !t.description.is_empty() {
            v["tooltip"] = t.description.clone().into();
        }
        properties.push(v);
    }
    json!({
        "shader": pbr.shader_id,
        "name": "Lit",
        "builtin": true,
        "ok": true,
        "lighting": "lit",
        "alpha": ["opaque", "mask", "blend"],
        "shadows": true,
        "vertex": false,
        "properties": properties,
        "diagnostics": [],
    })
}

/// Gives the asset file at `file` — a document, or a source's sidecar — a
/// fresh id in place (foundation 5.2): a document its `id` (a rig's
/// derived sub-assets follow it), a sidecar its `id` and a fresh id for
/// every sub-asset key. The rest of the file is written back as it was.
fn renew_id(file: &Path) -> Result<(), Diagnostic> {
    let text = if source_path(file).is_some() {
        let doc = Document::read(file, Some(DocumentKind::Meta)).map_err(|mut v| v.remove(0))?;
        let mut meta = fynkel_document::asset::Meta::from_document(doc);
        meta.document
            .root
            .set("id", Node::string(Uuid::new_v4().to_string()));
        for (_, sub) in &mut meta.sub_assets {
            *sub = Uuid::new_v4();
        }
        meta.to_document().write()?
    } else {
        let kind = DocumentKind::from_path(file)
            .ok_or_else(|| io_error(file, "not an asset document".to_string()))?;
        let mut doc = Document::read(file, Some(kind)).map_err(|mut v| v.remove(0))?;
        doc.root.set("id", Node::string(Uuid::new_v4().to_string()));
        doc.write()?
    };
    std::fs::write(file, text).map_err(|e| io_error(file, format!("cannot write: {e}")))
}
