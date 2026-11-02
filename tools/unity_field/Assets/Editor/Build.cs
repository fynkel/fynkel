// Builds tools/unity_field for a device, from the command line:
//
//   Unity -batchmode -quit -projectPath tools/unity_field -executeMethod Build.IOS|Android|Mac
//
// It first makes what the scene uses, in Assets/Resources (git-ignored):
// URP's quality levels made to match Fynkel's (Quality.low, low with 2x
// and with no anti-aliasing, medium, high: the same HDR, render scale,
// anti-aliasing, shadow maps, cascades and softness, shadow distance,
// ambient occlusion only at high, bloom and tone mapping from medium, soft
// particles where Fynkel has them), URP's own Lit and particle materials
// in the same colours as Fynkel's standard ones, and the sheep and the
// dog from Sheepdog's glTF files with their walk and run (glTFast, its
// materials swapped for URP's Lit, as Fynkel draws a glTF's with its own).
// The builds land in tools/unity_field/Build (git-ignored).
//
// Build.CrowdIOS|CrowdAndroid build the lab's crowd instead (Crowd.cs,
// stage 16): KayKit's Knight and Rogue copied from examples/lab/assets/
// kaykit into Assets/Crowd (git-ignored), every clip a state of their
// controller, skinned on the GPU as Fynkel skins them.

using System;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.Animations;
using UnityEditor.Rendering.Universal.ShaderGUI;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;

public static class Build
{
    const string Id = "com.fynkel.unityfield";
    const string Res = "Assets/Resources";

    public static void IOS()
    {
        Setup("field");
        PlayerSettings.iOS.appleDeveloperTeamID = "N2VZ98KJRE";
        PlayerSettings.iOS.appleEnableAutomaticSigning = true;
        Player(BuildTarget.iOS, BuildTargetGroup.iOS, "Build/ios");
    }

    public static void CrowdIOS()
    {
        Setup("crowd");
        PlayerSettings.iOS.appleDeveloperTeamID = "N2VZ98KJRE";
        PlayerSettings.iOS.appleEnableAutomaticSigning = true;
        Player(BuildTarget.iOS, BuildTargetGroup.iOS, "Build/crowd-ios", crowd: true);
    }

    public static void CrowdAndroid()
    {
        Setup("crowd");
        AndroidSettings();
        Player(BuildTarget.Android, BuildTargetGroup.Android, "Build/android/crowd.apk", crowd: true);
    }

    public static void Android()
    {
        Setup("field");
        AndroidSettings();
        Player(BuildTarget.Android, BuildTargetGroup.Android, "Build/android/field.apk");
    }

    static void AndroidSettings()
    {
        // The Hub's Android module came without its NDK and command-line
        // tools here: they're installed into its SDK with sdkmanager
        // (NDK r27c, as Unity 6 wants).
        var player = Path.Combine(EditorApplication.applicationPath, "../PlaybackEngines/AndroidPlayer");
        var ndk = Path.GetFullPath(Path.Combine(player, "SDK/ndk/27.2.12479018"));
        if (Directory.Exists(ndk)) UnityEditor.Android.AndroidExternalToolsSettings.ndkRootPath = ndk;
        PlayerSettings.Android.targetArchitectures = AndroidArchitecture.ARM64;
        PlayerSettings.SetScriptingBackend(UnityEditor.Build.NamedBuildTarget.Android, ScriptingImplementation.IL2CPP);
        PlayerSettings.SetGraphicsAPIs(BuildTarget.Android, new[] { GraphicsDeviceType.Vulkan });
    }

    public static void Mac()
    {
        Setup("field");
        Player(BuildTarget.StandaloneOSX, BuildTargetGroup.Standalone, "Build/mac/Field.app");
    }

    // Fynkel's levels (packages/fynkel/lib/src/world.dart, Quality).
    class Level
    {
        public readonly string Name;
        public readonly bool Hdr, Soft, Ao, Depth;
        public readonly int Msaa, ShadowMap, Cascades;
        public readonly float Scale, Distance;

        public Level(string name, bool hdr, int msaa, float scale, int shadowMap, int cascades, bool soft, float distance,
            bool ao, bool depth)
        {
            Name = name; Hdr = hdr; Msaa = msaa; Scale = scale; ShadowMap = shadowMap; Cascades = cascades;
            Soft = soft; Distance = distance; Ao = ao; Depth = depth;
        }
    }

    static readonly Level[] Levels =
    {
        new("low", false, 4, 0.75f, 1024, 1, false, 40, false, false),
        new("low, 2x AA", false, 2, 0.75f, 1024, 1, false, 40, false, false),
        new("low, no AA", false, 1, 0.75f, 1024, 1, false, 40, false, false),
        new("medium", true, 4, 1, 2048, 2, true, 60, false, true),
        new("high", true, 4, 1, 2048, 3, true, 100, true, true),
    };

    static void Setup(string which)
    {
        Directory.CreateDirectory(Res);
        File.WriteAllText($"{Res}/scene.txt", which);
        AssetDatabase.ImportAsset($"{Res}/scene.txt");
        // The field as it was measured in stage 14 (skinned on the CPU, by
        // jobs); the crowd on the GPU, as Fynkel skins.
        Skinning(which == "crowd" ? 2 : 0);
        PlayerSettings.colorSpace = ColorSpace.Linear;
        PlayerSettings.enableFrameTimingStats = true;

        var pipelines = Levels.Select(Pipeline).ToArray();
        GraphicsSettings.defaultRenderPipeline = pipelines.Last();
        Qualities(pipelines);
        Post();
        Materials();
        Animal("sheep", "Walk");
        Animal("dog", "Run");
        if (which == "crowd")
        {
            Lit("floor", Hex(0x3A3F4A), 0.5f);
            Person("knight");
            Person("rogue");
        }
        AssetDatabase.SaveAssets();

        // The scene's fog, so its shaders' fog is kept in the build (the
        // scene is made in code, as Fynkel's is).
        var scene = EditorSceneManager.NewScene(NewSceneSetup.EmptyScene);
        RenderSettings.fog = true;
        RenderSettings.fogMode = FogMode.Linear;
        EditorSceneManager.SaveScene(scene, "Assets/Field.unity");
    }

    static UniversalRenderPipelineAsset Pipeline(Level level)
    {
        var name = level.Name.Replace(",", "").Replace(" ", "-");
        var path = $"{Res}/urp-{name}.asset";
        AssetDatabase.DeleteAsset(path);
        AssetDatabase.DeleteAsset($"{Res}/urp-{name}-renderer.asset");
        var renderer = ScriptableObject.CreateInstance<UniversalRendererData>();
        AssetDatabase.CreateAsset(renderer, $"{Res}/urp-{name}-renderer.asset");
        var r = new SerializedObject(renderer);
        // Forward+, as Fynkel's clustered lamps.
        r.FindProperty("m_RenderingMode").intValue = 2;
        r.ApplyModifiedPropertiesWithoutUndo();
        if (level.Ao)
        {
            var type = typeof(UniversalRendererData).Assembly.GetType("UnityEngine.Rendering.Universal.ScreenSpaceAmbientOcclusion");
            var ao = (ScriptableRendererFeature)ScriptableObject.CreateInstance(type);
            ao.name = "SSAO";
            AssetDatabase.AddObjectToAsset(ao, renderer);
            var s = new SerializedObject(ao);
            // Fynkel's: intensity 1, 0.5 m, its passes at half size.
            s.FindProperty("m_Settings.Intensity").floatValue = 1;
            s.FindProperty("m_Settings.Radius").floatValue = 0.5f;
            s.FindProperty("m_Settings.Downsample").boolValue = true;
            s.ApplyModifiedPropertiesWithoutUndo();
            renderer.rendererFeatures.Add(ao);
            r.Update();
            var map = r.FindProperty("m_RendererFeatureMap");
            map.arraySize = 1;
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(ao, out _, out long id);
            map.GetArrayElementAtIndex(0).longValue = id;
            r.ApplyModifiedPropertiesWithoutUndo();
        }
        EditorUtility.SetDirty(renderer);

        var asset = UniversalRenderPipelineAsset.Create(renderer);
        AssetDatabase.CreateAsset(asset, path);
        var p = new SerializedObject(asset);
        void Set(string field, Action<SerializedProperty> set)
        {
            var property = p.FindProperty(field) ?? throw new Exception($"URP asset has no {field}");
            set(property);
        }
        Set("m_SupportsHDR", x => x.boolValue = level.Hdr);
        Set("m_MSAA", x => x.intValue = level.Msaa);
        Set("m_RenderScale", x => x.floatValue = level.Scale);
        Set("m_MainLightShadowsSupported", x => x.boolValue = true);
        Set("m_MainLightShadowmapResolution", x => x.intValue = level.ShadowMap);
        Set("m_ShadowCascadeCount", x => x.intValue = level.Cascades);
        Set("m_SoftShadowsSupported", x => x.boolValue = level.Soft);
        Set("m_ShadowDistance", x => x.floatValue = level.Distance);
        Set("m_AdditionalLightShadowsSupported", x => x.boolValue = false);
        Set("m_RequireDepthTexture", x => x.boolValue = level.Depth);
        Set("m_RequireOpaqueTexture", x => x.boolValue = false);
        p.ApplyModifiedPropertiesWithoutUndo();
        EditorUtility.SetDirty(asset);
        return asset;
    }

    static void Qualities(UniversalRenderPipelineAsset[] pipelines)
    {
        var qs = new SerializedObject(AssetDatabase.LoadAllAssetsAtPath("ProjectSettings/QualitySettings.asset")[0]);
        var list = qs.FindProperty("m_QualitySettings");
        list.arraySize = Levels.Length;
        for (var i = 0; i < Levels.Length; i++)
        {
            var q = list.GetArrayElementAtIndex(i);
            q.FindPropertyRelative("name").stringValue = Levels[i].Name;
            q.FindPropertyRelative("customRenderPipeline").objectReferenceValue = pipelines[i];
            q.FindPropertyRelative("vSyncCount").intValue = 0;
            q.FindPropertyRelative("antiAliasing").intValue = 0;
        }
        qs.FindProperty("m_CurrentQuality").intValue = 0;
        var defaults = qs.FindProperty("m_PerPlatformDefaultQuality");
        for (var i = 0; i < defaults.arraySize; i++)
        {
            defaults.GetArrayElementAtIndex(i).FindPropertyRelative("second").intValue = 0;
        }
        qs.ApplyModifiedPropertiesWithoutUndo();
    }

    // Bloom (Fynkel's Bloom(): strength 1, threshold 1) and the tone mapping
    // Fynkel's HDR ends with; the camera draws them from medium up.
    static void Post()
    {
        var path = $"{Res}/post.asset";
        AssetDatabase.DeleteAsset(path);
        var profile = ScriptableObject.CreateInstance<VolumeProfile>();
        AssetDatabase.CreateAsset(profile, path);
        var bloom = profile.Add<Bloom>(true);
        bloom.intensity.Override(1);
        bloom.threshold.Override(1);
        var tone = profile.Add<Tonemapping>(true);
        tone.mode.Override(TonemappingMode.Neutral);
        foreach (var c in profile.components) AssetDatabase.AddObjectToAsset(c, profile);
        EditorUtility.SetDirty(profile);
    }

    static Material Lit(string name, Color color, float roughness, Color? emission = null)
    {
        var m = new Material(Shader.Find("Universal Render Pipeline/Lit")) { name = name };
        m.SetColor("_BaseColor", color);
        m.SetFloat("_Smoothness", 1 - roughness);
        m.SetFloat("_Metallic", 0);
        if (emission is { } e)
        {
            m.EnableKeyword("_EMISSION");
            m.SetColor("_EmissionColor", e);
            m.globalIlluminationFlags = MaterialGlobalIlluminationFlags.None;
        }
        // As URP's inspector sets a material up: its keywords and blending.
        BaseShaderGUI.SetMaterialKeywords(m, LitGUI.SetMaterialKeywords);
        Save(m, name);
        return m;
    }

    static void Save(Material m, string name)
    {
        var path = $"{Res}/{name}.mat";
        AssetDatabase.DeleteAsset(path);
        AssetDatabase.CreateAsset(m, path);
    }

    static Color Hex(int rgb) => new Color32((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb, 255);

    static void Materials()
    {
        Lit("ground", Hex(0x6E8B4A), 1);
        Lit("wood", Hex(0x8B6B4A), 0.8f);
        Lit("pole", Hex(0x333333), 0.5f);
        // Fynkel's emissiveIntensity 2, in linear light as Unity's HDR colours are.
        Lit("glass", Hex(0xFFC880), 0.5f, Hex(0xFFC880).linear * 2);
        var texture = AssetDatabase.LoadAssetAtPath<Texture2D>("Assets/Field/smoke_04.png");
        foreach (var soft in new[] { true, false })
        {
            var m = new Material(Shader.Find("Universal Render Pipeline/Particles/Lit"));
            m.SetTexture("_BaseMap", texture);
            m.SetColor("_BaseColor", Color.white);
            m.SetFloat("_Surface", 1);
            m.SetFloat("_Blend", 0);
            // Fynkel's dust: roughness 1, plain alpha blending (URP's default
            // keeps a highlight over the clear parts too).
            m.SetFloat("_Smoothness", 0);
            m.SetFloat("_BlendModePreserveSpecular", 0);
            m.SetFloat("_SrcBlend", (float)BlendMode.SrcAlpha);
            m.SetFloat("_DstBlend", (float)BlendMode.OneMinusSrcAlpha);
            m.SetFloat("_SrcBlendAlpha", (float)BlendMode.One);
            m.SetFloat("_DstBlendAlpha", (float)BlendMode.OneMinusSrcAlpha);
            m.SetFloat("_ZWrite", 0);
            m.SetOverrideTag("RenderType", "Transparent");
            m.EnableKeyword("_SURFACE_TYPE_TRANSPARENT");
            m.renderQueue = (int)RenderQueue.Transparent;
            if (soft)
            {
                // Fynkel's softness 0.3: faded out within 0.3 m of what's behind.
                m.SetFloat("_SoftParticlesEnabled", 1);
                m.SetFloat("_SoftParticlesNearFadeDistance", 0);
                m.SetFloat("_SoftParticlesFarFadeDistance", 0.3f);
                m.SetVector("_SoftParticleFadeParams", new Vector4(0, 1 / 0.3f, 0, 0));
                m.EnableKeyword("_SOFTPARTICLES_ON");
            }
            BaseShaderGUI.SetMaterialKeywords(m, LitGUI.SetMaterialKeywords, ParticleGUI.SetMaterialKeywords);
            Save(m, soft ? "dust soft" : "dust hard");
        }
    }

    // [name].glb's model with its [clip] playing, URP's Lit in its colours.
    static void Animal(string name, string clip)
    {
        var glb = $"Assets/Field/{name}.glb";
        var importer = AssetImporter.GetAtPath(glb);
        var s = new SerializedObject(importer);
        var method = s.FindProperty("importSettings.animationMethod");
        if (method == null) throw new Exception($"glTFast importer has no importSettings.animationMethod: {Fields(s)}");
        if (method.intValue != 2)
        {
            method.intValue = 2; // Mecanim
            s.ApplyModifiedPropertiesWithoutUndo();
            importer.SaveAndReimport();
        }
        var all = AssetDatabase.LoadAllAssetsAtPath(glb);
        var animation = all.OfType<AnimationClip>().FirstOrDefault(c => c.name == clip)
            ?? throw new Exception($"{glb} has no clip {clip}: {string.Join(", ", all.OfType<AnimationClip>().Select(c => c.name))}");
        var controllerPath = $"{Res}/{name}.controller";
        AssetDatabase.DeleteAsset(controllerPath);
        var controller = AnimatorController.CreateAnimatorControllerAtPathWithClip(controllerPath, animation);
        controller.layers[0].stateMachine.states[0].state.name = clip;

        var model = (GameObject)PrefabUtility.InstantiatePrefab(AssetDatabase.LoadAssetAtPath<GameObject>(glb));
        PrefabUtility.UnpackPrefabInstance(model, PrefabUnpackMode.Completely, InteractionMode.AutomatedAction);
        foreach (var legacy in model.GetComponentsInChildren<Animation>()) UnityEngine.Object.DestroyImmediate(legacy);
        var animator = model.GetComponent<Animator>() ?? model.AddComponent<Animator>();
        animator.runtimeAnimatorController = controller;
        animator.cullingMode = AnimatorCullingMode.AlwaysAnimate;
        var made = 0;
        foreach (var r in model.GetComponentsInChildren<Renderer>())
        {
            r.sharedMaterials = r.sharedMaterials.Select(m =>
            {
                var color = m.HasProperty("baseColorFactor") ? m.GetColor("baseColorFactor") : m.color;
                var roughness = m.HasProperty("roughnessFactor") ? m.GetFloat("roughnessFactor") : 0.5f;
                return Lit($"{name} {made++}", color, roughness);
            }).ToArray();
        }
        var prefab = $"{Res}/{name}.prefab";
        AssetDatabase.DeleteAsset(prefab);
        PrefabUtility.SaveAsPrefabAsset(model, prefab);
        UnityEngine.Object.DestroyImmediate(model);
    }

    // ProjectSettings' mesh deformation: 0 on the CPU, 1 on the GPU, 2 on
    // the GPU batched (Unity 6's default for a new project).
    static void Skinning(int how)
    {
        var settings = new SerializedObject(AssetDatabase.LoadAllAssetsAtPath("ProjectSettings/ProjectSettings.asset")[0]);
        var skinning = settings.FindProperty("gpuSkinning") ?? throw new Exception("ProjectSettings has no gpuSkinning");
        skinning.intValue = how;
        settings.ApplyModifiedPropertiesWithoutUndo();
    }

    // KayKit's [name].glb from the lab, every clip a state of its
    // controller (named as the clip), URP's Lit with its texture.
    static void Person(string name)
    {
        Directory.CreateDirectory("Assets/Crowd");
        var glb = $"Assets/Crowd/{name}.glb";
        if (!File.Exists(glb))
        {
            File.Copy($"../../examples/lab/assets/kaykit/{name}.glb", glb);
            AssetDatabase.ImportAsset(glb);
        }
        var importer = AssetImporter.GetAtPath(glb);
        var s = new SerializedObject(importer);
        var method = s.FindProperty("importSettings.animationMethod");
        if (method.intValue != 2)
        {
            method.intValue = 2; // Mecanim
            s.ApplyModifiedPropertiesWithoutUndo();
            importer.SaveAndReimport();
        }
        var all = AssetDatabase.LoadAllAssetsAtPath(glb);
        var controllerPath = $"{Res}/{name}.controller";
        AssetDatabase.DeleteAsset(controllerPath);
        var controller = AnimatorController.CreateAnimatorControllerAtPath(controllerPath);
        foreach (var clip in all.OfType<AnimationClip>().OrderBy(c => c.name))
        {
            controller.AddMotion(clip);
        }

        var model = (GameObject)PrefabUtility.InstantiatePrefab(AssetDatabase.LoadAssetAtPath<GameObject>(glb));
        PrefabUtility.UnpackPrefabInstance(model, PrefabUnpackMode.Completely, InteractionMode.AutomatedAction);
        foreach (var legacy in model.GetComponentsInChildren<Animation>()) UnityEngine.Object.DestroyImmediate(legacy);
        var animator = model.GetComponent<Animator>() ?? model.AddComponent<Animator>();
        animator.runtimeAnimatorController = controller;
        animator.cullingMode = AnimatorCullingMode.AlwaysAnimate;
        var made = 0;
        foreach (var r in model.GetComponentsInChildren<Renderer>(true))
        {
            r.sharedMaterials = r.sharedMaterials.Select(m =>
            {
                var color = m.HasProperty("baseColorFactor") ? m.GetColor("baseColorFactor") : Color.white;
                var lit = Lit($"{name} {made++}", color, 0.5f);
                if (m.HasProperty("baseColorTexture")) lit.SetTexture("_BaseMap", m.GetTexture("baseColorTexture"));
                EditorUtility.SetDirty(lit);
                return lit;
            }).ToArray();
        }
        var prefab = $"{Res}/{name}.prefab";
        AssetDatabase.DeleteAsset(prefab);
        PrefabUtility.SaveAsPrefabAsset(model, prefab);
        UnityEngine.Object.DestroyImmediate(model);
    }

    static string Fields(SerializedObject s)
    {
        var names = new System.Collections.Generic.List<string>();
        var it = s.GetIterator();
        while (it.Next(true)) names.Add(it.propertyPath);
        return string.Join(", ", names);
    }

    static void Player(BuildTarget target, BuildTargetGroup group, string path, bool crowd = false)
    {
        PlayerSettings.productName = crowd ? "Unity Crowd" : "Unity Field";
        PlayerSettings.companyName = "Fynkel";
        PlayerSettings.SetApplicationIdentifier(UnityEditor.Build.NamedBuildTarget.FromBuildTargetGroup(group), crowd ? "com.fynkel.unitycrowd" : Id);
        var report = BuildPipeline.BuildPlayer(new[] { "Assets/Field.unity" }, path, target, BuildOptions.None);
        Debug.Log($"FIELD BUILD {report.summary.result}: {path}");
        if (report.summary.result != UnityEditor.Build.Reporting.BuildResult.Succeeded) EditorApplication.Exit(1);
    }
}
