using System.Diagnostics;
using System.Text.Json;
using Touchline.Wpf;

if (args is ["--helper-check"]) return 0;
UpdateInstallResult result;
string version = "unknown";
try
{
 if (args.Length != 1) throw new ArgumentException("설치 요청 파일이 필요합니다.");
 string requestPath = Path.GetFullPath(args[0]);
 string cache = Path.GetFullPath(UpdatePreferences.DataDirectory) + Path.DirectorySeparatorChar;
 if (!requestPath.StartsWith(cache, StringComparison.OrdinalIgnoreCase) || new FileInfo(requestPath).Length > 65536) throw new InvalidDataException("설치 요청 경로가 올바르지 않습니다.");
 var request = JsonSerializer.Deserialize<UpdateInstallRequest>(File.ReadAllText(requestPath)) ?? throw new InvalidDataException("설치 요청이 올바르지 않습니다.");
 version = request.Update.Version;
 string job = Path.GetDirectoryName(requestPath)!;
 if (!Path.GetFullPath(request.Update.Archive).StartsWith(job + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("다운로드 파일이 설치 요청 폴더 밖에 있습니다.");
 if (request.ParentProcessId == Environment.ProcessId) throw new InvalidDataException("잘못된 부모 프로세스입니다.");
 try
 {
  using var parent = Process.GetProcessById(request.ParentProcessId);
  if (parent.StartTime.ToUniversalTime().Ticks == request.ParentStartUtcTicks)
  {
   using var wait = new CancellationTokenSource(TimeSpan.FromMinutes(15));
   await parent.WaitForExitAsync(wait.Token);
  }
 }
 catch (ArgumentException) { /* Parent already exited. */ }
 UpdateInstall.ValidateTarget(request.TargetDirectory);
 // The portable launcher waits for the WPF process; allow it to exit as well.
 var deadline = DateTime.UtcNow.AddSeconds(30);
 while (true)
 {
  try
  {
   foreach (string file in UpdatePackage.SafeFiles(request.TargetDirectory).Where(p => p.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)))
    using (new FileStream(file, FileMode.Open, FileAccess.ReadWrite, FileShare.None)) { }
   break;
  }
  catch (IOException) when (DateTime.UtcNow < deadline) { await Task.Delay(500); }
 }
 string extracted = UpdatePackage.Extract(request.Update, Path.Combine(job, "install-" + Guid.NewGuid().ToString("N")));
 string backup = UpdateInstall.Install(request.TargetDirectory, extracted);
 result = new(true, version, backup, "설치 완료. 에디터를 다시 시작하면 새 버전을 사용합니다.");
}
catch (Exception error) { result = new(false, version, null, error.Message); }
try
{
 Directory.CreateDirectory(UpdatePreferences.DataDirectory);
 string temporary = UpdatePreferences.ResultPath + ".tmp";
 File.WriteAllText(temporary, JsonSerializer.Serialize(result)); File.Move(temporary, UpdatePreferences.ResultPath, true);
}
catch { /* Cannot report to an unavailable profile; never modify game data. */ }
return result.Success ? 0 : 1;
