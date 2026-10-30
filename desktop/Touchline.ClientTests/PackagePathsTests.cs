using Touchline.Wpf;

internal static class PackagePathsTests
{
    public static void Run(Action<bool, string> check)
    {
        var temp = Path.Combine(Path.GetTempPath(), "Touchline 경로 검사 " + Guid.NewGuid().ToString("N"));
        var app = Path.Combine(temp, "app");
        Directory.CreateDirectory(app);
        try
        {
            var engine = Path.Combine(temp, "engine", "Touchline Engine.exe");
            Directory.CreateDirectory(Path.GetDirectoryName(engine)!);
            File.WriteAllBytes(engine, []);
            check(PackagePaths.ResolveEngine(app) == engine, "portable_engine_sibling_with_unicode_spaces");
            var manual = Path.Combine(temp, "manual.exe");
            check(PackagePaths.ResolveEngine(app, manual) == manual, "explicit_engine_path_precedes_portable_layout");
            File.Delete(engine);
            var legacy = Path.Combine(app, "Touchline Engine.exe");
            File.WriteAllBytes(legacy, []);
            check(PackagePaths.ResolveEngine(app) == legacy, "legacy_flat_engine_layout_retained");
            File.WriteAllText(Path.Combine(temp, "package-manifest.json"), "{}");
            var rejected = false;
            try { PackagePaths.ResolveEngine(app); } catch (FileNotFoundException) { rejected = true; }
            check(rejected, "damaged_portable_release_does_not_fall_back_to_other_engine");
            File.Delete(Path.Combine(temp, "package-manifest.json"));
            File.Delete(legacy);
            var dev = Path.Combine(temp, "target", "release", "touchline-editor.exe");
            Directory.CreateDirectory(Path.GetDirectoryName(dev)!);
            File.WriteAllBytes(dev, []);
            check(PackagePaths.ResolveEngine(app) == dev, "development_target_engine_lookup_retained");
        }
        finally { Directory.Delete(temp, recursive: true); }
    }
}
