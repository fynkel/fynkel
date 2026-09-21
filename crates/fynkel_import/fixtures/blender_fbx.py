#!/usr/bin/env python3
"""Writes the FBX importer's fixtures by round-tripping the glTF ones
through Blender, so the two importers are compared on one file's content:

  rig.glb   → rig.fbx    the skinned column, its three joints, the Prop,
                         the wave and bob clips (see rig.py)
  robot.glb → robot.fbx  the cube, the two-material pyramid-and-quad mesh
                         and the textured "Painted" material, its image
                         embedded in the file
            → robot_external.fbx + paint.png
                         the same with the image referenced by name
                         instead (the sidecar's allowExternalFiles case)

Blender's glTF importer builds the objects, the armature and the actions;
its FBX exporter writes them the way FBX files in the wild look (Blender
and Mixamo alike): Y up, an anim stack per clip, every transform key
baked at the scene's frame rate, textures embedded. The binaries are
regenerable by anyone with Blender (5.x) and are not diffed in CI — this
script is the record of where they came from.

Run from this directory:

  /Applications/Blender.app/Contents/MacOS/Blender -b --python blender_fbx.py

`rig.glb` gives Blender "wave" on the armature and "bob" on the Prop as
NLA strips; the exporter writes one stack per strip, named after it.
"""

import sys
from pathlib import Path

import bpy

HERE = Path(__file__).resolve().parent


def convert(stem, out=None, embed=True):
    src = HERE / f"{stem}.glb"
    dst = HERE / f"{out or stem}.fbx"
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(src))

    # Blender's glTF importer leaves every clip as an NLA strip on the
    # object it animates ("wave" on the armature, "bob" on the Prop); the
    # exporter writes one stack per strip, named after the strip.
    for o in bpy.data.objects:
        if o.animation_data is not None:
            o.animation_data.action = None
            for track in o.animation_data.nla_tracks:
                track.mute = False
                print(f"{o.name}: " + ", ".join(f"{s.name} mute={s.mute}" for s in track.strips))

    if not embed:
        # The images as files next to the FBX, referenced by name.
        for image in bpy.data.images:
            if image.users:
                image.filepath_raw = str(HERE / Path(image.name).with_suffix(".png").name)
                image.file_format = "PNG"
                image.save()
                print(f"wrote {image.filepath_raw}")

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.export_scene.fbx(
        filepath=str(dst),
        use_selection=True,
        # Centimetre units like most exporters, with the scale applied to
        # every value ("FBX All") rather than left as ×100 on the root
        # objects (Blender's default, which also imports, just less tidily).
        apply_unit_scale=True,
        apply_scale_options="FBX_SCALE_ALL",
        axis_forward="-Z",
        axis_up="Y",
        # Y up in the vertices themselves (as Maya and Mixamo write them),
        # not as a -90° X rotation on every root object.
        bake_space_transform=True,
        object_types={"ARMATURE", "MESH", "EMPTY"},
        add_leaf_bones=False,
        bake_anim=True,
        bake_anim_use_all_bones=True,
        bake_anim_use_nla_strips=True,
        bake_anim_use_all_actions=False,
        bake_anim_force_startend_keying=True,
        bake_anim_step=1.0,
        bake_anim_simplify_factor=0.0,
        # Vertex colours as authored (the default would convert them to sRGB).
        colors_type="LINEAR",
        path_mode="COPY" if embed else "STRIP",
        embed_textures=embed,
    )
    print(f"wrote {dst} ({dst.stat().st_size} bytes)")


def main():
    convert("rig")
    convert("robot")
    convert("robot", out="robot_external", embed=False)


if __name__ == "__main__":
    try:
        main()
    except Exception as e:  # noqa: BLE001 — Blender swallows exit codes otherwise
        print(f"error: {e}", file=sys.stderr)
        sys.exit(1)
