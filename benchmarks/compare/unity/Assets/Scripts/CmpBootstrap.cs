// Engine comparison, run C4 (docs/engine-comparison.md, "Rules").
//
// The whole scene is built here, in code. The editor script (CmpSetup) only
// imports the glTF files (glTFast, at import time), creates the URP asset /
// renderer / volume profile and a scene holding this one component with the
// references filled in.
//
// Coordinates: the spec is written in glTF space (right-handed, Y up).
// glTFast converts glTF to Unity's left-handed space by negating X, so every
// spec position/direction (x, y, z) is used here as (-x, y, z). That keeps
// the picture identical (not mirrored) to a right-handed engine's.
//
// Light units: URP has no physical camera exposure, and its BRDF is pi-scaled
// (Lambert diffuse = albedo, no 1/pi). Fynkel (Frostbite conventions) shades
// in nits: L = albedo/pi * E, then multiplies by 1 / (1.2 * 2^EV100). So the
// URP intensity that gives the same pre-tonemap value is
//     I_urp = physical * exposure / pi,   exposure = 1 / (1.2 * 2^12) = 1/4915.2
//   sun     10 000 lux                     -> 10000 / 4915.2 / pi       = 0.6476
//   point   2 000 lm = 2000/(4 pi) cd       -> 159.155 / 4915.2 / pi     = 0.01031
//   ambient 2 000 lux, uniform (E = pi L)   -> SH constant 2000/pi/4915.2 = 0.1295
// with GraphicsSettings.lightsUseLinearIntensity = true and light colours
// given in linear (converted with .gamma because Light.color is sRGB-space).

using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using UnityEngine;
using UnityEngine.Animations;
using UnityEngine.Playables;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;

public class CmpBootstrap : MonoBehaviour
{
    public GameObject sponzaPrefab;
    public GameObject characterPrefab;
    public AnimationClip walkClip;
    public UniversalRenderPipelineAsset urpAsset;
    public UniversalRendererData rendererData;
    public VolumeProfile volumeProfile;

    const float Ev100 = 12f;
    const int CharacterCount = 20;
    const float ShotDelay = 5f;
    const float ShotClipTime = 0.5f;

    // Camera clear colour. The spec wants the final pixel to be sRGB
    // (0.5, 0.6, 0.8) (Fynkel writes it after tone mapping). URP clears the
    // HDR buffer, which then goes through ACES, so this is the linear HDR
    // value that ACES maps to that pixel (found by measuring a shot; see
    // README).
    public static readonly Color ClearLinear = new Color(0.268f, 0.419f, 0.965f);

    string level = "full";
    float scale = 1f;
    string scaleText = "1.0";
    bool shot;
    bool useNsLog;
    bool debug;

    Camera cam;
    readonly List<AnimationClipPlayable> clips = new List<AnimationClipPlayable>();
    readonly List<PlayableGraph> graphs = new List<PlayableGraph>();
    float startTime;

    // Stats, one window per second.
    readonly FrameTiming[] timing = new FrameTiming[1];
    readonly List<double> cpuSamples = new List<double>(256);
    readonly List<double> gpuSamples = new List<double>(256);
    int frames;
    float windowStart;

#if UNITY_IOS && !UNITY_EDITOR
    [DllImport("__Internal")] static extern int cmp_thermal_state();
    [DllImport("__Internal")] static extern void cmp_print(string line, int useNsLog);
#endif

    void Awake()
    {
        ReadConfig();
        Application.targetFrameRate = 120;
        QualitySettings.vSyncCount = 0;
        // macOS only (iOS ignores vSyncCount and paces with CADisplayLink):
        // CMP_VSYNC=1 paces with vsync instead of the targetFrameRate sleep.
        if (Environment.GetEnvironmentVariable("CMP_VSYNC") == "1") { QualitySettings.vSyncCount = 1; Application.targetFrameRate = -1; }
        if (Environment.GetEnvironmentVariable("CMP_UNCAPPED") == "1") Application.targetFrameRate = -1;
        Screen.sleepTimeout = SleepTimeout.NeverSleep;
        ConfigurePipeline();
        BuildScene();
        InstallLoopBrackets();
    }

    void ReadConfig()
    {
        var l = Environment.GetEnvironmentVariable("CMP_LEVEL");
        if (!string.IsNullOrEmpty(l))
        {
            l = l.Trim().ToLowerInvariant();
            if (l == "plain" || l == "full" || l == "msaa") level = l;
        }
        var s = Environment.GetEnvironmentVariable("CMP_SCALE");
        if (!string.IsNullOrEmpty(s) && float.TryParse(s.Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out var v) && v > 0.05f && v <= 2f)
            scale = v;
        scaleText = scale.ToString("0.0#", CultureInfo.InvariantCulture);
        shot = Environment.GetEnvironmentVariable("CMP_SHOT") == "1";
        useNsLog = Environment.GetEnvironmentVariable("CMP_NSLOG") == "1";
        debug = Environment.GetEnvironmentVariable("CMP_DEBUG") == "1";
    }

    // ---- Settings levels ------------------------------------------------

    void ConfigurePipeline()
    {
        GraphicsSettings.lightsUseLinearIntensity = true;
        GraphicsSettings.defaultRenderPipeline = urpAsset;
        QualitySettings.renderPipeline = urpAsset;

        bool full = level != "plain";
        urpAsset.renderScale = scale;
        urpAsset.msaaSampleCount = level == "msaa" ? 4 : 1;
        urpAsset.supportsHDR = true;
        urpAsset.mainLightShadowmapResolution = 2048;
        // All levels: 4 cascades (spec change 2026-10-06: Fynkel has a
        // fixed 4). Distance and splits as Fynkel's shadow.rs: 60 m,
        // practical split lambda 0.75 for near 0.1 -> 4.14 / 9.35 / 20.35 m.
        urpAsset.shadowCascadeCount = 4;
        urpAsset.shadowDistance = 60f;
        urpAsset.cascade4Split = new Vector3(4.14f / 60f, 9.35f / 60f, 20.35f / 60f);

        foreach (var f in rendererData.rendererFeatures)
        {
            if (f is ScreenSpaceAmbientOcclusion ssao)
            {
                f.SetActive(full);
                TuneAo(ssao);
            }
        }

        if (volumeProfile.TryGet<Bloom>(out var bloom)) bloom.active = full;
        if (volumeProfile.TryGet<Tonemapping>(out var tm))
        {
            tm.active = true;
            tm.mode.Override(TonemappingMode.ACES);
        }
    }

    // CMP_AO="radius,intensity" overrides the SSAO feature's settings (a
    // tuning knob; the defaults are written by CmpSetup).
    static void TuneAo(ScreenSpaceAmbientOcclusion ssao)
    {
        var env = Environment.GetEnvironmentVariable("CMP_AO");
        if (string.IsNullOrEmpty(env)) return;
        var parts = env.Split(',');
        var flags = System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance;
        var settings = typeof(ScreenSpaceAmbientOcclusion).GetField("m_Settings", flags)?.GetValue(ssao);
        if (settings == null) return;
        var t = settings.GetType();
        if (parts.Length > 0) t.GetField("Radius", flags)?.SetValue(settings, float.Parse(parts[0], CultureInfo.InvariantCulture));
        if (parts.Length > 1) t.GetField("Intensity", flags)?.SetValue(settings, float.Parse(parts[1], CultureInfo.InvariantCulture));
    }

    // ---- Scene ----------------------------------------------------------

    static float Exposure => 1f / (1.2f * Mathf.Pow(2f, Ev100));

    static Vector3 Spec(float x, float y, float z) => new Vector3(-x, y, z);

    void BuildScene()
    {
        // Environment: no skybox, no reflection, flat ambient (2 000 lux).
        RenderSettings.skybox = null;
        RenderSettings.ambientMode = AmbientMode.Flat;
        float amb = 2000f / Mathf.PI * Exposure;
        var ambLinear = new Color(amb, amb, amb);
        RenderSettings.ambientLight = ambLinear.gamma;
        var sh = new SphericalHarmonicsL2();
        sh.AddAmbientLight(ambLinear);
        RenderSettings.ambientProbe = sh;
        // The same uniform ambient as the specular environment: a constant
        // cubemap of the ambient radiance. Fynkel's constant ambient lights
        // the specular term too ((diffuse + f0) * ambient / pi), Bevy's
        // AmbientLight as well; URP's flat ambient is diffuse-only, and with
        // no skybox its default reflection is black (metals go black).
        // CMP_AMBIENT_SPEC=0 turns it off.
        RenderSettings.defaultReflectionMode = DefaultReflectionMode.Custom;
        if (Environment.GetEnvironmentVariable("CMP_AMBIENT_SPEC") == "0")
        {
            RenderSettings.customReflectionTexture = null;
            RenderSettings.reflectionIntensity = 0f;
        }
        else
        {
            RenderSettings.customReflectionTexture = UniformCube(ambLinear);
            RenderSettings.reflectionIntensity = 1f;
        }
        RenderSettings.fog = false;

        // Volume: ACES + Bloom (bloom only in full/msaa).
        var volGo = new GameObject("Volume");
        var vol = volGo.AddComponent<Volume>();
        vol.isGlobal = true;
        vol.priority = 100;
        vol.sharedProfile = volumeProfile;

        // Camera.
        var camGo = new GameObject("Camera");
        cam = camGo.AddComponent<Camera>();
        cam.fieldOfView = 60f;
        cam.nearClipPlane = 0.1f;
        cam.farClipPlane = 100f;
        cam.clearFlags = CameraClearFlags.SolidColor;
        var clear = ClearLinear;
        var clearEnv = Environment.GetEnvironmentVariable("CMP_CLEAR");
        if (!string.IsNullOrEmpty(clearEnv))
        {
            var c = Array.ConvertAll(clearEnv.Split(','), p => float.Parse(p, CultureInfo.InvariantCulture));
            clear = new Color(c[0], c[1], c[2]);
        }
        cam.backgroundColor = clear.gamma;
        cam.allowHDR = true;
        cam.allowMSAA = true;
        var camData = camGo.AddComponent<UniversalAdditionalCameraData>();
        camData.renderPostProcessing = true;
        camData.antialiasing = AntialiasingMode.None;
        camData.renderShadows = true;
        camData.volumeLayerMask = ~0;
        UpdateCamera(0f);

        // Sponza (keeps its own 0.008 node scale from the file).
        Instantiate(sponzaPrefab).name = "Sponza";

        // Sun.
        var sunGo = new GameObject("Sun");
        var sun = sunGo.AddComponent<Light>();
        sun.type = LightType.Directional;
        sun.color = Color.white;
        sun.intensity = 10000f * Exposure / Mathf.PI;
        sun.shadows = Environment.GetEnvironmentVariable("CMP_NOSHADOW") == "1" ? LightShadows.None : LightShadows.Soft;
        sunGo.transform.rotation = Quaternion.LookRotation(Spec(-0.3f, -1.0f, -0.25f).normalized);
        RenderSettings.sun = sun;

        // Eight point lights.
        float pointCd = 2000f / (4f * Mathf.PI);
        // CMP_POINT_GAIN: debug multiplier to see where the lights are.
        float pointGain = 1f;
        var gainEnv = Environment.GetEnvironmentVariable("CMP_POINT_GAIN");
        if (!string.IsNullOrEmpty(gainEnv)) pointGain = float.Parse(gainEnv, CultureInfo.InvariantCulture);
        var warm = new Color(1.0f, 0.8f, 0.6f).gamma;
        int n = 0;
        foreach (var x in new[] { -10.5f, -3.5f, 3.5f, 10.5f })
        foreach (var z in new[] { -3f, 3f })
        {
            var go = new GameObject("Point" + n++);
            go.transform.position = Spec(x, 1.5f, z);
            var l = go.AddComponent<Light>();
            l.type = LightType.Point;
            l.color = warm;
            l.intensity = pointCd * Exposure / Mathf.PI * pointGain;
            l.range = 6f;
            l.shadows = LightShadows.None;
        }

        // Twenty characters, each playing `walk` through a Playable graph
        // (Mecanim) whose time is set from t every frame.
        for (int i = 0; i < CharacterCount; i++)
        {
            var c = Instantiate(characterPrefab);
            c.name = "Character" + i;
            c.transform.SetPositionAndRotation(Spec(-9f + i * 18f / 19f, 0f, i % 2 == 0 ? 1.2f : -1.2f), Quaternion.identity);
            var animator = c.GetComponentInChildren<Animator>();
            if (animator == null) animator = c.AddComponent<Animator>();
            animator.cullingMode = AnimatorCullingMode.AlwaysAnimate;
            var playable = AnimationPlayableUtilities.PlayClip(animator, walkClip, out var graph);
            playable.SetSpeed(0);
            playable.SetApplyFootIK(false);
            clips.Add(playable);
            graphs.Add(graph);
        }
        UpdateCharacters(0f);
    }

    static Cubemap UniformCube(Color linear)
    {
        const int size = 16;
        var cube = new Cubemap(size, TextureFormat.RGBAHalf, true);
        for (int mip = 0; mip < cube.mipmapCount; mip++)
        {
            int s = Mathf.Max(1, size >> mip);
            var px = new Color[s * s];
            for (int i = 0; i < px.Length; i++) px[i] = linear;
            for (int face = 0; face < 6; face++) cube.SetPixels(px, (CubemapFace)face, mip);
        }
        cube.Apply(false, true);
        return cube;
    }

    void UpdateCamera(float t)
    {
        // CMP_CAM="x,y,z,dx,dy,dz" (spec space): a fixed debug camera.
        var dbg = Environment.GetEnvironmentVariable("CMP_CAM");
        if (!string.IsNullOrEmpty(dbg))
        {
            var v = Array.ConvertAll(dbg.Split(','), p => float.Parse(p, CultureInfo.InvariantCulture));
            cam.transform.position = Spec(v[0], v[1], v[2]);
            cam.transform.rotation = Quaternion.LookRotation(Spec(v[3], v[4], v[5]).normalized, Vector3.up);
            return;
        }
        float phi = 2f * Mathf.PI * t / 30f;
        cam.transform.position = Spec(10f * Mathf.Sin(phi), 2.0f, 0f);
        var dir = Spec(Mathf.Cos(phi), -0.05f, Mathf.Sin(phi)).normalized;
        cam.transform.rotation = Quaternion.LookRotation(dir, Vector3.up);
    }

    void UpdateCharacters(float t)
    {
        float len = walkClip.length;
        for (int i = 0; i < clips.Count; i++)
        {
            double time = shot ? ShotClipTime : Mathf.Repeat(t + i * 0.1f, len);
            clips[i].SetTime(time);
        }
    }

    // ---- Frame loop -----------------------------------------------------

    void Start()
    {
        startTime = Time.realtimeSinceStartup;
        windowStart = startTime;
        Emit($"[cmp-info] engine=unity level={level} scale={scaleText} shot={(shot ? 1 : 0)} screen={Screen.width}x{Screen.height} gfx={SystemInfo.graphicsDeviceType} timing={FrameTimingManager.IsFeatureEnabled()}");
        if (shot) StartCoroutine(TakeShot());
    }

    void Update()
    {
        float t = shot ? 0f : Time.realtimeSinceStartup - startTime;
        UpdateCamera(t);
        UpdateCharacters(t);
        Sample();
    }

    // Main-thread work, bracketed in the player loop: from the start of
    // EarlyUpdate (after Initialization, where the targetFrameRate /
    // display-link wait happens) to the end of PostLateUpdate (after the
    // frame's rendering is submitted). FrameTimingManager's
    // cpuMainThreadFrameTime includes that wait, so it cannot be used.
    static readonly System.Diagnostics.Stopwatch clock = System.Diagnostics.Stopwatch.StartNew();
    static long loopBegin;
    static double lastMainMs;

    static void InstallLoopBrackets()
    {
        var loop = UnityEngine.LowLevel.PlayerLoop.GetCurrentPlayerLoop();
        for (int i = 0; i < loop.subSystemList.Length; i++)
        {
            var sys = loop.subSystemList[i];
            var list = new List<UnityEngine.LowLevel.PlayerLoopSystem>(sys.subSystemList ?? Array.Empty<UnityEngine.LowLevel.PlayerLoopSystem>());
            if (sys.type == typeof(UnityEngine.PlayerLoop.EarlyUpdate))
                list.Insert(0, new UnityEngine.LowLevel.PlayerLoopSystem { type = typeof(CmpBootstrap), updateDelegate = () => loopBegin = clock.ElapsedTicks });
            else if (sys.type == typeof(UnityEngine.PlayerLoop.PostLateUpdate))
                list.Add(new UnityEngine.LowLevel.PlayerLoopSystem { type = typeof(CmpBootstrap), updateDelegate = () =>
                {
                    if (loopBegin != 0) lastMainMs = (clock.ElapsedTicks - loopBegin) * 1000.0 / System.Diagnostics.Stopwatch.Frequency;
                } });
            else continue;
            sys.subSystemList = list.ToArray();
            loop.subSystemList[i] = sys;
        }
        UnityEngine.LowLevel.PlayerLoop.SetPlayerLoop(loop);
    }

    void Sample()
    {
        // The previous frame's main-thread work (this frame's is not over).
        double main = lastMainMs;
        FrameTimingManager.CaptureFrameTimings();
        if (FrameTimingManager.GetLatestTimings(1, timing) > 0)
        {
            var ft = timing[0];
            double cpu = Math.Max(main, ft.cpuRenderThreadFrameTime);
            if (cpu > 0) cpuSamples.Add(cpu);
            if (debug && frames % 120 == 0)
                Emit(string.Format(CultureInfo.InvariantCulture, "[cmp-debug] bracket={5:0.00} frame={0:0.00} main={1:0.00} presentWait={2:0.00} render={3:0.00} gpu={4:0.00}",
                    ft.cpuFrameTime, ft.cpuMainThreadFrameTime, ft.cpuMainThreadPresentWaitTime, ft.cpuRenderThreadFrameTime, ft.gpuFrameTime, main));
            if (ft.gpuFrameTime > 0) gpuSamples.Add(ft.gpuFrameTime);
        }
        frames++;

        float now = Time.realtimeSinceStartup;
        float elapsed = now - windowStart;
        if (elapsed >= 1f)
        {
            var ci = CultureInfo.InvariantCulture;
            double fps = frames / elapsed;
            string line = string.Format(ci,
                "[cmp] engine=unity level={0} scale={1} fps={2:0.0} cpu_ms={3:0.00} gpu_ms={4:0.00} gpu_p95={5:0.00} thermal={6}",
                level, scaleText, fps, Percentile(cpuSamples, 0.5), Percentile(gpuSamples, 0.5), Percentile(gpuSamples, 0.95), Thermal());
            Emit(line);
            cpuSamples.Clear();
            gpuSamples.Clear();
            frames = 0;
            windowStart = now;
        }
    }

    static double Percentile(List<double> v, double p)
    {
        if (v.Count == 0) return 0;
        v.Sort();
        int idx = (int)Math.Ceiling(p * v.Count) - 1;
        return v[Mathf.Clamp(idx, 0, v.Count - 1)];
    }

    static int Thermal()
    {
#if UNITY_IOS && !UNITY_EDITOR
        return cmp_thermal_state();
#else
        return 0;
#endif
    }

    void Emit(string line)
    {
#if UNITY_IOS && !UNITY_EDITOR
        cmp_print(line, useNsLog ? 1 : 0);
#elif UNITY_EDITOR
        Debug.Log(line);
#else
        Console.WriteLine(line);
        Console.Out.Flush();
#endif
    }

    IEnumerator TakeShot()
    {
        float until = Time.realtimeSinceStartup + ShotDelay;
        while (Time.realtimeSinceStartup < until) yield return null;
        yield return new WaitForEndOfFrame();
        var tex = ScreenCapture.CaptureScreenshotAsTexture();
        var png = tex.EncodeToPNG();
        Destroy(tex);
        var name = $"cmp_shot_unity_{level}_{scaleText}.png";
        var path = Path.Combine(Application.persistentDataPath, name);
        File.WriteAllBytes(path, png);
        Emit($"[cmp-shot] engine=unity path={path}");
    }

    void OnDestroy()
    {
        foreach (var g in graphs) if (g.IsValid()) g.Destroy();
    }
}
