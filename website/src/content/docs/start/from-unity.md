---
title: "Coming from Unity"
description: "Unity names and their Fynkel ones, and what stays different."
---

Fynkel keeps Unity's ideas, and mostly its names:

| Unity | Fynkel |
|---|---|
| `GameObject`, a prefab | `GameObject`, a subclass of it |
| `MonoBehaviour` | `Behaviour` |
| `AddComponent<T>()`, `GetComponent<T>()` | `addComponent(T())`, `get<T>()` |
| `Destroy(go)`, `Destroy(go, t)` | `go.destroy()`, `go.destroy(after: t)` |
| `SetActive(false)`, `enabled = false` | `active = false`, `enabled = false` |
| `ObjectPool<T>`: `Get`, `Release` | `Pool(parent, make)`: `get`, `release` |
| `Awake`, `Start`, `Update`, `LateUpdate`, `OnDestroy` | `onLoad`, `onStart`, `onUpdate`, `onLateUpdate`, `onDestroy` |
| `FixedUpdate`, `Time.fixedDeltaTime` | `onFixedUpdate`, `time.fixedDelta` |
| `Time.deltaTime`, `Time.timeScale` | `time.delta`, `time.scale` |
| `Time.timeScale = 0` for a pause menu | `world.paused = true`: time and input stop |
| `Application.targetFrameRate` | `world.targetFrameRate` (60 unless set) |
| `Application.lowMemory` | `world.onLowMemory` |
| The render scale (URP), `QualitySettings` | `Quality.renderScale`, `world.quality` |
| Anti Aliasing (MSAA) | `Quality.antiAliasing`: 0, 2 or 4 |
| On-screen stick and button (`OnScreenStick`, `OnScreenButton`) | `Stick(input)`; a button is a Flutter widget calling `action.press()`/`release()` |
| `yield return new WaitForFixedUpdate()` | `await fixedStep` |
| `StartCoroutine`, `StopAllCoroutines`, `yield return new WaitForSeconds(1)` | `startRoutine(() async {...})`, `stopAllRoutines()`, `await wait(1)` |
| `WaitUntil`, `WaitWhile` | `await waitUntil(...)`, `await waitWhile(...)` |
| `FindObjectOfType<T>()`, `FindObjectsByType<T>()` | `world.first<T>()`, `world.all<T>()` (`includeInactive:` too) |
| Degrees (`Quaternion.Euler(0, 90, 0)`, joint limits, `fieldOfView`) | radians, with `90.deg`: `Quat.euler(0, 90.deg, 0)` |
| `transform.eulerAngles`, `Quaternion.FromToRotation`, `Quaternion.RotateTowards` | `rotation.eulerAngles` (radians), `Quat.fromTo`, `Quat.rotateTowards` |
| `color.a = 0.5`, `Vector2.normalized` | `color.withAlpha(0.5)`, `v.normalized` (or `normalizedOrZero`, as Unity's) |
| `Camera.main`, `ScreenPointToRay`, `WorldToScreenPoint` | `world.camera`, `screenPointToRay`, `worldToScreenPoint` |
| `RaycastHit` | `RaycastHit` (physics) and `RayHit` (`world.raycast`), each with `gameObject` |
| `transform.position`, `rotation`, `localPosition`, `localRotation`, `localScale`, `lossyScale` | the same, on the object or a behaviour |
| `transform.Translate(v)`, `Rotate(axis, angle)`, `Space.World` | `translate(v)`, `rotate(axis, angle)` (radians), `relativeTo: Space.world` |
| `transform.parent = p`, `SetParent(p, false)` | `parent = p`, `setParent(p, worldPositionStays: false)` |
| `transform.Find("name")` | `find('name')` |
| `GetComponentInParent<T>()` | `getInParent<T>()`, or `ancestor<T>()` for an object |
| `GetComponentInChildren<T>()`, `GetComponentsInChildren<T>()` | `getInChildren<T>()`, `getAllInChildren<T>()` (`includeInactive:` too) |
| `transform.childCount`, `GetChild(i)` | `childCount`, `childAt(i)` |
| C# events, `UnityEvent` | `Signal<T>` and `on(signal, ...)` |
| `Input.GetKeyDown(KeyCode.Space)`, `Input.GetKey` | `input.wasPressed(KeyCode.space)`, `input.isPressed` |
| `IPointerClickHandler` | `onClick` |
| Cinemachine | `VirtualCamera`, `Follow`, `LookAt`, `Orbit`, `world.cameras` |
| Cinemachine's Deoccluder (a camera kept out of walls), an extension | `CameraCollision` (fynkel_physics), `CameraExtension` |
| DOTween | `moveTo`, `tween`, `sequence`, `Ease` |
| `new Mesh()`, `SetVertices`, `SetTriangles`, `MarkDynamic` | `Mesh()`, `setPositions`, `setTriangles`, `markDynamic()` ([Meshes that change](/assets/meshes/)) |
| ProBuilder's shapes with settings | `Mesh.box`, `grid`, `sphere`, `cylinder`, `cone`, `torus`, `disc`, `capsule` with sizes and segments |
| ProBuilder's extrude, a lathe | `Mesh.extrude(Shape2D, depth:)` with holes, `Mesh.lathe(profile)` |
| `TextMesh` (`text`, `font`, `characterSize`, `alignment`, `lineSpacing`) | `Mesh.text(text, font:, size:, align: TextAlignment.center, lineSpacing:)`, solid with `depth`; `world.assets.font` |
| TextMeshPro's `TextMeshPro` (`text`, `fontSize`, `color`, outline, underlay) | `TextRenderer(text, font:, size:, color:, outline:, outlineColor:, shadowColor:, shadowOffset:)` ([Flat text](/assets/flat-text/)) |
| A billboard (`transform.LookAt(camera)`) | `facing: Facing.camera`, or `Facing.upright` to stay standing |
| `SpriteRenderer` (`sprite`, `color`, `flipX`, `flipY`, `sortingLayerName`, `sortingOrder`) | `SpriteRenderer(texture, color:, flipX:, flipY:, sortingLayer:, order:)`; sorting layers are numbers |
| A sprite's Pixels Per Unit, Pivot and Border (9-slice), Draw Mode Sliced | `pixelsPerMetre:`, `pivot:`, `slice: Insets(…)`; sliced whenever `slice` isn't `Insets.zero` |
| A UI drawn into a `RenderTexture` | `widget.toTexture(size:, pixelRatio:)`, drawn once ([A widget as a picture](/assets/widget-pictures/)) |
| A World Space Canvas (a health bar over a head) | `WorldWidget` ([Widgets in the world](/flutter/world-widgets/)): a widget over the view at the object, `hideBehind`, `keepOnScreen` |
| `Mesh.CombineMeshes(CombineInstance[])` | `Mesh.combine([CombineInstance(mesh, position:, ...)])`, a part each kept apart |
| Splines: `Spline`, `BezierKnot`, `TangentMode`, `Evaluate`, `GetLength`, `SplineUtility.GetNearestPoint` | `Spline` with `Knot`s and `TangentMode`, `positionAt` / `directionAt` / `upAt` / `rotationAt` by distance, `length`, `nearest` |
| `SplineContainer`, `SplineInstantiate`, `SplineAnimate` | the same three, as components |
| `SplineExtrude` | `Mesh.sweep(spline, shape)` for any profile, `setSweep` to sweep again |
| `BoxCollider`, `SphereCollider`, `CapsuleCollider`, `MeshCollider` | the same, from `fynkel_physics` |
| A non-convex `MeshCollider` on a dynamic `Rigidbody` (refused) | collides as its hull |
| `Rigidbody`, `ForceMode`, `RigidbodyConstraints`, `PhysicMaterial` | `Rigidbody`, `ForceMode`, `RigidbodyConstraints`, `PhysicsMaterial` |
| `Rigidbody.velocity` | `linearVelocity` (as Unity 6's) |
| A small ball rolling fast at 50 steps a second | physics turns a body at most a quarter turn a step: take more steps (`time.fixedDelta = 1 / 100`) |
| `MaterialPropertyBlock` for one object's colour | `renderer.color` |
| `new Material(material)` | `material.copy()` |
| Rendering Mode Fade, Transparent; an additive shader | `Material(opacity:)`, `blending: Blending.transparent` or `.additive` |
| Shader Graph, a Surface Shader, URP's Lit shader with a custom function | a `.wgsl` file with `fn surface` ([A first shader](/shaders/first/)), starting from `standard(in)` |
| An Unlit shader | `fn unlit` |
| Vertex displacement (a Shader Graph vertex position) | `fn vertex`, with the material's `reach` |
| Shader properties, `material.SetFloat/SetColor/SetTexture` | `struct Values`, `material.set(name, value)` |
| `MaterialPropertyBlock` for a shader's numbers | `struct PerObject`, `renderer.set(name, value)` |
| `_CameraDepthTexture`, `_CameraOpaqueTexture` | `depth_behind(in)`, `scene_depth(screen)`, `scene_color(screen)` |
| A custom lighting function (toon) | `fn light` |
| A full-screen pass (a renderer feature, a post-process) | `ScreenEffect`, `camera.effects` |
| Color Grading's lookup texture (a LUT) | an effect with `color_lut(table, colour)` ([Screen effects](/shaders/effects/)) |
| `_CameraDepthTexture` in a full-screen pass | `scene_depth(in.uv)`, `scene_position(in.uv)` in an effect |
| Shader warm-up, `ShaderVariantCollection.WarmUp` | `await world.shadersReady()` behind a loading screen ([Shaders in practice](/shaders/in-practice/)) |
| Bloom and SSAO (a Volume, a renderer feature) | `Camera(bloom: Bloom(...), ambientOcclusion: AmbientOcclusion(...))`, off unless given |
| Quality Settings, a URP asset per level | `world.quality = Quality(...)` or `Quality.low/medium/high.copyWith(...)` |
| `Camera.orthographicSize` | `orthographicSize`, on `Camera` and `VirtualCamera` |
| Cinemachine's binding modes (world space, lock to target, lock to target on assign/with world up) | `Follow(space: FollowSpace.world / target / targetYaw)` |
| `Input.mouseScrollDelta` | `input.scroll`: +y with the wheel turned away, as Unity's |
| `Plane`, `Plane.distance`, `GetDistanceToPoint` | `Plane`, `distance`, `distanceTo` |
| `Instantiate(prefab)` of a model | `await assets.instantiate('model.glb')` |
| `Animator.Play`, `CrossFade`, `speed`, an Animator Controller | `model.animator!.play`, `crossFade`, `speed`; the game's code is the state machine |
| A 1D blend tree; animator layers with an avatar mask, `SetLayerWeight` | `animator.blend('move', clips, thresholds:, at: speed)`; `animator.layer(1, mask: 'spine', additive:, strength:)` |
| `AudioSource`: `Play`, `Pause`, `UnPause`, `Stop`, `PlayOneShot`, `time` | `AudioSource` (from `fynkel_audio`): `play`, `pause`, `unPause`, `stop`, `playOneShot`, `playTime` |
| `AudioClip` in the project | `await assets.audio('bark.ogg')` (`.wav`, `.ogg`, `.mp3`) |
| `AudioListener.volume`, `AudioListener.pause` | `audio.volume`; `world.paused` holds the game's sounds |
| `ignoreListenerPause` | `ignorePause` |
| `spatialBlend`, `panStereo`, `minDistance`, `maxDistance`, `rolloffMode` | The same names on `AudioSource` (no custom rolloff curve) |
| An `AudioListener` on the camera | `AudioListener`; without one, the world's camera hears |
| `AudioSource.PlayClipAtPoint(clip, position)` | `audio.playAt(clip, position)` |
| The 3D sound gizmos of a selected source | `audio.drawSources = true` |
| `AudioSettings`' Mute Other Audio Sources (iOS) | `audio.session = AudioSession.playback` (on Android too, by the audio focus) |
| An `AudioMixer`'s groups, their volume and mute; `outputAudioMixerGroup` | `audio.group('music')`: `volume`, `mute`, `fade(to, seconds)`; a source's `group` |
| Music on a looping `AudioSource`, crossfades by hand | `audio.music`: `play(clip, fadeIn:)`, `crossFade(clip, seconds)`, `stop(fadeOut:)` |
| An `AudioClip`'s Load Type: Streaming | Clips over 10 s stream; `AudioClip.decode(bytes, stream:)` |
| `ParticleSystem`: main module, `emission`, `shape`, size and colour over lifetime | `ParticleSystem`: the same names; `sizeOverLifetime` a function, `colorOverLifetime` a `ColorGradient` |
| `MinMaxCurve` "Random Between Two Constants", `MinMaxGradient` "Random Between Two Colors" | `Range(min, max)`, `ColorRange(a, b)` |
| `ParticleSystem.Play`, `Stop`, `Pause`, `Clear`, `Emit`, `Simulate`, `particleCount` | `play`, `stop(clear:)`, `pause`, `clear`, `emit`, `simulate`, `particleCount` |
| Stop Action: Destroy, Disable, Callback (`OnParticleSystemStopped`) | `stopAction: StopAction.destroy`, `disable`, `callback` (`onStopped`) |
| Simulation Space: Local, World | `simulationSpace: ParticleSpace.local`, `world` |
| A particle material (Particles/Unlit), Alpha or Additive | A `Material` with `Blending.transparent` or `Blending.additive` |
| `MinMaxCurve` (Constant, Curve, Random Between Two Constants or Curves) | `ParticleCurve(v)`, `.curve(f)`, `.between(a, b)`, `.curves(f, g)` |
| Velocity, Limit Velocity, Force and Rotation over Lifetime; Noise | `velocityOverLifetime`, `limitVelocity`, `forceOverLifetime`, `rotationOverLifetime`, `noise` |
| Size by Speed, Color by Speed | `sizeBySpeed: SizeBySpeed(f, minSpeed:, maxSpeed:)`, `colorBySpeed: ColorBySpeed(gradient, …)` |
| Texture Sheet Animation (Grid) | `textureSheet: TextureSheet(columns:, rows:)` |
| Renderer's Render Mode: Billboard, Stretched, Horizontal, Vertical, Mesh | `renderMode: ParticleRenderMode.billboard`, `.stretched()`, `.horizontal`, `.vertical`, `.mesh(mesh)` |
| Particles/Lit; Soft Particles | `lit: true`; `softness` |
| Emission's Rate over Distance; Inherit Velocity | `Emission(rateOverDistance:)`; `inheritVelocity: InheritVelocity(mode:, curve:)` |
| Sub Emitters: Birth, Collision, Death; Inherit Color, Size, Rotation | `subEmitters: [SubEmitter(system, SubEmitterType.birth, inherit: {SubEmitterInherit.color})]` |
| Collision (Planes): Dampen, Bounce, Lifetime Loss, Min Kill Speed; `OnParticleCollision` | `collision: ParticleCollision(planes:, dampen:, bounce:, lifetimeLoss:, minKillSpeed:)`; `onParticleCollision(plane, count)` |
| `TrailRenderer`: Time, Min Vertex Distance, Width, Color, Emitting, `Clear()` | `TrailRenderer(time:, minVertexDistance:, widthMultiplier:, widthCurve:, colorGradient:, emitting:)`, `clear()` |
| Quality's Soft Particles | `Quality.softParticles` |
| A sound slowed with `Time.timeScale` (by setting its `pitch`) | `followTimeScale: true` |
| Animation events, `StateMachineBehaviour.OnStateExit` | `animator.at(clip, time, callback)`, `removeMark`, `removeMarks`, `animator.onEnd` |
| A character on a moving platform (parenting it, by hand) | done for you: `moveWithGround`, `ground`, `groundVelocity` |
| `Physics.gravity` | `physics.gravity` |
| `Physics.Raycast`, `SphereCast`, `OverlapSphere`, … | `physics.raycast`, `sphereCast`, `overlapSphere`, … |
| `Physics.Raycast` against what's drawn (no colliders) | `world.raycast` |
| `OnTriggerEnter(Collider)`, `OnCollisionEnter(Collision)` | `with Triggers`, `with Collisions`, then `onTriggerEnter`, `onCollisionEnter` |
| `Physics.IgnoreLayerCollision`, `Physics.IgnoreCollision` | `physics.ignoreLayerCollision`, `physics.ignoreCollision` |
| `CharacterController`, `Move`, `SimpleMove`, `CollisionFlags` | the same, as `move`, `simpleMove`, `CollisionFlags.below` |
| `OnControllerColliderHit(ControllerColliderHit)` | `with ControllerHits`, then `onControllerColliderHit` |
| Pushing rigidbodies from `OnControllerColliderHit` | done for you, by the controller's `mass` |
| `FixedJoint`, `HingeJoint`, `SpringJoint`, `CharacterJoint`, `JointLimits`, `JointMotor`, `JointSpring`, `SoftJointLimit` | the same, from `fynkel_physics` |
| `OnJointBreak(float breakForce)` | `with JointBreaks`, then `onJointBreak(joint, breakForce)` |
| `HingeJoint` with both a motor and a spring | the motor wins |
| `Debug.DrawLine` | `debug.line` |
| Scenes, the Inspector | Classes; Flutter for UI |

## What stays different

**What stays different**, on purpose or by the tools underneath:

- **Axes**: right-handed, +Y up, forward is **−Z** (`Vec3.forward`), as glTF
  and most of Flutter's 3D. Unity is left-handed with forward +Z, so a scene
  copied number for number comes out mirrored.
- **Angles are radians** everywhere, joints and `slopeLimit` too; write
  `90.deg`. `Orbit.pitch` is positive looking down.
- **Key names are Flutter's**: `KeyCode.arrowUp`, `shiftLeft`, `digit1`, not
  `UpArrow`, `LeftShift`, `Alpha1`. Letters are plain: `KeyCode.q`, not
  `keyQ`.
- **`MeshRenderer.material` is shared**, as Unity's `sharedMaterial`: change
  `renderer.color` for one object, or give it `material.copy()`.
- **`find('name')` searches everything below**, not only the children (Unity's
  `Transform.Find`), and takes no paths.
- **`Mesh.plane()` is 1 × 1**, Unity's Quad facing up; Unity's Plane is 10 ×
  10.
- **An error in `onLoad` destroys the object** ([Errors](/basics/errors/));
  one in Unity's `Awake` leaves it half-made.
- **The physics step runs between frames**
  ([Rigidbodies](/physics/rigidbodies/)), so a force shows a frame later;
  `physics.stepBetweenFrames = false` steps as Unity does.
- **A new `physics.gravity` wakes every body**; Unity leaves sleeping ones
  hanging.
- **Exit callbacks always come**: for a collider disabled, destroyed, hidden
  or moved to a layer that doesn't meet; Unity skips some.
- **A `CharacterController` turns with its object**; Unity's stays upright.
  Keep its object upright and turn a child for the model.
- **`ControllerColliderHit.moveLength` is the move asked for**, the length
  given to `move`; Unity's is how far the controller went before the hit.
- **A HUD is Flutter**: widgets over the view, not a Canvas in the scene.
