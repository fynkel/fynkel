// Stage 5's comparison (docs/stage-5.md §6): package:fynkel_physics's
// example `tumble`, built the same in Unity. 1,000 cubes of 0.5 m fall in
// columns of 10, tumble and settle, and are dropped again every 8 s; a
// fixed step of 1/50 s, a sun with shadows, the same camera. It makes the
// scene itself, so the build's one scene is empty.
//
// After 3 s it counts the frames of each second for 30 s, and times each
// physics step (Physics.Simulate, called here as Unity would call it), then
// logs one line starting "TUMBLE" and writes it to tumble.txt in
// Application.persistentDataPath. The cube count is Resources/tumble.txt
// (written by Build.cs from TUMBLE_CUBES), or -cubes=N on a desktop.

using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using UnityEngine;
using Debug = UnityEngine.Debug;

public class Tumble : MonoBehaviour
{
    const float Size = 0.5f;
    const float Warmup = 3, Seconds = 30;

    int count = 1000;
    readonly List<Rigidbody> bodies = new();
    float sinceDrop;

    readonly Stopwatch clock = new();
    readonly List<double> steps = new();
    readonly List<int> perSecond = new();
    float measuring = -Warmup;
    int framesThisSecond;
    bool reported;

    [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.AfterSceneLoad)]
    static void Boot() => new GameObject("Tumble").AddComponent<Tumble>();

    void Start()
    {
        // "4.50", not "4,50", whatever the device's language.
        System.Globalization.CultureInfo.CurrentCulture = System.Globalization.CultureInfo.InvariantCulture;
        var setting = Resources.Load<TextAsset>("tumble");
        if (setting != null) count = int.Parse(setting.text.Trim());
        foreach (var arg in System.Environment.GetCommandLineArgs())
            if (arg.StartsWith("-cubes=")) count = int.Parse(arg.Substring(7));

        // As fast as the screen goes: 120 on the Mac and the iPhone, 60 on
        // the Note10+.
        QualitySettings.vSyncCount = Application.isMobilePlatform ? 0 : 1;
        Application.targetFrameRate = 120;
        Time.fixedDeltaTime = 1 / 50f;
        Physics.simulationMode = SimulationMode.Script;
        Application.runInBackground = true;
        Debug.Log($"TUMBLE START {count} cubes");

        var camera = new GameObject("Camera").AddComponent<Camera>();
        camera.transform.SetPositionAndRotation(new Vector3(0, 14, 22), Quaternion.LookRotation(new Vector3(0, -10, -22)));
        camera.fieldOfView = 60;
        camera.clearFlags = CameraClearFlags.SolidColor;
        camera.backgroundColor = new Color(0.1f, 0.12f, 0.15f);

        var sun = new GameObject("Sun").AddComponent<Light>();
        sun.type = LightType.Directional;
        sun.shadows = LightShadows.Hard;
        sun.transform.rotation = Quaternion.LookRotation(new Vector3(-0.4f, -1, -0.3f));

        var plane = GameObject.CreatePrimitive(PrimitiveType.Plane);
        Destroy(plane.GetComponent<Collider>());
        plane.transform.localScale = new Vector3(4, 1, 4); // a plane is 10 m across
        Paint(plane, new Color32(0x3A, 0x4A, 0x5A, 0xFF));
        var ground = new GameObject("Ground").AddComponent<BoxCollider>();
        ground.size = new Vector3(40, 1, 40);
        ground.center = new Vector3(0, -0.5f, 0);

        Material cubes = null;
        for (var i = 0; i < count; i++)
        {
            var cube = GameObject.CreatePrimitive(PrimitiveType.Cube);
            cube.transform.localScale = new Vector3(Size, Size, Size);
            cubes ??= Paint(cube, new Color32(0xE0, 0x61, 0x3A, 0xFF));
            cube.GetComponent<Renderer>().sharedMaterial = cubes;
            bodies.Add(cube.AddComponent<Rigidbody>());
        }
        Drop();
    }

    static Material Paint(GameObject o, Color color)
    {
        var renderer = o.GetComponent<Renderer>();
        var material = new Material(renderer.sharedMaterial) { color = color, enableInstancing = true };
        renderer.sharedMaterial = material;
        return material;
    }

    /// Every cube back in the air, as tumble's drop().
    void Drop()
    {
        var side = Mathf.CeilToInt(Mathf.Sqrt(count / 10f));
        var axis = new Vector3(1, 1, 0).normalized;
        for (var i = 0; i < count; i++)
        {
            var column = i / 10;
            var position = new Vector3(
                (column % side - side / 2f) * Size * 1.6f,
                2 + i % 10 * Size * 1.4f,
                (column / side - side / 2f) * Size * 1.6f);
            var rotation = Quaternion.AngleAxis(i * 0.37f * Mathf.Rad2Deg, axis);
            var body = bodies[i];
            body.transform.SetPositionAndRotation(position, rotation);
            body.position = position;
            body.rotation = rotation;
            body.linearVelocity = Vector3.zero;
            body.angularVelocity = Vector3.zero;
            body.WakeUp();
        }
        sinceDrop = 0;
    }

    void FixedUpdate()
    {
        clock.Restart();
        Physics.Simulate(Time.fixedDeltaTime);
        clock.Stop();
        if (measuring >= 0 && !reported) steps.Add(clock.Elapsed.TotalMilliseconds);
    }

    void Update()
    {
        sinceDrop += Time.deltaTime;
        if (sinceDrop > 8) Drop();
        if (reported) return;
        var before = measuring;
        measuring += Time.unscaledDeltaTime;
        if (measuring < 0) return;
        if (before < 0) return; // the first measured frame starts the clock
        framesThisSecond++;
        if (Mathf.Floor(measuring) > Mathf.Floor(before))
        {
            perSecond.Add(framesThisSecond);
            framesThisSecond = 0;
        }
        if (measuring >= Seconds) Report();
    }

    void Report()
    {
        reported = true;
        perSecond.Sort();
        steps.Sort();
        var line = $"TUMBLE {count} cubes, {SystemInfo.deviceModel}: fps median {perSecond[perSecond.Count / 2]}, " +
                   $"min {perSecond[0]}, max {perSecond[^1]}; step median {steps[steps.Count / 2]:F2} ms, " +
                   $"90th {steps[steps.Count * 9 / 10]:F2} ms, slowest {steps[^1]:F2} ms ({steps.Count} steps)";
        Debug.Log(line);
        File.WriteAllText(Path.Combine(Application.persistentDataPath, "tumble.txt"), line + "\n");
        if (!Application.isMobilePlatform) Application.Quit();
    }
}
