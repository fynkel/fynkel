//! Rig assembly (blueprint §19.2d item 19b): matching a clip to a skeleton
//! by bone name. A rig document names one skeleton file and clips from
//! any imported model; the cook re-emits every clip against the
//! skeleton, and `fynkel rig inspect|validate` reports the same match
//! from in-memory imports. [`retarget`] is the one rule both use.
//!
//! A channel is kept when its target names a joint of the skeleton's skin
//! or an entity of the skeleton's hierarchy (a rig's prop, its mesh
//! node). A channel that targets a **joint of the clip file's own skin**
//! the skeleton lacks is the bone mismatch `FYN-A0106` (the skeletons
//! differ; the first such bone is named); a clip file without a skin has
//! no way to say which targets are bones, so every unmatched target is a
//! mismatch. Other unmatched targets (the clip file's mesh or armature
//! nodes) are dropped with `A0105`. A clip left with no channel is
//! `A0106` too.

use std::collections::BTreeSet;
use std::path::Path;

use fynkel_contracts::{Code, Diagnostic};
use fynkel_cooked::payload::{decode_animation, decode_skin, AnimationPayload};
use fynkel_cooked::{KIND_ANIMATION, KIND_JSON, KIND_SKIN};
use serde_json::Value;

use crate::intermediate::Intermediate;

/// What a skeleton file offers as animation targets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkeletonNames {
    /// Every joint of every skin.
    pub joints: BTreeSet<String>,
    /// Every entity of the hierarchy (joints included).
    pub nodes: BTreeSet<String>,
}

impl SkeletonNames {
    pub fn from_intermediate(im: &Intermediate) -> SkeletonNames {
        let mut names = SkeletonNames::default();
        for skin in &im.skins {
            names
                .joints
                .extend(skin.payload.joints.iter().map(|j| j.name.clone()));
        }
        if let Some(prefab) = &im.prefab {
            names.nodes.extend(prefab_entity_names(prefab));
        }
        names
    }

    pub fn contains(&self, target: &str) -> bool {
        self.joints.contains(target) || self.nodes.contains(target)
    }

    pub fn is_empty(&self) -> bool {
        self.joints.is_empty() && self.nodes.is_empty()
    }
}

/// The joint names of a clip file's own skins, `None` when it has none.
pub fn joint_names(im: &Intermediate) -> Option<BTreeSet<String>> {
    if im.skins.is_empty() {
        return None;
    }
    Some(
        im.skins
            .iter()
            .flat_map(|s| s.payload.joints.iter().map(|j| j.name.clone()))
            .collect(),
    )
}

/// The `name` of every entity of a prefab document body.
pub fn prefab_entity_names(prefab: &Value) -> BTreeSet<String> {
    prefab["entities"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, e)| e["name"].as_str().map(str::to_string))
        .collect()
}

/// A clip matched to a skeleton.
#[derive(Clone, Debug, PartialEq)]
pub struct Retargeted {
    /// The kept channels, source order.
    pub payload: AnimationPayload,
    /// Channels kept.
    pub matched: usize,
    /// Targets dropped (not bones of the clip file), sorted, unique.
    pub dropped: Vec<String>,
    /// Bones the skeleton lacks, sorted, unique (the mismatch).
    pub missing: Vec<String>,
}

/// Matches `clip` to `skeleton` by target name — see the module doc.
/// `clip_joints` are the clip file's own skin joints (`None`: it has no
/// skin). The diagnostics carry `clip_name` and no document; the caller
/// anchors them.
pub fn retarget(
    clip_name: &str,
    clip: &AnimationPayload,
    clip_joints: Option<&BTreeSet<String>>,
    skeleton: &SkeletonNames,
) -> (Retargeted, Vec<Diagnostic>) {
    let mut kept = Vec::new();
    let mut dropped = BTreeSet::new();
    let mut missing = BTreeSet::new();
    for c in &clip.channels {
        if skeleton.contains(&c.target) {
            kept.push(c.clone());
        } else if clip_joints.is_none_or(|j| j.contains(&c.target)) {
            missing.insert(c.target.clone());
        } else {
            dropped.insert(c.target.clone());
        }
    }
    let mut diagnostics = Vec::new();
    if let Some(first) = missing.iter().next() {
        let more = missing.len() - 1;
        diagnostics.push(
            Diagnostic::error(
                Code::RIG_BONE_MISMATCH,
                format!(
                    "clip {clip_name:?} animates bone {first:?}, which the skeleton does not have{}",
                    if more > 0 {
                        format!(" ({more} more)")
                    } else {
                        String::new()
                    }
                ),
            )
            .with_suggestion(
                "clips are matched to the skeleton by bone name: export the clip for this skeleton (Mixamo: the same character for every download), or name a skeleton file with these bones",
            ),
        );
    } else if kept.is_empty() {
        diagnostics.push(
            Diagnostic::error(
                Code::RIG_BONE_MISMATCH,
                format!("clip {clip_name:?} animates nothing the skeleton has"),
            )
            .with_suggestion("the clip's targets name none of the skeleton's joints or nodes"),
        );
    }
    if !dropped.is_empty() {
        diagnostics.push(
            Diagnostic::warning(
                Code::IMPORT_SKIPPED,
                format!(
                    "clip {clip_name:?}: {} channel target(s) the skeleton does not have dropped: {}",
                    dropped.len(),
                    dropped.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            )
            .with_suggestion("nodes of the clip file that are not bones (its mesh, its armature root); nothing of the skeleton is affected"),
        );
    }
    let matched = kept.len();
    (
        Retargeted {
            // The clip keeps its length: a dropped channel does not
            // shorten a loop.
            payload: AnimationPayload {
                duration: if kept.is_empty() { 0.0 } else { clip.duration },
                channels: kept,
                markers: clip.markers.clone(),
            },
            matched,
            dropped: dropped.into_iter().collect(),
            missing: missing.into_iter().collect(),
        },
        diagnostics,
    )
}

/// The same three readers over a cooked container (what the cook has for
/// a dependency): the skeleton's names, a clip file's joints, one clip.
pub mod cooked {
    use super::*;

    struct Opened {
        file: fynkel_cooked::ChunkFile<std::fs::File>,
        chunks: Vec<Value>,
    }

    fn open(path: &Path) -> Result<Opened, String> {
        let mut file = fynkel_cooked::ChunkFile::open_path(path).map_err(|e| e.to_string())?;
        let meta = file.meta(1 << 20).map_err(|e| e.to_string())?;
        let meta: Value = serde_json::from_slice(&meta).map_err(|e| e.to_string())?;
        let chunks = meta["chunks"].as_array().cloned().unwrap_or_default();
        Ok(Opened { file, chunks })
    }

    impl Opened {
        fn indices(&self, kind: &str) -> Vec<(usize, Value)> {
            self.chunks
                .iter()
                .filter(|c| c["kind"] == kind)
                .filter_map(|c| c["index"].as_u64().map(|i| (i as usize, c.clone())))
                .collect()
        }

        fn read(&mut self, index: usize) -> Result<Vec<u8>, String> {
            self.file.read(index, 64 << 20).map_err(|e| e.to_string())
        }
    }

    fn kind(k: [u8; 4]) -> String {
        String::from_utf8_lossy(&k).into_owned()
    }

    /// Joints of every `SKIN` chunk and the entity names of the `prefab`
    /// `JSON` chunk.
    pub fn skeleton_names(path: &Path) -> Result<SkeletonNames, String> {
        let mut opened = open(path)?;
        let mut names = SkeletonNames::default();
        for (i, _) in opened.indices(&kind(KIND_SKIN)) {
            let bytes = opened.read(i)?;
            let skin = decode_skin(&bytes).ok_or("a SKIN chunk does not decode")?;
            names.joints.extend(skin.joints.into_iter().map(|j| j.name));
        }
        for (i, c) in opened.indices(&kind(KIND_JSON)) {
            if c["sub"] != "prefab" {
                continue;
            }
            let bytes = opened.read(i)?;
            let prefab: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            names.nodes.extend(prefab_entity_names(&prefab));
        }
        Ok(names)
    }

    /// The joints of every `SKIN` chunk; `None` without one.
    pub fn joint_names(path: &Path) -> Result<Option<BTreeSet<String>>, String> {
        let mut opened = open(path)?;
        let skins = opened.indices(&kind(KIND_SKIN));
        if skins.is_empty() {
            return Ok(None);
        }
        let mut joints = BTreeSet::new();
        for (i, _) in skins {
            let bytes = opened.read(i)?;
            let skin = decode_skin(&bytes).ok_or("a SKIN chunk does not decode")?;
            joints.extend(skin.joints.into_iter().map(|j| j.name));
        }
        Ok(Some(joints))
    }

    /// The `ANIM` chunk with sub-asset id `sub`, `None` when the object
    /// has none.
    pub fn animation(path: &Path, sub: &str) -> Result<Option<AnimationPayload>, String> {
        let mut opened = open(path)?;
        let Some((i, _)) = opened
            .indices(&kind(KIND_ANIMATION))
            .into_iter()
            .find(|(_, c)| c["sub"] == sub)
        else {
            return Ok(None);
        };
        let bytes = opened.read(i)?;
        decode_animation(&bytes)
            .map(Some)
            .ok_or_else(|| format!("the ANIM chunk {sub} does not decode"))
    }

    /// The `prefab` `JSON` chunk's text, `None` without one.
    pub fn prefab_text(path: &Path) -> Result<Option<String>, String> {
        let mut opened = open(path)?;
        let Some((i, _)) = opened
            .indices(&kind(KIND_JSON))
            .into_iter()
            .find(|(_, c)| c["sub"] == "prefab")
        else {
            return Ok(None);
        };
        let bytes = opened.read(i)?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_cooked::payload::{
        AnimationChannel, ANIM_INTERPOLATION_LINEAR, ANIM_PROPERTY_ROTATION,
    };

    fn channel(target: &str) -> AnimationChannel {
        AnimationChannel {
            target: target.to_string(),
            property: ANIM_PROPERTY_ROTATION,
            interpolation: ANIM_INTERPOLATION_LINEAR,
            times: vec![0.0, 1.0],
            values: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }
    }

    fn skeleton(joints: &[&str], nodes: &[&str]) -> SkeletonNames {
        SkeletonNames {
            joints: joints.iter().map(|s| s.to_string()).collect(),
            nodes: nodes.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn matched_channels_are_kept_in_order() {
        let clip = AnimationPayload {
            duration: 1.0,
            channels: vec![channel("hips"), channel("Prop"), channel("spine")],
            markers: Vec::new(),
        };
        let sk = skeleton(&["hips", "spine"], &["Rig", "hips", "spine", "Prop"]);
        let (r, diags) = retarget("wave", &clip, Some(&sk.joints), &sk);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(r.matched, 3);
        assert_eq!(r.payload.duration, 1.0);
        let targets: Vec<&str> = r
            .payload
            .channels
            .iter()
            .map(|c| c.target.as_str())
            .collect();
        assert_eq!(targets, ["hips", "Prop", "spine"]);
    }

    #[test]
    fn a_clip_bone_the_skeleton_lacks_is_a0106() {
        let clip = AnimationPayload {
            duration: 1.0,
            channels: vec![channel("hips"), channel("tail"), channel("tail2")],
            markers: Vec::new(),
        };
        let sk = skeleton(&["hips"], &["hips"]);
        let clip_joints: BTreeSet<String> = ["hips", "tail", "tail2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (r, diags) = retarget("walk", &clip, Some(&clip_joints), &sk);
        assert_eq!(r.matched, 1);
        assert_eq!(r.missing, ["tail", "tail2"]);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Code::RIG_BONE_MISMATCH);
        assert!(
            diags[0].message.contains("\"tail\""),
            "{}",
            diags[0].message
        );
        assert!(diags[0].message.contains("1 more"), "{}", diags[0].message);
    }

    #[test]
    fn non_bone_targets_are_dropped_with_a_warning() {
        let clip = AnimationPayload {
            duration: 2.0,
            channels: vec![channel("hips"), channel("Armature"), channel("Body")],
            markers: Vec::new(),
        };
        let sk = skeleton(&["hips"], &["Rig", "hips"]);
        let clip_joints: BTreeSet<String> = ["hips".to_string()].into_iter().collect();
        let (r, diags) = retarget("run", &clip, Some(&clip_joints), &sk);
        assert_eq!(r.matched, 1);
        assert_eq!(r.dropped, ["Armature", "Body"]);
        assert!(r.missing.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Code::IMPORT_SKIPPED);
        assert_eq!(diags[0].severity, fynkel_contracts::Severity::Warning);
    }

    #[test]
    fn without_a_skin_every_unmatched_target_is_a_mismatch() {
        let clip = AnimationPayload {
            duration: 1.0,
            channels: vec![channel("Armature")],
            markers: Vec::new(),
        };
        let sk = skeleton(&["hips"], &["hips"]);
        let (r, diags) = retarget("x", &clip, None, &sk);
        assert_eq!(r.matched, 0);
        assert_eq!(r.missing, ["Armature"]);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Code::RIG_BONE_MISMATCH);
    }
}
