//! What a frame drew and cost ([`FrameStats`]).

/// Counts from the last `prepare`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Instances drawn in the scene passes (inside the camera frustum).
    pub instances: u32,
    /// Instances outside the camera frustum, not drawn in the scene (they
    /// may still cast into the shadow map).
    pub culled: u32,
    /// Records the scene camera's `culling_mask` leaves out (ADR-0020):
    /// neither drawn nor casting a shadow.
    pub layer_hidden: u32,
    /// Cooked meshes among the records (before culling).
    pub meshes: u32,
    pub placeholders: u32,
    pub missing: u32,
    /// Distinct opaque `(mesh, material)` batches.
    pub batches: u32,
    /// Mesh draw calls in the scene pass: opaque batches with something
    /// in view, plus one per transparent instance.
    pub draws: u32,
    /// Instances drawn skinned (a joint palette applied per vertex), and
    /// the joint matrices uploaded for them this frame.
    pub skinned: u32,
    pub joints: u32,
    /// Joint influence spheres evaluated for animated culling this frame.
    pub bounds_joints: u32,
    /// Draw candidates kept because no valid finite deformation bound exists.
    pub bounds_unbounded: u32,
    /// `blend` instances drawn back-to-front (one draw each).
    pub transparent: u32,
    /// Instances drawn into the shadow map (0 when no shadow was drawn).
    pub shadow_casters: u32,
    /// Draw calls in the shadow pass.
    pub shadow_draws: u32,
    /// The shadow map side drawn this frame (0: no shadow — the profile
    /// has shadows off, or no sun casts).
    pub shadow_map: u32,
    /// Cascades drawn into that map's 2×2 atlas (0 when no shadow was
    /// drawn); each one is `shadow_map / 2` on a side.
    pub shadow_cascades: u32,
    /// Lines the overlay drew: the [`Overlay`] queue's and the
    /// [`DebugDraw`] queue's (ADR-0019).
    pub debug_lines: u32,
    /// Triangles the overlay drew.
    pub overlay_triangles: u32,
    /// Whether the overlay drew the ground grid.
    pub grid: bool,
    /// Instances drawn into the selection outline's mask (0: the frame
    /// has no outline, and no outline passes).
    pub outlined: u32,
    /// [`ExtraWorld`]s drawn beside the main extract this frame.
    pub extra_worlds: u32,
    /// Whether the frame drew ambient occlusion (R2) in the screen-space
    /// stage.
    pub ambient_occlusion: bool,
    /// Whether the frame ran the bloom stage (R3b).
    pub bloom: bool,
    /// Whether the frame marched contact shadows (R3d) in the
    /// screen-space stage.
    pub contact_shadows: bool,
    /// Whether the post pass graded the frame (R3c): a volume set more
    /// than the tone mapper.
    pub grading: bool,
    /// Volumes (R3a) whose profile reached the camera this frame.
    pub volumes: u32,
    /// Samples per pixel of the scene passes (1 or 4).
    pub msaa: u32,
    /// The internal render size (target × render scale).
    pub render_width: u32,
    pub render_height: u32,
    /// Point + spot lights uploaded to the frame's pool (≤ [`LIGHT_POOL`]):
    /// those whose range reaches the view.
    pub lights: u32,
    /// Point + spot lights that reached the view but did not fit the pool.
    pub lights_dropped: u32,
    /// The longest per-object light list this frame (≤ [`MAX_LIGHTS`]).
    pub lights_max_per_object: u32,
    /// Instances that had more lights on them than [`MAX_LIGHTS`]: the
    /// weakest at their bounds were left out.
    pub lights_overflow: u32,
    pub scene_camera: bool,
    pub scene_light: bool,
    /// The `Environment` sky is drawn behind the meshes.
    pub sky: bool,
    /// The sky is an HDR environment and lights the scene (E1): the
    /// prefiltered cubes are bound and the constant ambient is off.
    pub ibl: bool,
    /// Instrumentation (blueprint §15.4, M3-16). Wall time of `prepare`
    /// (upload + cull + the timestamp read-back) and of the last `draw`
    /// (pass recording; the submit is the host's), in microseconds.
    pub prepare_us: u32,
    pub encode_us: u32,
    /// Meshes, textures and materials a background load made resident
    /// that this frame put on the GPU ahead of their first draw
    /// (ADR-0038 §5).
    pub warmed: u32,
    /// Estimated GPU memory held: resident meshes and textures, the
    /// attachments at the internal size, the shadow map, the instance,
    /// joint and debug buffers.
    pub gpu_bytes: u64,
    /// The device samples timestamps at pass boundaries
    /// (`wgpu::Features::TIMESTAMP_QUERY`; Metal on Apple GPUs). Without
    /// it the four GPU times stay 0.
    pub gpu_timestamps: bool,
    /// GPU time of the most recently read-back frame's passes (the
    /// read-back trails the draw by three or four frames): the shadow
    /// pass, the ambient-occlusion stage (the depth prepass, SSAO and its
    /// blur, timed as one span), the scene pass (opaque + sky +
    /// transparent, resolved), post (+ present), and the frame's span
    /// from the first pass's begin to the last pass's end — less than the
    /// sum on a tile-based GPU, whose passes overlap.
    pub gpu_shadow_us: u32,
    pub gpu_ao_us: u32,
    pub gpu_scene_us: u32,
    pub gpu_post_us: u32,
    pub gpu_frame_us: u32,
    /// Frames drawn whose timestamps could not be taken because every
    /// read-back slot was still in flight (the GPU more than three frames
    /// behind); cumulative.
    pub gpu_untimed: u32,
    /// Frames whose timestamps have been read back; cumulative, so a host
    /// sampling per frame knows when the four times are new.
    pub gpu_timed: u32,
}
