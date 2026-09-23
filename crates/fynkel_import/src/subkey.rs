//! Sub-asset keys from names (`docs/design/stable-sub-asset-ids.md`).
//!
//! An importer names each part it reads by its position (`mesh/3`,
//! `anim/0`, `node/12`) and registers the part's source name here. The
//! [`Keys`] built from those names turn a position key into the name key
//! a migrated sidecar uses (`mesh/ramp`, `anim/walk`, `node/Hero/Hips`),
//! so reordering parts in the modelling tool keeps every reference:
//!
//! - a name is sanitised to the key alphabet (`[A-Za-z0-9_. -]`, anything
//!   else `_`, surrounding spaces trimmed);
//! - an unnamed part keeps its position behind `#`: `mesh/#3`;
//! - a name met again (same kind; same parent for nodes) gets `~2`, `~3`
//!   in file order: `mesh/Cube`, `mesh/Cube~2`;
//! - a name longer than [`MAX_NAME`] is cut and gets `~` plus a hash of
//!   the whole name, so the sidecar's 128-character keys always fit.
//!
//! `#` and `~` never survive sanitising, so a marker cannot collide with a
//! name. Whatever follows the part (`mesh/3/1` for a primitive,
//! `image/0/linear` for a data copy) is kept after the renamed head.

use std::collections::{BTreeMap, HashMap};

use fynkel_document::asset::Meta;
use uuid::Uuid;

/// What moving one file's sidecar from position keys to name keys
/// changes: its sub-asset keys, and the ids derived from node positions
/// that prefab overrides address.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Renames {
    /// Position key → name key, for every sub-asset whose key changes.
    pub subs: Vec<(String, String)>,
    /// Old derived id → new derived id (entities and components of the
    /// imported prefab), for every id that changes.
    pub ids: Vec<(Uuid, Uuid)>,
}

/// The longest sanitised name a key carries before it is cut.
pub const MAX_NAME: usize = 80;

/// The sidecar `formatVersion` from which sub-asset keys are names.
pub const NAME_KEYS_FORMAT: u32 = 2;

/// True when `meta` was written for name keys; older sidecars keep
/// position keys until `fynkel migrate` moves them.
pub fn uses_name_keys(meta: &Meta) -> bool {
    meta.document.envelope.format_version >= NAME_KEYS_FORMAT
}

/// A source name as a key segment; `None` when nothing is left of it.
pub fn sanitize(name: &str) -> Option<String> {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ' ' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.len() <= MAX_NAME {
        return Some(cleaned);
    }
    let hash = blake3::hash(name.as_bytes()).to_hex();
    Some(format!("{}~{}", &cleaned[..MAX_NAME - 9], &hash[..8]))
}

/// Unique segments for names met in order within one scope: the first of
/// a name keeps it, later ones get `~2`, `~3`; an unnamed entry gets
/// `#<position>`.
#[derive(Default)]
struct Scope {
    seen: HashMap<String, u32>,
    taken: std::collections::HashSet<String>,
}

impl Scope {
    fn segment(&mut self, name: Option<&str>, position: &str) -> String {
        let Some(base) = name.and_then(sanitize) else {
            return format!("#{position}");
        };
        let n = self.seen.entry(base.clone()).or_insert(0);
        *n += 1;
        let mut candidate = if *n == 1 {
            base.clone()
        } else {
            format!("{base}~{n}")
        };
        // `Cube~2` is not a sanitised name, so only our own suffixes can
        // collide; keep counting until the segment is free.
        while !self.taken.insert(candidate.clone()) {
            *n += 1;
            candidate = format!("{base}~{n}");
        }
        candidate
    }
}

/// Position key → name key for the parts of one file.
#[derive(Default, Debug, Clone)]
pub struct Keys {
    /// `kind/<position>` → `kind/<segment>`.
    heads: BTreeMap<String, String>,
}

impl Keys {
    /// Registers the parts of one kind in file order: `(position, name)`.
    pub fn kind<N: AsRef<str>>(
        &mut self,
        kind: &str,
        parts: impl IntoIterator<Item = (String, Option<N>)>,
    ) {
        let mut scope = Scope::default();
        for (position, name) in parts {
            let segment = scope.segment(name.as_ref().map(AsRef::as_ref), &position);
            self.heads
                .insert(format!("{kind}/{position}"), format!("{kind}/{segment}"));
        }
    }

    /// The name key for a position key; keys this table does not know
    /// (`prefab`, a head no importer registered) come back unchanged.
    pub fn rename(&self, key: &str) -> String {
        let mut parts = key.splitn(3, '/');
        let (Some(kind), Some(position)) = (parts.next(), parts.next()) else {
            return key.to_string();
        };
        let head = format!("{kind}/{position}");
        match (self.heads.get(&head), parts.next()) {
            (Some(new), Some(rest)) => format!("{new}/{rest}"),
            (Some(new), None) => new.clone(),
            (None, _) => key.to_string(),
        }
    }

    /// Every registered head that changes, position → name.
    pub fn changes(&self) -> impl Iterator<Item = (&str, &str)> {
        self.heads
            .iter()
            .filter(|(a, b)| a != b)
            .map(|(a, b)| (a.as_str(), b.as_str()))
    }
}

/// Name paths for a node tree: `node/<root name>/<child name>/…`, with the
/// same `#`/`~` rules among siblings (roots are siblings of each other).
/// `names[i]` is node `i`'s source name (`None` unnamed), `children[i]`
/// its children in order, `roots` the top nodes in order. Nodes not
/// reached from a root keep their position key `node/<i>`.
pub fn node_paths(names: &[Option<&str>], children: &[Vec<usize>], roots: &[usize]) -> Vec<String> {
    let mut paths: Vec<Option<String>> = vec![None; names.len()];
    let mut stack: Vec<(Vec<usize>, String)> = vec![(roots.to_vec(), "node".to_string())];
    while let Some((siblings, parent)) = stack.pop() {
        let mut scope = Scope::default();
        for &i in &siblings {
            if i >= names.len() || paths[i].is_some() {
                continue;
            }
            let path = format!("{parent}/{}", scope.segment(names[i], &i.to_string()));
            paths[i] = Some(path.clone());
            stack.push((children[i].clone(), path));
        }
    }
    paths
        .into_iter()
        .enumerate()
        .map(|(i, p)| p.unwrap_or_else(|| format!("node/{i}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(kind: &str, names: &[Option<&str>]) -> Keys {
        let mut k = Keys::default();
        k.kind(
            kind,
            names.iter().enumerate().map(|(i, n)| (i.to_string(), *n)),
        );
        k
    }

    #[test]
    fn names_become_keys_and_suffixes_are_kept() {
        let k = keys("mesh", &[Some("cube"), Some("ramp")]);
        assert_eq!(k.rename("mesh/0"), "mesh/cube");
        assert_eq!(k.rename("mesh/1"), "mesh/ramp");
        assert_eq!(k.rename("mesh/1/2"), "mesh/ramp/2");
        assert_eq!(k.rename("mesh/7"), "mesh/7");
        assert_eq!(k.rename("prefab"), "prefab");
    }

    #[test]
    fn unnamed_duplicate_and_odd_names() {
        let k = keys(
            "anim",
            &[
                None,
                Some("Run"),
                Some("Run"),
                Some("mixamo.com|Take:1"),
                Some("  "),
                Some("Run~2"),
            ],
        );
        assert_eq!(k.rename("anim/0"), "anim/#0");
        assert_eq!(k.rename("anim/1"), "anim/Run");
        assert_eq!(k.rename("anim/2"), "anim/Run~2");
        assert_eq!(k.rename("anim/3"), "anim/mixamo.com_Take_1");
        assert_eq!(k.rename("anim/4"), "anim/#4");
        // `~` is sanitised away, so a name cannot take a marker's place.
        assert_eq!(k.rename("anim/5"), "anim/Run_2");
    }

    #[test]
    fn long_names_are_cut_with_a_hash() {
        let long = "x".repeat(200);
        let other = format!("{}y", "x".repeat(199));
        let k = keys("material", &[Some(&long), Some(&other)]);
        let a = k.rename("material/0");
        let b = k.rename("material/1");
        assert_ne!(a, b);
        assert!(a.len() <= "material/".len() + MAX_NAME, "{a}");
    }

    #[test]
    fn node_paths_follow_the_tree() {
        // 0 Hero ─ 1 Hips ─ 2 Leg, 3 Leg ; 4 (unnamed root)
        let names = [
            Some("Hero"),
            Some("mixamorig:Hips"),
            Some("Leg"),
            Some("Leg"),
            None,
        ];
        let children = vec![vec![1], vec![2, 3], vec![], vec![], vec![]];
        let paths = node_paths(&names, &children, &[0, 4]);
        assert_eq!(paths[0], "node/Hero");
        assert_eq!(paths[1], "node/Hero/mixamorig_Hips");
        assert_eq!(paths[2], "node/Hero/mixamorig_Hips/Leg");
        assert_eq!(paths[3], "node/Hero/mixamorig_Hips/Leg~2");
        assert_eq!(paths[4], "node/#4");
    }
}
