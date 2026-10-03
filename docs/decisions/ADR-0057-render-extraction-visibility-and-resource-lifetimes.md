# ADR-0057: Render extraction, spatial visibility, lighting and GPU lifetimes

Status: Accepted

Date: 2026-10-02

Scope: F6.1–F6.4; extends ADR-0055 and ADR-0056. Viewport ABI 14 supersedes ABI 13; gameplay component and document schemas are unchanged.

## Context

Maintained spatial membership still published every spatial object and allocated new extraction buffers each frame. Draw preparation did per-object work before rejecting whole off-screen groups. The old local-light path scanned a fixed 64-light uniform pool per object and encoded eight indices as bytes. Device loss required an explicit ownership/recreation policy across renderer resources and Flutter texture registration.

## Decisions

### One extraction implementation and owned frame data

`World::extract_into(out, placeholders)` refreshes maintained render membership, clears and reuses output vectors, and resolves transforms through retained memo capacity. The output owns its matrices, component values and asset references; no ECS borrow or pointer escapes. When placeholders are disabled, only owners of render components produce records. Ancestors and skin joints are resolved as dependencies without unnecessary draw records. The explicit placeholder mode includes empty spatial objects for editor/debug rendering. `extract()` is an allocating convenience wrapper over this implementation, not another publication authority.

Apple and standalone frame loops retain their output buffers. Snapshot cloning remains valid for extra worlds, asynchronous consumers and callers retaining a previous frame: mutating the next output cannot mutate an already-owned snapshot. No parallel mutable renderer or ECS access is introduced.

### Persistent conservative visibility

Use a balanced sphere BVH over current draw bounds. Membership changes rebuild; sparse bound changes refit ancestor chains; dense changes refit bottom-up. When summed finite node radius-squared cost doubles relative to the build, rebuild spatially. Queries reject disjoint nodes and accept fully contained subtrees. Unknown/non-finite bounds remain visible.

Query camera and shadow frusta independently. A caster outside the camera can still contribute to the shadow map and cascade fit. Mesh/material replacement invalidates the GPU descriptions used for the visibility prepass. Before those descriptions are available, draws remain unbounded and continue requesting resources. Skins use the existing conservative union of transformed joint-influence bounds, including extra-world placement and shear; finite posed bounds are reused by instance preparation. Custom vertex displacement has no authored displacement bound, so it is deliberately unbounded rather than incorrectly culled.

Occlusion culling and automatic LOD are not part of this decision. The qualification workloads separately exercise 10,000 records mostly outside the frustum and 10,000 simultaneously visible cubes at 1280×720. Frustum hierarchy rejection and instancing address their measured costs. Neither workload establishes an occlusion bottleneck or a high-poly asset LOD requirement. Add either feature only with an appropriate content workload and correctness gate; their absence is an explicit F6 decision, not a placeholder implementation.

### Spatial forward lighting

Replace the fixed light uniform array with a growable read-only storage buffer. Full-width indices eliminate byte-index truncation. Each instance has a 48-byte list: eight u32 indices, count, alignment padding. The vertex stage forwards its list offset/count and the fragment stage reads only the selected entries. Shader layout tests compare CPU and WGSL frame/light record sizes; a pixel regression addresses index 300.

A second persistent sphere BVH indexes local-light influence bounds. Per-object selection visits overlapping candidates, applies layer masks, and ranks the shader's windowed inverse-square contribution at the object's bounds. Equal scores use stable pool index order. Camera-masked and out-of-frustum lights do not consume the frame budget. Frame overflow admits nearest-to-camera lights with stable source-order ties.

Default: 1,024 local lights per frame and eight per object. Supported explicit frame limit: 16,384 (one MiB of light records); per-object limit remains eight. Unsupported budgets are rejected atomically. Both Rust and Flutter expose the budget. Counters expose frame drops and per-object overflow; Rust diagnostics explain them. A high-overlap object intentionally receives only its strongest eight local contributions. Local shadow maps and arbitrary numbers of simultaneous per-pixel contributions are not claimed.

Choose this spatial forward path instead of GPU tiled/clustered lighting for F6. It removes the global 64-light bottleneck and the per-object full-pool scan without adding compute/pass synchronization. Qualification includes 1,024 admitted lights, 10,000 visible objects, real GPU timestamps, and candidate counts versus the all-pairs upper bound. A future tiled/clustered decision must demonstrate a workload where this bounded forward contract is insufficient; raising constants alone is not that decision.

### Explicit pass and resource lifetimes

Keep the existing explicit pass chain rather than introduce a general render graph. The renderer has a fixed ordering and no transient resource alias allocator. `passes.rs` is the execution authority: shadow → optional depth/AO → opaque/sky/transparent HDR → optional bloom → post/presentation → outline/overlay. Optional passes consume resources rebuilt for their current sample count and dimensions. There is no second scheduling representation to drift.

| Owner/lifetime | Resources | Invalidation |
| --- | --- | --- |
| Device/renderer | layouts, pipelines, samplers, fallback textures, query/readback slots | rebuild all on replacement device |
| Target/quality | HDR/depth/resolve/AO/bloom/presentation/outline attachments | resize or relevant quality change; unchanged resize is a no-op |
| Asset residency | meshes, textures, material bindings, shader pipelines, IBL | F5 ownership, edits, eviction and explicit clear |
| Frame capacity | instance, palette, light-list and local-light buffers | geometric growth and bind-group rebuild; capacities counted in GPU bytes |
| Prepared frame | instances, per-object light lists, camera/lighting uniforms | next prepare, after the host submitted the prior encoded frame |
| Presenter | IOSurfaces/CVPixelBuffers and completion callbacks | unregister before native destruction; old retained surfaces outlive raster use; completion generations reject stale writes |

Hosts must encode and submit a prepared frame before preparing the next one. Queue ordering and wgpu-owned command resources protect in-flight GPU references; no mutable CPU snapshot is shared with a worker. Existing GPU submission/presentation overlap remains enabled. Additional CPU render-thread overlap is not introduced without a separately owned frame/resource publication protocol.

`ExtractRenderer::recreate(device, format)` rebuilds GPU ownership while preserving logical camera, quality/profile, culling, lighting/upload budgets, asset store and queued overlay/extra-world inputs. The host recreates the target/surface too. Apple reports device loss as -5 before GPU preparation; the viewport is not poisoned merely because the device was lost. Flutter `recover()` unregisters the old texture, destroys the viewport, creates a new device/viewport and registers new surfaces, restoring controller settings without replacing the world. Teardown publishes its guard before notifying listeners and awaits any pending registration or rebind before unregistering/freeing. A failed rebind or missing texture id is an explicit failure, and recovery preserves a failed resize request. Disposed controllers cannot recreate. The viewport widget replaces a failed texture with an explicit retry action. Failed texture unregistration retains native ownership and the old texture identity, so retry cannot free resources still used by the raster thread. Standalone recreates its device/surface/renderer after its loss callback; inability to obtain a replacement device remains a fatal host startup/recovery error.

### Readiness is a property of the prepared frame

Cache readiness at the end of preparation is insufficient: a worker can finish between two objects sharing an asset, leaving an earlier object already assembled with a fallback. Track whether required draw preparation deferred any resource or upload. Explicit readiness and idle redraw remain pending until a complete preparation used ready resources. Unrelated background warming does not set this draw-frame requirement. The regression controls worker completion between the earlier miss and the later GPU cache hit.

## Qualification and limits

See [the F6 gate report](../foundation-f6-gate-report.md) and its machine-readable evidence for measured extraction, preparation, encoding, GPU timing, resource bytes, failures and retries. These fixtures qualify architecture and supported budgets on the recorded machine. They are not a claim that every production asset set, shadow workload or platform sustains the same frame rate.
