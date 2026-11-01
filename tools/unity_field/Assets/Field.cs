// Stage 14's comparison (docs/stage-14-findings.md, "Unity and Fynkel"):
// Sheepdog's field, built in examples/sheepdog/tool/field.dart from the
// same numbers. Unity's scenes are left-handed and Fynkel's right-handed,
// so every z here is Fynkel's turned round: the same view, not a mirror.
//
// 60 sheep walking in rings and the dog running round them (skinned, each
// on its clip), 120 fence posts, 8 lamps on poles with glowing bulbs, a
// sun with shadows, 500 lit dust particles, linear fog, from a camera 14 m
// up. Each quality level (Build.cs makes them to match Fynkel's) is shown
// for 20 s, the first 5 to settle, and the frames counted over the rest;
// each logs one line starting "FIELD unity", also written to field.txt in
// Application.persistentDataPath with a picture of each level.

using System.Collections;
using System.Collections.Generic;
using System.IO;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;

public class Field : MonoBehaviour
{
    const int Sheep = 60, Posts = 120, Lamps = 8;
    const float Settle = 5, Seconds = 20;
    static readonly Color Sky = Hex(0x9DB8CF);

    readonly List<(Transform walker, float radius, float speed, float start)> walkers = new();
    UniversalAdditionalCameraData cameraData;
    ParticleSystemRenderer dust;
    Material softDust, hardDust;
    float t;

    [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.AfterSceneLoad)]
    static void Boot()
    {
        // Build.CrowdIOS / CrowdAndroid build the lab's crowd (Crowd.cs) instead.
        var scene = Resources.Load<TextAsset>("scene");
        if (scene != null && scene.text.Trim() == "crowd") new GameObject("crowd").AddComponent<Crowd>();
        else new GameObject("field").AddComponent<Field>();
    }

    static Color Hex(int rgb) => new Color32((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb, 255);

    static Material Load(string name) => Resources.Load<Material>(name);

    IEnumerator Start()
    {
        Application.targetFrameRate = 60;
        QualitySettings.vSyncCount = 0;
        Make();
        var lines = new List<string>();
        foreach (var level in QualitySettings.names)
        {
            QualitySettings.SetQualityLevel(System.Array.IndexOf(QualitySettings.names, level), true);
            // Bloom, and the tone mapping HDR brings, from medium up, as Fynkel's.
            var hdr = level == "medium" || level == "high";
            cameraData.renderPostProcessing = hdr;
            dust.sharedMaterial = hdr ? softDust : hardDust;
            yield return new WaitForSecondsRealtime(Settle);
            // What it looks like at this level, to hold against Fynkel's.
            // A bare name: on a phone it goes to Application.persistentDataPath.
            ScreenCapture.CaptureScreenshot(Application.isMobilePlatform ? $"field {level}.png" : Path.Combine(Application.persistentDataPath, $"field {level}.png"));
            var main = new List<double>();
            var render = new List<double>();
            var gpu = new List<double>();
            var timings = new FrameTiming[1];
            var frames = 0;
            var from = Time.realtimeSinceStartupAsDouble;
            while (Time.realtimeSinceStartupAsDouble - from < Seconds - Settle)
            {
                yield return null;
                frames++;
                FrameTimingManager.CaptureFrameTimings();
                if (FrameTimingManager.GetLatestTimings(1, timings) == 1)
                {
                    main.Add(timings[0].cpuMainThreadFrameTime);
                    render.Add(timings[0].cpuRenderThreadFrameTime);
                    gpu.Add(timings[0].gpuFrameTime);
                }
            }
            var fps = frames / (Time.realtimeSinceStartupAsDouble - from);
            var line = $"FIELD unity {level}: {fps:F1} fps, main thread {Median(main):F2} ms, " +
                       $"render thread {Median(render):F2} ms, gpu {Median(gpu):F2} ms a frame";
            Debug.Log(line);
            lines.Add(line);
            File.WriteAllLines(Path.Combine(Application.persistentDataPath, "field.txt"), lines);
        }
        Debug.Log("FIELD done");
    }

    static double Median(List<double> times)
    {
        if (times.Count == 0) return 0;
        times.Sort();
        return times[times.Count / 2];
    }

    void Make()
    {
        var camera = new GameObject("camera").AddComponent<Camera>();
        camera.transform.position = new Vector3(0, 14, -24);
        camera.transform.rotation = Quaternion.LookRotation(new Vector3(0, -14, 24));
        camera.fieldOfView = 50;
        camera.nearClipPlane = 0.1f;
        camera.farClipPlane = 1000;
        camera.clearFlags = CameraClearFlags.SolidColor;
        camera.backgroundColor = Sky;
        camera.allowHDR = true;
        camera.allowMSAA = true;
        cameraData = camera.GetUniversalAdditionalCameraData();
        new GameObject("post").AddComponent<Volume>().sharedProfile = Resources.Load<VolumeProfile>("post");

        RenderSettings.fog = true;
        RenderSettings.fogMode = FogMode.Linear;
        RenderSettings.fogColor = Sky;
        RenderSettings.fogStartDistance = 18;
        RenderSettings.fogEndDistance = 60;
        RenderSettings.ambientMode = AmbientMode.Flat;
        RenderSettings.ambientLight = Color.white * 0.3f;
        RenderSettings.skybox = null;

        var sun = new GameObject("sun").AddComponent<Light>();
        sun.type = LightType.Directional;
        sun.transform.rotation = Quaternion.LookRotation(new Vector3(-0.4f, -1, 0.6f));
        sun.intensity = 1;
        sun.shadows = LightShadows.Soft;

        var ground = Thing(PrimitiveType.Quad, Load("ground"));
        ground.rotation = Quaternion.Euler(90, 0, 0);
        ground.localScale = new Vector3(80, 80, 1);

        var wood = Load("wood");
        for (var i = 0; i < Posts; i++)
        {
            var a = i / (float)Posts * Mathf.PI * 2;
            var post = Thing(PrimitiveType.Cube, wood);
            post.position = new Vector3(Mathf.Cos(a) * 26, 0.6f, -Mathf.Sin(a) * 26);
            post.localScale = new Vector3(0.15f, 1.2f, 0.15f);
        }
        var glass = Load("glass");
        var pole = Load("pole");
        for (var i = 0; i < Lamps; i++)
        {
            var a = (i + 0.5f) / Lamps * Mathf.PI * 2;
            float x = Mathf.Cos(a) * 18, z = -Mathf.Sin(a) * 18;
            var stick = Thing(PrimitiveType.Cube, pole);
            stick.position = new Vector3(x, 1.25f, z);
            stick.localScale = new Vector3(0.1f, 2.5f, 0.1f);
            var bulb = Thing(PrimitiveType.Sphere, glass);
            bulb.position = new Vector3(x, 2.6f, z);
            bulb.localScale = Vector3.one * 0.3f;
            var lamp = bulb.gameObject.AddComponent<Light>();
            lamp.type = LightType.Point;
            lamp.color = Hex(0xFFC880);
            lamp.intensity = 4;
            lamp.range = 10;
            lamp.shadows = LightShadows.None;
        }

        softDust = Load("dust soft");
        hardDust = Load("dust hard");
        var system = new GameObject("dust").AddComponent<ParticleSystem>();
        system.Stop(true, ParticleSystemStopBehavior.StopEmittingAndClear);
        var m = system.main;
        m.loop = true;
        m.prewarm = true;
        m.maxParticles = 500;
        m.startLifetime = 2;
        m.startSpeed = new ParticleSystem.MinMaxCurve(0.2f, 0.5f);
        m.startSize = new ParticleSystem.MinMaxCurve(0.5f, 0.8f);
        m.startColor = Hex(0xC8B48E);
        m.simulationSpace = ParticleSystemSimulationSpace.World;
        var emission = system.emission;
        emission.rateOverTime = 250;
        var shape = system.shape;
        shape.shapeType = ParticleSystemShapeType.Box;
        shape.scale = new Vector3(40, 0.2f, 40);
        dust = system.GetComponent<ParticleSystemRenderer>();
        dust.sharedMaterial = hardDust;
        system.Play();

        var sheep = Resources.Load<GameObject>("sheep");
        var dog = Resources.Load<GameObject>("dog");
        for (var i = 0; i < Sheep + 1; i++)
        {
            var isDog = i == Sheep;
            var walker = new GameObject(isDog ? "dog" : "sheep").transform;
            // As Sheepdog has them: a game's size, facing the way it goes.
            var model = Instantiate(isDog ? dog : sheep, walker).transform;
            model.localRotation = Quaternion.Euler(0, 180, 0);
            model.localScale = Vector3.one * (isDog ? 0.28f : 0.2f);
            var radius = isDog ? 14f : 4 + i % 6 * 2.5f;
            var speed = isDog ? 4f : 1.2f;
            walkers.Add((walker, radius, speed / radius, i * 2.399963f));
            var animator = model.GetComponentInChildren<Animator>();
            var clip = isDog ? "Run" : "Walk";
            var length = animator.runtimeAnimatorController.animationClips[0].length;
            animator.Play(clip, 0, i * 0.37f % length / length);
        }
        Place();
    }

    static Transform Thing(PrimitiveType type, Material material)
    {
        var thing = GameObject.CreatePrimitive(type);
        Destroy(thing.GetComponent<Collider>());
        thing.GetComponent<MeshRenderer>().sharedMaterial = material;
        return thing.transform;
    }

    void Place()
    {
        foreach (var (walker, radius, w, start) in walkers)
        {
            var a = start + t * w;
            walker.position = new Vector3(Mathf.Cos(a) * radius, 0, -Mathf.Sin(a) * radius);
            walker.rotation = Quaternion.LookRotation(new Vector3(-Mathf.Sin(a), 0, -Mathf.Cos(a)));
        }
    }

    void Update()
    {
        t += Time.deltaTime;
        Place();
    }
}
