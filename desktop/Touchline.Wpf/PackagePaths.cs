using System.IO;

namespace Touchline.Wpf;

/// <summary>Resolve portable, legacy and development layouts independently of cwd.</summary>
public static class PackagePaths
{
    public static string ResolveEngine(string applicationDirectory, string? explicitPath = null)
    {
        if (!string.IsNullOrWhiteSpace(explicitPath)) return Path.GetFullPath(explicitPath);
        var app = new DirectoryInfo(Path.GetFullPath(applicationDirectory));
        var portable = Path.GetFullPath(Path.Combine(app.FullName, "..", "engine", "Touchline Engine.exe"));
        if (File.Exists(portable)) return portable;
        // A damaged release must not silently fall back to an unrelated dev build.
        if (app.Parent is { } package && File.Exists(Path.Combine(package.FullName, "package-manifest.json")))
            throw new FileNotFoundException("배포본의 engine 폴더가 없습니다. ZIP 전체를 다시 압축 해제하세요.", portable);
        var legacy = Path.Combine(app.FullName, "Touchline Engine.exe");
        if (File.Exists(legacy)) return legacy;
        for (DirectoryInfo? dir = app; dir is not null; dir = dir.Parent)
        {
            var candidate = Path.Combine(dir.FullName, "target", "release", "touchline-editor.exe");
            if (File.Exists(candidate)) return candidate;
        }
        throw new FileNotFoundException("Touchline Engine.exe를 찾을 수 없습니다. app·engine 폴더를 포함해 배포 ZIP 전체를 압축 해제하세요.", portable);
    }
}
