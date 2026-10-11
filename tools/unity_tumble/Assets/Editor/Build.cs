// Builds tools/unity_tumble for a device, from the command line:
//
//   Unity -batchmode -quit -projectPath tools/unity_tumble -executeMethod Build.Mac|IOS|Android
//
// TUMBLE_CUBES (default 1,000) is baked in as Resources/tumble.txt. The
// builds land in tools/unity_tumble/Build (git-ignored).

using System;
using System.IO;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

public static class Build
{
    const string Id = "com.fynkel.unitytumble";

    public static void Mac() => Player(BuildTarget.StandaloneOSX, BuildTargetGroup.Standalone, "Build/mac/Tumble.app");

    public static void IOS()
    {
        PlayerSettings.iOS.appleDeveloperTeamID = "N2VZ98KJRE";
        PlayerSettings.iOS.appleEnableAutomaticSigning = true;
        Player(BuildTarget.iOS, BuildTargetGroup.iOS, "Build/ios");
        // ProMotion: without this an iPhone stops at 60 fps.
        var plist = new UnityEditor.iOS.Xcode.PlistDocument();
        plist.ReadFromFile("Build/ios/Info.plist");
        plist.root.SetBoolean("CADisableMinimumFrameDurationOnPhone", true);
        plist.WriteToFile("Build/ios/Info.plist");
    }

    public static void Android()
    {
        PlayerSettings.Android.targetArchitectures = AndroidArchitecture.ARM64;
        PlayerSettings.SetScriptingBackend(UnityEditor.Build.NamedBuildTarget.Android, ScriptingImplementation.IL2CPP);
        Player(BuildTarget.Android, BuildTargetGroup.Android, "Build/android/tumble.apk");
    }

    static void Player(BuildTarget target, BuildTargetGroup group, string path)
    {
        var cubes = Environment.GetEnvironmentVariable("TUMBLE_CUBES") ?? "1000";
        Directory.CreateDirectory("Assets/Resources");
        File.WriteAllText("Assets/Resources/tumble.txt", cubes);
        const string scene = "Assets/Tumble.unity";
        if (!File.Exists(scene))
        {
            EditorSceneManager.SaveScene(EditorSceneManager.NewScene(NewSceneSetup.EmptyScene), scene);
        }
        AssetDatabase.Refresh();
        PlayerSettings.productName = "Unity Tumble";
        PlayerSettings.companyName = "Fynkel";
        PlayerSettings.SetApplicationIdentifier(UnityEditor.Build.NamedBuildTarget.FromBuildTargetGroup(group), Id);
        var report = BuildPipeline.BuildPlayer(new[] { scene }, path, target, BuildOptions.None);
        Debug.Log($"TUMBLE BUILD {report.summary.result}: {path}, {cubes} cubes");
        if (report.summary.result != UnityEditor.Build.Reporting.BuildResult.Succeeded) EditorApplication.Exit(1);
    }
}
