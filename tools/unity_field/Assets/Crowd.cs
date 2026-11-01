// Stage 16's second comparison (docs/stage-16.md §2): the lab's crowd
// (examples/lab/lib/scenes/crowd.dart, measured by examples/lab/tool/
// measured.dart with CROWD=100), built the same here. 100 of KayKit's
// Knights and Rogues in a square 1.5 m apart, each on a clip of its own
// from a time of its own, on the lab's floor under its sun (with shadows)
// and ambient light, from the lab's camera. Unity's scenes are
// left-handed and Fynkel's right-handed, so every z is Fynkel's turned
// round. Each character's clip is picked by its place here, not by Dart's
// Random as the lab's are: a different mix of the same 76 clips.
//
// Built by Build.CrowdIOS / Build.CrowdAndroid, which write "crowd" into
// Resources/scene.txt; Field.Boot starts this in place of the field. Each
// quality level is shown for 20 s, the first 5 to settle; each logs one
// line starting "CROWD unity", also written to crowd.txt in
// Application.persistentDataPath.

using System.Collections;
using System.Collections.Generic;
using System.IO;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;

public class Crowd : MonoBehaviour
{
    const int Count = 100;
    const float Gap = 1.5f, Settle = 5, Seconds = 20;

    static readonly string[] KnightExtras = { "1H_Sword_Offhand", "Badge_Shield", "Rectangle_Shield", "Spike_Shield", "2H_Sword" };
    static readonly string[] RogueExtras = { "Knife_Offhand", "1H_Crossbow", "2H_Crossbow", "Knife", "Throwable" };

    static Color Hex(int rgb) => new Color32((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb, 255);

    IEnumerator Start()
    {
        Application.targetFrameRate = 60;
        QualitySettings.vSyncCount = 0;
        Make();
        var lines = new List<string>();
        foreach (var level in QualitySettings.names)
        {
            QualitySettings.SetQualityLevel(System.Array.IndexOf(QualitySettings.names, level), true);
            yield return new WaitForSecondsRealtime(Settle);
            ScreenCapture.CaptureScreenshot(Application.isMobilePlatform ? $"crowd {level}.png" : Path.Combine(Application.persistentDataPath, $"crowd {level}.png"));
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
            var line = $"CROWD unity {level}: {fps:F1} fps, main thread {Median(main):F2} ms, " +
                       $"render thread {Median(render):F2} ms, gpu {Median(gpu):F2} ms a frame";
            Debug.Log(line);
            lines.Add(line);
            File.WriteAllLines(Path.Combine(Application.persistentDataPath, "crowd.txt"), lines);
        }
        Debug.Log("CROWD done");
    }

    static double Median(List<double> times)
    {
        if (times.Count == 0) return 0;
        times.Sort();
        return times[times.Count / 2];
    }

    void Make()
    {
        // The lab's camera: at (0, 9, 15), looking at (0, 0, -1), 60°.
        var camera = new GameObject("camera").AddComponent<Camera>();
        camera.transform.position = new Vector3(0, 9, -15);
        camera.transform.rotation = Quaternion.LookRotation(new Vector3(0, -9, 16));
        camera.fieldOfView = 60;
        camera.nearClipPlane = 0.1f;
        camera.farClipPlane = 1000;
        camera.clearFlags = CameraClearFlags.SolidColor;
        camera.backgroundColor = Hex(0x20242C);
        camera.allowHDR = true;
        camera.allowMSAA = true;
        // The lab's camera has no bloom: what HDR costs, tone mapped, and no
        // more (Fynkel tone maps HDR without bloom too).
        camera.GetUniversalAdditionalCameraData().renderPostProcessing = false;

        RenderSettings.fog = false;
        RenderSettings.ambientMode = AmbientMode.Flat;
        RenderSettings.ambientLight = Color.white * 0.5f;
        RenderSettings.skybox = null;

        var sun = new GameObject("sun").AddComponent<Light>();
        sun.type = LightType.Directional;
        sun.transform.rotation = Quaternion.LookRotation(new Vector3(-0.4f, -1, 0.5f));
        sun.intensity = 2.5f;
        sun.shadows = LightShadows.Soft;

        var floor = GameObject.CreatePrimitive(PrimitiveType.Quad);
        Destroy(floor.GetComponent<Collider>());
        floor.GetComponent<MeshRenderer>().sharedMaterial = Resources.Load<Material>("floor");
        floor.transform.rotation = Quaternion.Euler(90, 0, 0);
        floor.transform.localScale = new Vector3(20, 20, 1);

        var knight = Resources.Load<GameObject>("knight");
        var rogue = Resources.Load<GameObject>("rogue");
        var columns = Mathf.Max(1, Mathf.CeilToInt(Mathf.Sqrt(Count)));
        for (var i = 0; i < Count; i++)
        {
            var isKnight = i % 2 == 0;
            var person = Instantiate(isKnight ? knight : rogue).transform;
            person.position = new Vector3((i % columns - (columns - 1) / 2f) * Gap, 0, -(i / columns - (columns - 1) / 2f) * Gap);
            // Facing the camera, as in the lab (the z turned round).
            person.rotation = Quaternion.Euler(0, 180, 0);
            foreach (var extra in isKnight ? KnightExtras : RogueExtras)
            {
                var found = Find(person, extra);
                if (found != null) found.gameObject.SetActive(false);
            }
            var animator = person.GetComponentInChildren<Animator>();
            var clips = animator.runtimeAnimatorController.animationClips;
            var clip = clips[(i * 7 + 3) % clips.Length];
            var random = new System.Random(i);
            animator.Play(clip.name, 0, (float)random.NextDouble());
        }
    }

    static Transform Find(Transform root, string name)
    {
        foreach (var t in root.GetComponentsInChildren<Transform>(true))
        {
            if (t.name == name) return t;
        }
        return null;
    }
}
