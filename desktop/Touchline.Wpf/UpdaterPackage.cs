using System.IO;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Touchline.Wpf;

public sealed record UpdatePackageFile(string Path, long Bytes, string Sha256);
public sealed record UpdatePackageManifest(string Version, IReadOnlyList<UpdatePackageFile> Files);

public static class UpdatePackage
{
 public const long MaximumExpandedBytes = 4L * 1024 * 1024 * 1024;
 public static string RelativePath(string path)
 {
  if (string.IsNullOrEmpty(path) || path.Contains('\\') || path.Contains(':') || path.StartsWith('/') || Path.IsPathRooted(path)) throw new InvalidDataException("안전하지 않은 패키지 경로입니다.");
  string[] parts = path.Split('/');
  foreach (string part in parts)
  {
   if (part.Length == 0 || part is "." or ".." || part.EndsWith('.') || part.EndsWith(' ') || part.Any(c => c < 32 || "<>\"|?*".Contains(c)) || Regex.IsMatch(part, @"^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])($|\.)", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)) throw new InvalidDataException("안전하지 않은 패키지 경로입니다.");
  }
  return path;
 }
 public static string Inside(string directory, string relative)
 {
  RelativePath(relative);
  string root = Path.GetFullPath(directory).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
  string result = Path.GetFullPath(Path.Combine(root, relative));
  if (!result.StartsWith(root, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("패키지 경로가 폴더 밖을 가리킵니다.");
  return result;
 }
 public static string HashFile(string file, CancellationToken cancellation = default)
 {
  cancellation.ThrowIfCancellationRequested();
  using var stream = File.OpenRead(file); using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
  var buffer = new byte[131072]; int count;
  while ((count = stream.Read(buffer)) > 0) { cancellation.ThrowIfCancellationRequested(); hash.AppendData(buffer, 0, count); }
  cancellation.ThrowIfCancellationRequested(); return Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
 }
 public static UpdatePackageManifest ReadManifest(string directory, CancellationToken cancellation = default)
 {
  cancellation.ThrowIfCancellationRequested();
  var file = new FileInfo(Path.Combine(directory, "package-manifest.json"));
  if (!file.Exists || file.Length > 8 * 1024 * 1024) throw new InvalidDataException("패키지 파일 목록이 없거나 너무 큽니다.");
  using var json = JsonDocument.Parse(File.ReadAllText(file.FullName)); var root = json.RootElement;
  if (root.GetProperty("layout").GetString() != "app-engine-v1" || root.GetProperty("algorithm").GetString() != "SHA256") throw new InvalidDataException("지원하지 않는 패키지 형식입니다.");
  string version = root.GetProperty("version").GetString()!; UpdateVersion.Parse(version);
  var paths = new HashSet<string>(StringComparer.OrdinalIgnoreCase); var files = new List<UpdatePackageFile>(); long total = 0;
  foreach (var entry in root.GetProperty("files").EnumerateArray())
  {
   cancellation.ThrowIfCancellationRequested();
   string path = RelativePath(entry.GetProperty("path").GetString()!);
   if (!paths.Add(path) || path == "package-manifest.json" || !IsPackageFile(path)) throw new InvalidDataException("패키지 파일 목록에 중복되거나 허용되지 않는 경로가 있습니다.");
   long size = entry.GetProperty("bytes").GetInt64(); string hash = entry.GetProperty("sha256").GetString()!;
   if (size < 0 || size > MaximumExpandedBytes || total > MaximumExpandedBytes - size || files.Count >= 15000 || !Regex.IsMatch(hash, "^[0-9a-fA-F]{64}$")) throw new InvalidDataException("패키지 파일 크기 또는 SHA256이 올바르지 않습니다.");
   total += size; files.Add(new(path, size, hash));
  }
  foreach (string required in RequiredFiles)
   if (!paths.Contains(required)) throw new InvalidDataException("필수 패키지 파일이 없습니다: " + required);
  return new(version, files);
 }
 public static readonly string[] RequiredFiles = ["Touchline Editor.exe", "app/Touchline Editor.exe", "app/Touchline Editor.dll", "app/Touchline Editor.runtimeconfig.json", "app/Touchline Editor.deps.json", "app/coreclr.dll", "app/hostfxr.dll", "app/hostpolicy.dll", "app/PresentationFramework.dll", "app/Wpf.Ui.dll", "app/Touchline Updater.exe", "engine/Touchline Engine.exe", "package-report.json"];
 private static bool IsPackageFile(string path) => path.StartsWith("app/", StringComparison.Ordinal) || path.StartsWith("engine/", StringComparison.Ordinal) || path.StartsWith("docs/", StringComparison.Ordinal) || path.StartsWith("api/", StringComparison.Ordinal) || path.StartsWith("licenses/", StringComparison.Ordinal) || path is "Touchline Editor.exe" or "README.txt" or "THIRD-PARTY-NOTICES.txt" or "package-report.json";
 public static IEnumerable<string> SafeFiles(string directory)
 {
  if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0) throw new InvalidDataException("링크된 폴더는 업데이트할 수 없습니다.");
  foreach (var item in new DirectoryInfo(directory).EnumerateFileSystemInfos())
  {
   if ((item.Attributes & FileAttributes.ReparsePoint) != 0) throw new InvalidDataException("패키지 안의 파일 링크를 거부했습니다.");
   if (item is DirectoryInfo) { foreach (string file in SafeFiles(item.FullName)) yield return file; }
   else yield return item.FullName;
  }
 }
 public static UpdatePackageManifest Verify(string directory, string? expectedVersion = null, bool allowUserFiles = false, CancellationToken cancellation = default)
 {
  var manifest = ReadManifest(directory, cancellation);
  if (expectedVersion != null && manifest.Version != expectedVersion) throw new InvalidDataException("패키지 버전이 릴리스 버전과 다릅니다.");
  var paths = manifest.Files.Select(f => f.Path).Append("package-manifest.json").ToHashSet(StringComparer.OrdinalIgnoreCase);
  foreach (string file in SafeFiles(directory))
  {
   cancellation.ThrowIfCancellationRequested();
   string relative = Path.GetRelativePath(directory, file).Replace('\\', '/');
   if (!allowUserFiles && !paths.Contains(relative)) throw new InvalidDataException("파일 목록에 없는 패키지 파일입니다: " + relative);
  }
  foreach (var file in manifest.Files)
  {
   cancellation.ThrowIfCancellationRequested();
   string path = Inside(directory, file.Path); var info = new FileInfo(path);
   if (!info.Exists || info.Length != file.Bytes || !HashFile(path, cancellation).Equals(file.Sha256, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("패키지 파일 검증에 실패했습니다: " + file.Path);
  }
  using var report = JsonDocument.Parse(File.ReadAllText(Path.Combine(directory, "package-report.json")));
  if (report.RootElement.GetProperty("version").GetString() != manifest.Version || report.RootElement.GetProperty("layout").GetString() != "app-engine-v1" || report.RootElement.GetProperty("target").GetString() != "win-x64") throw new InvalidDataException("Windows x64 패키지가 아닙니다.");
  return manifest;
 }
 public static string Extract(DownloadedUpdate update, string output, CancellationToken cancellation = default)
 {
  cancellation.ThrowIfCancellationRequested();
  if (!Regex.IsMatch(update.Sha256, "^[0-9a-fA-F]{64}$") || !HashFile(update.Archive, cancellation).Equals(update.Sha256, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("설치 전 ZIP SHA256 검증에 실패했습니다.");
  if (Directory.Exists(output)) throw new IOException("압축 해제 대상은 새 폴더여야 합니다.");
  using var zip = ZipFile.OpenRead(update.Archive);
  if (zip.Entries.Count == 0 || zip.Entries.Count > 20000) throw new InvalidDataException("ZIP 항목 수가 허용 범위를 벗어났습니다.");
  var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase); long total = 0;
  foreach (var entry in zip.Entries)
  {
   cancellation.ThrowIfCancellationRequested();
   string name = entry.FullName.TrimEnd('/'); RelativePath(name);
   if (!names.Add(name)) throw new InvalidDataException("ZIP에 중복 경로가 있습니다.");
   int unixKind = (entry.ExternalAttributes >> 16) & 0xF000;
   if (unixKind is not (0 or 0x8000 or 0x4000) || (entry.ExternalAttributes & 0x400) != 0) throw new InvalidDataException("ZIP의 파일 링크를 거부했습니다.");
   if (entry.Length > MaximumExpandedBytes || total > MaximumExpandedBytes - entry.Length) throw new InvalidDataException("압축 해제 크기가 허용 범위를 넘었습니다.");
   total += entry.Length;
  }
  var manifests = zip.Entries.Where(e => e.FullName == "package-manifest.json" || e.FullName.EndsWith("/package-manifest.json", StringComparison.Ordinal)).ToArray();
  if (manifests.Length != 1) throw new InvalidDataException("ZIP에는 하나의 패키지가 있어야 합니다.");
  string prefix = manifests[0].FullName[..^"package-manifest.json".Length];
  if (prefix.TrimEnd('/').Contains('/')) throw new InvalidDataException("패키지가 너무 깊은 폴더에 있습니다.");
  Directory.CreateDirectory(output);
  try
  {
   foreach (var entry in zip.Entries)
   {
    cancellation.ThrowIfCancellationRequested();
    if (entry.FullName == prefix) continue;
    if (!entry.FullName.StartsWith(prefix, StringComparison.Ordinal)) throw new InvalidDataException("ZIP에 패키지 밖의 파일이 있습니다.");
    string relative = entry.FullName[prefix.Length..].TrimEnd('/'); if (relative == "") continue;
    string path = Inside(output, relative);
    if (entry.FullName.EndsWith('/')) { Directory.CreateDirectory(path); continue; }
    Directory.CreateDirectory(Path.GetDirectoryName(path)!);
    using var input = entry.Open(); using var destination = new FileStream(path, FileMode.CreateNew, FileAccess.Write);
    var buffer = new byte[131072]; long written = 0; int count;
    while ((count = input.Read(buffer)) > 0) { cancellation.ThrowIfCancellationRequested(); written += count; if (written > entry.Length) throw new InvalidDataException("압축 파일의 크기 정보가 일치하지 않습니다."); destination.Write(buffer, 0, count); }
    if (written != entry.Length) throw new InvalidDataException("압축 파일이 완전하지 않습니다.");
   }
   Verify(output, update.Version, cancellation: cancellation);
   return output;
  }
  catch { Directory.Delete(output, true); throw; }
 }
}
