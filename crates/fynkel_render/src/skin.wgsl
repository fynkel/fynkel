// Skinning (stage 9 phase 2), shared by the passes that draw meshes: a
// skinned mesh's vertex goes where its joints take it, mixed by its
// weights. Each pass's skinned vertex stage bends the vertex first, then
// does what it does for any other ("skin, then the rest").

// A joint, as the renderer writes it: where it takes its meshes' vertices,
// in the world (its world matrix times its inverse bind matrix), as the
// matrix's three rows (the last is always 0, 0, 0, 1).
struct Joint {
    row_0: vec4<f32>,
    row_1: vec4<f32>,
    row_2: vec4<f32>,
};

// Every skeleton's joints this frame; a draw's start where its instance
// says.
@group(0) @binding(2) var<storage, read> joints: array<Joint>;

// A vertex's four joints, as numbers within its skeleton, and how much
// each moves it, adding up to 1 (fynkel_mesh's SkinVertex).
struct SkinIn {
    @location(14) joints: vec4<u32>,
    @location(15) weights: vec4<f32>,
};

// The matrix that places a vertex bent by `skin`, whose skeleton's joints
// start at `first`: its joints' mixed by its weights, as three rows.
fn skin_rows(skin: SkinIn, first: u32) -> array<vec4<f32>, 3> {
    let a = joints[first + skin.joints.x];
    let b = joints[first + skin.joints.y];
    let c = joints[first + skin.joints.z];
    let d = joints[first + skin.joints.w];
    let w = skin.weights;
    return array<vec4<f32>, 3>(
        a.row_0 * w.x + b.row_0 * w.y + c.row_0 * w.z + d.row_0 * w.w,
        a.row_1 * w.x + b.row_1 * w.y + c.row_1 * w.z + d.row_1 * w.w,
        a.row_2 * w.x + b.row_2 * w.y + c.row_2 * w.z + d.row_2 * w.w,
    );
}
