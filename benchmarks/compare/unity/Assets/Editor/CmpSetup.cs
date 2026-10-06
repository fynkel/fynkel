// Engine comparison, run C4: project setup and builds, all from batch mode.
//
//   Unity -batchmode -projectPath . -executeMethod CmpSetup.Setup -quit
//   Unity -batchmode -projectPath . -executeMethod CmpSetup.BuildMac -quit
//   Unity -batchmode -projectPath . -executeMethod CmpSetup.BuildIOS -quit
//
// Setup copies ../sponza and ../character.glb into Assets/Models (git-ignored;
// Sponza's licence allows benchmarking only), lets glTFast import them at
// import time, creates the URP asset (Forward+, SSAO feature), the volume
// profile (ACES, Bloom), the player settings and Assets/Scenes/Cmp.unity —
// one GameObject with CmpBootstrap, which builds everything else in code.

using System;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.Build;
using UnityEditor.Callbacks;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;
#if UNITY_IOS
using UnityEditor.iOS.Xcode;
#endif

public static class CmpSetup
{
    const string ModelsDir = "Assets/Models";
    const string SponzaPath = "Assets/Models/Sponza/Sponza.gltf";
    const string CharacterPath = "Assets/Models/character.glb";
    const string SettingsDir = "Assets/Settings";
    const string UrpPath = "Assets/Settings/CmpURP.asset";
    const string RendererPath = "Assets/UniversalRenderer.asset";
    const string VolumePath = "Assets/Settings/CmpVolume.asset";
    const string ScenePath = "Assets/Scenes/Cmp.unity";
    const string BundleId = "dev.fynkel.compare.unity";
    const string Team = "N2VZ98KJRE";

    static string ProjectDir => Path.GetDirectoryName(Application.dataPath);
    static string CompareDir => Path.GetFullPath(Path.Combine(ProjectDir, ".."));

    [MenuItem("Cmp/Setup")]
    public static void Setup()
    {
        CopyModels();
        var urp = CreatePipeline(out var renderer);
        var profile = CreateVolume();
        ConfigurePlayer(urp);
        CreateScene(urp, renderer, profile);
        AssetDatabase.SaveAssets();
        Debug.Log("[cmp-setup] done");
    }

    // ---- Models ---------------------------------------------------------

    static void CopyModels()
    {
        var sponzaSrc = Path.Combine(CompareDir, "sponza");
        var sponzaDst = Path.Combine(ProjectDir, "Assets/Models/Sponza");
        if (!Directory.Exists(sponzaSrc)) throw new Exception("missing " + sponzaSrc + " (run tools/compare/fetch_sponza.sh)");
        Directory.CreateDirectory(sponzaDst);
        foreach (var f in Directory.GetFiles(sponzaSrc))
        {
            var dst = Path.Combine(sponzaDst, Path.GetFileName(f));
            if (!File.Exists(dst) || new FileInfo(dst).Length != new FileInfo(f).Length) File.Copy(f, dst, true);
        }
        var charSrc = Path.Combine(CompareDir, "character.glb");
        var charDst = Path.Combine(ProjectDir, CharacterPath);
        if (!File.Exists(charDst) || !File.ReadAllBytes(charDst).SequenceEqual(File.ReadAllBytes(charSrc))) File.Copy(charSrc, charDst, true);

        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        // glTFast resolves the texture files as dependencies; re-import once
        // the textures are known to the AssetDatabase.
        AssetDatabase.ImportAsset(SponzaPath, ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
        AssetDatabase.ImportAsset(CharacterPath, ImportAssetOptions.ForceUpdate | ImportAssetOptions.ForceSynchronousImport);
    }

    // ---- URP ------------------------------------------------------------

    static UniversalRenderPipelineAsset CreatePipeline(out UniversalRendererData renderer)
    {
        Directory.CreateDirectory(Path.Combine(ProjectDir, SettingsDir));
        var urp = AssetDatabase.LoadAssetAtPath<UniversalRenderPipelineAsset>(UrpPath);
        if (urp == null)
        {
            urp = UniversalRenderPipelineAsset.Create();
            AssetDatabase.CreateAsset(urp, UrpPath);
            urp.LoadBuiltinRendererData(); // writes Assets/UniversalRenderer.asset
            AssetDatabase.SaveAssets();
        }
        renderer = AssetDatabase.LoadAssetAtPath<UniversalRendererData>(RendererPath);
        renderer.renderingMode = RenderingMode.ForwardPlus;
        renderer.depthPrimingMode = DepthPrimingMode.Disabled;

        if (!renderer.rendererFeatures.OfType<ScreenSpaceAmbientOcclusion>().Any())
        {
            var ssao = ScriptableObject.CreateInstance<ScreenSpaceAmbientOcclusion>();
            ssao.name = "SSAO";
            AssetDatabase.AddObjectToAsset(ssao, renderer);
            AssetDatabase.TryGetGUIDAndLocalFileIdentifier(ssao, out _, out long localId);
            var so = new SerializedObject(renderer);
            var features = so.FindProperty("m_RendererFeatures");
            var map = so.FindProperty("m_RendererFeatureMap");
            features.arraySize++;
            features.GetArrayElementAtIndex(features.arraySize - 1).objectReferenceValue = ssao;
            map.arraySize++;
            map.GetArrayElementAtIndex(map.arraySize - 1).longValue = localId;
            so.ApplyModifiedPropertiesWithoutUndo();
        }
        foreach (var ssao in renderer.rendererFeatures.OfType<ScreenSpaceAmbientOcclusion>())
        {
            var so = new SerializedObject(ssao);
            // AO darkens the ambient only (as Fynkel's), radius as Fynkel's
            // default volume (0.6 m), depth+normals source, full resolution. URP's
            // estimator scales with the radius (ao *= RADIUS), so intensity 1 at
            // 0.6 m saturates to black; 0.2 gives a picture close to URP's own
            // default strength (0.035 m, 3).
            so.FindProperty("m_Settings.Source").enumValueIndex = 1; // DepthNormals
            so.FindProperty("m_Settings.Downsample").boolValue = false;
            so.FindProperty("m_Settings.AfterOpaque").boolValue = false;
            so.FindProperty("m_Settings.Intensity").floatValue = 0.2f;
            so.FindProperty("m_Settings.DirectLightingStrength").floatValue = 0f;
            so.FindProperty("m_Settings.Radius").floatValue = 0.6f;
            so.FindProperty("m_Settings.Falloff").floatValue = 100f;
            so.ApplyModifiedPropertiesWithoutUndo();
            ssao.SetActive(true);
        }
        EditorUtility.SetDirty(renderer);

        urp.supportsHDR = true;
        urp.msaaSampleCount = 1;
        urp.renderScale = 1f;
        urp.supportsCameraDepthTexture = false;
        urp.supportsCameraOpaqueTexture = false;
        urp.mainLightShadowmapResolution = 2048;
        urp.shadowCascadeCount = 4;
        urp.shadowDistance = 60f;
        var uso = new SerializedObject(urp);
        uso.FindProperty("m_SoftShadowsSupported").boolValue = true;
        uso.FindProperty("m_MainLightShadowsSupported").boolValue = true;
        uso.FindProperty("m_AdditionalLightShadowsSupported").boolValue = false;
        uso.FindProperty("m_AdditionalLightsRenderingMode").intValue = (int)LightRenderingMode.PerPixel;
        uso.FindProperty("m_UpscalingFilter").intValue = (int)UpscalingFilterSelection.Auto;
        uso.ApplyModifiedPropertiesWithoutUndo();
        EditorUtility.SetDirty(urp);

        GraphicsSettings.defaultRenderPipeline = urp;
        int current = QualitySettings.GetQualityLevel();
        for (int i = 0; i < QualitySettings.names.Length; i++)
        {
            QualitySettings.SetQualityLevel(i, false);
            QualitySettings.renderPipeline = urp;
            QualitySettings.vSyncCount = 0;
        }
        QualitySettings.SetQualityLevel(current, false);
        AssetDatabase.SaveAssets();
        return urp;
    }

    static VolumeProfile CreateVolume()
    {
        var profile = AssetDatabase.LoadAssetAtPath<VolumeProfile>(VolumePath);
        if (profile == null)
        {
            profile = ScriptableObject.CreateInstance<VolumeProfile>();
            AssetDatabase.CreateAsset(profile, VolumePath);
        }
        if (!profile.TryGet<Tonemapping>(out var tm))
        {
            tm = profile.Add<Tonemapping>(true);
            AssetDatabase.AddObjectToAsset(tm, profile);
        }
        tm.mode.Override(TonemappingMode.ACES);
        if (!profile.TryGet<Bloom>(out var bloom))
        {
            bloom = profile.Add<Bloom>(true);
            AssetDatabase.AddObjectToAsset(bloom, profile);
        }
        // Threshold and scatter are URP's defaults, which Fynkel's volume
        // defaults copy (0.9, 0.7); intensity 1.
        bloom.threshold.Override(0.9f);
        bloom.scatter.Override(0.7f);
        bloom.intensity.Override(1.0f);
        bloom.highQualityFiltering.Override(false);
        EditorUtility.SetDirty(profile);
        AssetDatabase.SaveAssets();
        return profile;
    }

    // ---- Player ---------------------------------------------------------

    static void ConfigurePlayer(UniversalRenderPipelineAsset urp)
    {
        PlayerSettings.companyName = "fynkel";
        PlayerSettings.productName = "CmpUnity";
        PlayerSettings.colorSpace = ColorSpace.Linear;
        PlayerSettings.enableFrameTimingStats = true;
        PlayerSettings.SetStackTraceLogType(LogType.Log, StackTraceLogType.None);
        PlayerSettings.SplashScreen.show = false;
        PlayerSettings.runInBackground = true;
        PlayerSettings.defaultInterfaceOrientation = UIOrientation.LandscapeLeft;

        var ios = NamedBuildTarget.iOS;
        PlayerSettings.SetApplicationIdentifier(ios, BundleId);
        PlayerSettings.SetApplicationIdentifier(NamedBuildTarget.Standalone, BundleId);
        PlayerSettings.SetScriptingBackend(ios, ScriptingImplementation.IL2CPP);
        PlayerSettings.SetGraphicsAPIs(BuildTarget.iOS, new[] { GraphicsDeviceType.Metal });
        PlayerSettings.SetUseDefaultGraphicsAPIs(BuildTarget.StandaloneOSX, false);
        PlayerSettings.SetGraphicsAPIs(BuildTarget.StandaloneOSX, new[] { GraphicsDeviceType.Metal });
        PlayerSettings.iOS.appleDeveloperTeamID = Team;
        PlayerSettings.iOS.appleEnableAutomaticSigning = true;
        PlayerSettings.iOS.targetDevice = iOSTargetDevice.iPhoneAndiPad;
        PlayerSettings.iOS.sdkVersion = iOSSdkVersion.DeviceSDK;
        PlayerSettings.iOS.requiresFullScreen = true;
        PlayerSettings.iOS.hideHomeButton = true;
        PlayerSettings.iOS.targetOSVersionString = "15.0";
        PlayerSettings.SetManagedStrippingLevel(ios, ManagedStrippingLevel.Low);

        // macOS sanity build: Mono (the IL2CPP Mac module is not installed).
        PlayerSettings.SetScriptingBackend(NamedBuildTarget.Standalone, ScriptingImplementation.Mono2x);
        PlayerSettings.fullScreenMode = FullScreenMode.Windowed;
        PlayerSettings.defaultScreenWidth = 1600;
        PlayerSettings.defaultScreenHeight = 900;
        PlayerSettings.resizableWindow = true;
        PlayerSettings.usePlayerLog = true;
        PlayerSettings.macRetinaSupport = false;

        EditorUserBuildSettings.development = false;
        EditorUserBuildSettings.iOSXcodeBuildConfig = XcodeBuildConfig.Release;
    }

    // ---- Scene ----------------------------------------------------------

    static void CreateScene(UniversalRenderPipelineAsset urp, UniversalRendererData renderer, VolumeProfile profile)
    {
        var sponza = AssetDatabase.LoadAssetAtPath<GameObject>(SponzaPath);
        var character = AssetDatabase.LoadAssetAtPath<GameObject>(CharacterPath);
        var walk = AssetDatabase.LoadAllAssetsAtPath(CharacterPath).OfType<AnimationClip>().FirstOrDefault(c => c.name == "walk");
        if (sponza == null) throw new Exception("Sponza did not import: " + SponzaPath);
        if (character == null) throw new Exception("character did not import: " + CharacterPath);
        if (walk == null) throw new Exception("no 'walk' clip in " + CharacterPath + ": " + string.Join(",", AssetDatabase.LoadAllAssetsAtPath(CharacterPath).OfType<AnimationClip>().Select(c => c.name)));

        Directory.CreateDirectory(Path.Combine(ProjectDir, "Assets/Scenes"));
        var scene = EditorSceneManager.NewScene(NewSceneSetup.EmptyScene, NewSceneMode.Single);
        RenderSettings.skybox = null;
        RenderSettings.ambientMode = AmbientMode.Flat;
        RenderSettings.defaultReflectionMode = DefaultReflectionMode.Custom;
        RenderSettings.reflectionIntensity = 0f;

        var go = new GameObject("Cmp");
        var b = go.AddComponent<CmpBootstrap>();
        b.sponzaPrefab = sponza;
        b.characterPrefab = character;
        b.walkClip = walk;
        b.urpAsset = urp;
        b.rendererData = renderer;
        b.volumeProfile = profile;
        EditorSceneManager.SaveScene(scene, ScenePath);
        EditorBuildSettings.scenes = new[] { new EditorBuildSettingsScene(ScenePath, true) };
    }

    // ---- Builds ---------------------------------------------------------

    static string Arg(string name, string fallback)
    {
        var args = Environment.GetCommandLineArgs();
        int i = Array.IndexOf(args, name);
        return i >= 0 && i + 1 < args.Length ? args[i + 1] : fallback;
    }

    public static void BuildMac()
    {
        Setup();
        var outPath = Arg("-cmpOut", Path.Combine(ProjectDir, "Build/mac/CmpUnity.app"));
        Build(BuildTarget.StandaloneOSX, BuildTargetGroup.Standalone, outPath);
    }

    public static void BuildIOS()
    {
        Setup();
        var outPath = Arg("-cmpOut", Path.Combine(ProjectDir, "Build/ios"));
        Build(BuildTarget.iOS, BuildTargetGroup.iOS, outPath);
    }

    static void Build(BuildTarget target, BuildTargetGroup group, string outPath)
    {
        if (EditorUserBuildSettings.activeBuildTarget != target)
            EditorUserBuildSettings.SwitchActiveBuildTarget(group, target);
        var opts = new BuildPlayerOptions
        {
            scenes = new[] { ScenePath },
            locationPathName = outPath,
            target = target,
            targetGroup = group,
            options = BuildOptions.None,
        };
        var report = BuildPipeline.BuildPlayer(opts);
        Debug.Log($"[cmp-build] {target} {report.summary.result} -> {outPath} ({report.summary.totalErrors} errors)");
        if (report.summary.result != UnityEditor.Build.Reporting.BuildResult.Succeeded)
            EditorApplication.Exit(1);
    }

    // ---- iOS post-process -----------------------------------------------

    [PostProcessBuild(100)]
    public static void OnPostprocessBuild(BuildTarget target, string path)
    {
#if UNITY_IOS
        if (target != BuildTarget.iOS) return;
        var plistPath = Path.Combine(path, "Info.plist");
        var plist = new PlistDocument();
        plist.ReadFromFile(plistPath);
        var root = plist.root;
        // ProMotion: allow 120 Hz CADisplayLink on iPhone.
        root.SetBoolean("CADisableMinimumFrameDurationOnPhone", true);
        // Shots in Documents are reachable from Files / devicectl.
        root.SetBoolean("UIFileSharingEnabled", true);
        root.SetBoolean("LSSupportsOpeningDocumentsInPlace", true);
        plist.WriteToFile(plistPath);

        var projPath = PBXProject.GetPBXProjectPath(path);
        var proj = new PBXProject();
        proj.ReadFromFile(projPath);
        foreach (var guid in new[] { proj.GetUnityMainTargetGuid(), proj.GetUnityFrameworkTargetGuid() })
        {
            proj.SetBuildProperty(guid, "DEVELOPMENT_TEAM", Team);
            proj.SetBuildProperty(guid, "CODE_SIGN_STYLE", "Automatic");
        }
        proj.SetBuildProperty(proj.GetUnityMainTargetGuid(), "PRODUCT_BUNDLE_IDENTIFIER", BundleId);
        proj.WriteToFile(projPath);

        // IL2CPP "Master" configuration: the generated Xcode script maps
        // every non-Debug Xcode configuration to il2cpp --configuration=Release
        // (PlayerSettings' C++ configuration does not apply to iOS). il2cpp's
        // own enum is Debug/Release/ReleasePlus/ReleaseSize; ReleasePlus is
        // what the editor's "Master" maps to on other platforms.
        var text = File.ReadAllText(projPath);
        var patched = text.Replace("IL2CPP_CONFIG=\\\"Release\\\"", "IL2CPP_CONFIG=\\\"ReleasePlus\\\"");
        if (patched == text) Debug.LogWarning("[cmp-build] IL2CPP_CONFIG=Release not found; il2cpp stays on Release");
        File.WriteAllText(projPath, patched);
#endif
    }
}
