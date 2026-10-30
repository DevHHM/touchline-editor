using System.IO;
using System.Text.Json;

namespace Touchline.Wpf;

public sealed record UpdateInstallRequest(DownloadedUpdate Update, string TargetDirectory, int ParentProcessId, long ParentStartUtcTicks);
public sealed record UpdateInstallResult(bool Success, string Version, string? BackupDirectory, string Message);

/// <summary>Package-only transaction; preserves untracked files and retains the entire previous installation.</summary>
public static class UpdateInstall
{
 public static string? PackageRoot(string appDirectory)
 {
  var app = new DirectoryInfo(Path.GetFullPath(appDirectory));
  return app.Name.Equals("app", StringComparison.OrdinalIgnoreCase) && app.Parent != null && File.Exists(Path.Combine(app.Parent.FullName, "package-manifest.json")) ? app.Parent.FullName : null;
 }
 public static void ValidateTarget(string directory)
 {
  string target = Path.GetFullPath(directory).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
  var info = new DirectoryInfo(target);
  if (info.Parent == null || !Directory.Exists(target) || !File.Exists(Path.Combine(target, "Touchline Editor.exe")) || !Directory.Exists(Path.Combine(target, "app")) || !Directory.Exists(Path.Combine(target, "engine"))) throw new InvalidDataException("Touchline 배포 폴더를 찾지 못했습니다.");
  // Reject junctions in every ancestor, not just at the destination leaf.
  for (DirectoryInfo? current = info; current != null; current = current.Parent)
   if ((current.Attributes & FileAttributes.ReparsePoint) != 0) throw new InvalidDataException("링크된 경로에는 자동 설치할 수 없습니다.");
  UpdatePackage.ReadManifest(target);
 }
 public static string Install(string target, string prepared, Action<string>? checkpoint = null)
 {
  target = Path.GetFullPath(target).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
  ValidateTarget(target);
  var old = UpdatePackage.ReadManifest(target); var next = UpdatePackage.Verify(prepared);
  if (UpdateVersion.Parse(next.Version).CompareTo(UpdateVersion.Parse(old.Version)) <= 0) throw new InvalidDataException("같거나 오래된 버전은 설치하지 않습니다.");
  string parent = Path.GetDirectoryName(target)!;
  string name = Path.GetFileName(target);
  string incoming = Path.Combine(parent, "." + name + ".incoming-" + Guid.NewGuid().ToString("N"));
  string backup = Path.Combine(parent, name + ".backup-" + DateTime.UtcNow.ToString("yyyyMMdd-HHmmss") + "-" + Guid.NewGuid().ToString("N")[..8]);
  string journal = Path.Combine(parent, "." + name + ".update-transaction.json");
  using var updateLock = new FileStream(Path.Combine(parent, "." + name + ".update-lock"), FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
  if (File.Exists(journal)) throw new IOException("이전 업데이트 복구 기록이 있습니다. 업데이트 설명서의 복구 방법을 확인하세요: " + journal);
  bool movedOld = false, installedNew = false, committed = false;
  try
  {
   Directory.CreateDirectory(incoming);
   foreach (string source in UpdatePackage.SafeFiles(prepared)) Copy(source, incoming, Path.GetRelativePath(prepared, source).Replace('\\', '/'));
   var managed = old.Files.Select(f => f.Path).Append("package-manifest.json").ToHashSet(StringComparer.OrdinalIgnoreCase);
   // Preserve snapshots and other user files saved inside a portable package.
   foreach (string source in UpdatePackage.SafeFiles(target))
   {
    string relative = Path.GetRelativePath(target, source).Replace('\\', '/');
    if (!managed.Contains(relative)) Copy(source, incoming, relative);
   }
   UpdatePackage.Verify(incoming, next.Version, true);
   // Opening every executable exclusively catches another editor/API using this copy.
   foreach (string file in UpdatePackage.SafeFiles(target).Where(p => p.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)))
    using (new FileStream(file, FileMode.Open, FileAccess.ReadWrite, FileShare.None)) { }
   File.WriteAllText(journal, JsonSerializer.Serialize(new { target, backup, incoming, state = "prepared" }));
   checkpoint?.Invoke("prepared");
   Directory.Move(target, backup); movedOld = true;
   checkpoint?.Invoke("old-moved");
   Directory.Move(incoming, target); installedNew = true;
   checkpoint?.Invoke("new-moved");
   File.Delete(journal);
   committed = true;
   return backup;
  }
  catch (Exception installError)
  {
   try
   {
    if (movedOld && !committed)
    {
     if (installedNew) Directory.Move(target, incoming);
     if (!Directory.Exists(target) && Directory.Exists(backup)) Directory.Move(backup, target);
     else throw new IOException("이전 배포 폴더를 복구할 위치가 비어 있지 않습니다.");
    }
    if (!committed && File.Exists(journal)) File.Delete(journal);
   }
   catch (Exception rollbackError) { throw new IOException("업데이트 복구를 마치지 못했습니다. 이전 버전은 다음 폴더에 보존되어 있습니다: " + backup + ". 복구 기록: " + journal, new AggregateException(installError, rollbackError)); }
   throw;
  }
  finally { try { if (Directory.Exists(incoming)) Directory.Delete(incoming, true); } catch (IOException) { /* Preserve staging if another process opened it. */ } catch (UnauthorizedAccessException) { } }
 }
 private static void Copy(string source, string target, string relative)
 {
  string destination = UpdatePackage.Inside(target, relative);
  Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
  // Any conflict with untracked user data aborts before the installed folder moves.
  File.Copy(source, destination, false);
 }
}
