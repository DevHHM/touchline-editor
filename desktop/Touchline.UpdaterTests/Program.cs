using System.IO.Compression;
using System.Net;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Touchline.Wpf;

if (args is ["--verify-package", var packageDirectory])
{
 string directory = Path.GetFullPath(packageDirectory);
 var manifest = UpdatePackage.Verify(directory);
 Console.WriteLine(JsonSerializer.Serialize(new { success = true, mode = "verify-package", version = manifest.Version, files = manifest.Files.Count, directory, inputModified = false, executedPackage = false, installed = false, gameWrites = false }));
 return;
}
if (args is ["--verify-archive", var archiveFile, var archiveHash, var archiveVersion])
{
 string archive = Path.GetFullPath(archiveFile);
 UpdateVersion.Parse(archiveVersion);
 DirectoryInfo? workspace = new DirectoryInfo(Path.GetDirectoryName(archive)!);
 while (workspace != null && !File.Exists(Path.Combine(workspace.FullName, "build-wpf.ps1"))) workspace = workspace.Parent;
 if (workspace == null) throw new InvalidOperationException("Archive verification requires an archive under the repository so extraction stays inside output/.");
 string output = Path.Combine(workspace.FullName, "output", "updater-package-verification", Guid.NewGuid().ToString("N"));
 UpdatePackage.Extract(new(archive, archiveHash, archiveVersion), output);
 var manifest = UpdatePackage.ReadManifest(output);
 Console.WriteLine(JsonSerializer.Serialize(new { success = true, mode = "verify-archive", version = manifest.Version, files = manifest.Files.Count, archive, sha256 = archiveHash.ToLowerInvariant(), extractedTo = output, inputModified = false, executedPackage = false, installed = false, gameWrites = false }));
 return;
}
if (args is ["--public-feed"])
{
 using var download = new UpdateDownload();
 var candidate = await download.CheckAsync(UpdateVersion.Parse("0.12.1"), true, CancellationToken.None);
 Console.WriteLine(JsonSerializer.Serialize(new { success = true, repository = UpdateFeed.Repository, apiVersion = "2026-03-10", candidate = candidate?.Version.ToString(), authentication = false, downloadedAssets = false, installed = false }));
 return;
}

var root = Path.Combine(Path.GetTempPath(), "Touchline-updater-tests-한글 공백-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);
int checks = 0;
void Check(bool success, string name) { if (!success) throw new Exception(name); checks++; Console.WriteLine("PASS " + name); }
void Reject(Action work, string name) { try { work(); } catch (Exception e) when (e is IOException or InvalidDataException or UnauthorizedAccessException) { Check(true, name); return; } throw new Exception("Expected rejection: " + name); }
async Task RejectAsync(Func<Task> work, string name) { try { await work(); } catch (Exception e) when (e is IOException or InvalidDataException or HttpRequestException) { Check(true, name); return; } throw new Exception("Expected rejection: " + name); }
string Package(string name, string version)
{
 string directory = Path.Combine(root, name); Directory.CreateDirectory(directory);
 foreach (string file in UpdatePackage.RequiredFiles) { string path = UpdatePackage.Inside(directory, file); Directory.CreateDirectory(Path.GetDirectoryName(path)!); File.WriteAllText(path, "fixture " + version + " " + file); }
 File.WriteAllText(Path.Combine(directory, "package-report.json"), JsonSerializer.Serialize(new { version, layout = "app-engine-v1", target = "win-x64" }));
 Manifest(directory, version);
 return directory;
}
void Manifest(string directory, string version)
{
 var files = Directory.EnumerateFiles(directory, "*", SearchOption.AllDirectories).Where(file => Path.GetFileName(file) != "package-manifest.json").Select(file => new { path = Path.GetRelativePath(directory, file).Replace('\\', '/'), bytes = new FileInfo(file).Length, sha256 = UpdatePackage.HashFile(file) }).ToArray();
 File.WriteAllText(Path.Combine(directory, "package-manifest.json"), JsonSerializer.Serialize(new { version, layout = "app-engine-v1", algorithm = "SHA256", files }));
}
string Feed(string version, bool preview = false, string? url = null, long size = 123, string? digest = null, bool checksum = true)
{
 string tag = "v" + version, name = UpdateFeed.ArchiveName(UpdateVersion.Parse(version));
 var assets = new List<object> { new { name, browser_download_url = url ?? $"https://github.com/{UpdateFeed.Repository}/releases/download/{tag}/{name}", size, digest } };
 if (checksum) assets.Add(new { name = name + ".sha256", browser_download_url = $"https://github.com/{UpdateFeed.Repository}/releases/download/{tag}/{name}.sha256", size = 120, digest = (string?)null });
 return JsonSerializer.Serialize(new[] { new { tag_name = tag, draft = false, prerelease = preview, assets } });
}
DownloadedUpdate Zip(string directory, string name, Action<ZipArchive>? edit = null)
{
 string zip = Path.Combine(root, name + ".zip"); ZipFile.CreateFromDirectory(directory, zip, CompressionLevel.Fastest, true);
 if (edit != null) { using var archive = ZipFile.Open(zip, ZipArchiveMode.Update); edit(archive); }
 return new(zip, UpdatePackage.HashFile(zip), "0.13.0");
}
Dictionary<string, string> Snapshot(string directory) => UpdatePackage.SafeFiles(directory).ToDictionary(f => Path.GetRelativePath(directory, f), f => UpdatePackage.HashFile(f));
bool Same(Dictionary<string, string> before, string directory) { var after = Snapshot(directory); return before.Count == after.Count && before.All(pair => after.TryGetValue(pair.Key, out var hash) && hash == pair.Value); }

try
{
 var current = UpdateVersion.Parse("0.12.1");
 Check(UpdateVersion.Parse("v0.13.0").CompareTo(current) > 0 && UpdateVersion.Parse("0.10.0").CompareTo(UpdateVersion.Parse("0.9.0")) > 0, "numeric version comparison");
 foreach (string bad in new[] { "0.13", "0.13.0-beta", "v01.2.3", "0.1.2.3", "2147483648.0.0", "../0.13.0" }) Reject(() => UpdateVersion.Parse(bad), "reject version " + bad);
 Check(UpdateFeed.Select(Feed("0.12.0"), current, false) == null && UpdateFeed.Select(Feed("0.12.1"), current, false) == null, "no downgrade or same-version install");
 Check(UpdateFeed.Select(Feed("0.13.0", true), current, false) == null && UpdateFeed.Select(Feed("0.13.0", true), current, true) != null, "preview channel opt-in");
 Check(UpdateFeed.Select(Feed("0.13.0", checksum: false), current, false) == null, "incomplete release not offered");
 foreach (string bad in new[] { "https://github.com/other/repo/releases/download/v0.13.0/Touchline-Editor-0.13.0-win-x64.zip", "http://github.com/DevHHM/touchline-editor/releases/download/v0.13.0/Touchline-Editor-0.13.0-win-x64.zip", "https://github.com.evil.test/DevHHM/touchline-editor/releases/download/v0.13.0/Touchline-Editor-0.13.0-win-x64.zip" }) Reject(() => UpdateFeed.Select(Feed("0.13.0", url: bad), current, false), "reject untrusted release URL");
 Reject(() => UpdateFeed.ParseChecksum(new string('a', 64) + "  other.zip", "expected.zip"), "checksum bound to archive filename");
 Reject(() => UpdateFeed.ParseChecksum(new string('a', 64) + "  expected.zip\n" + new string('b', 64) + "  expected.zip", "expected.zip"), "reject ambiguous checksum");
 string package = Package("new package", "0.13.0"); var zip = Zip(package, "good");
 string extracted = UpdatePackage.Extract(zip, Path.Combine(root, "extracted"));
 Check(UpdatePackage.Verify(extracted).Version == "0.13.0", "ZIP prefix extraction and full manifest verification");
 string cancellable = Package("cancellable", "0.13.0");
 using (var payload = new FileStream(Path.Combine(cancellable, "app", "cancel-fixture.bin"), FileMode.CreateNew)) payload.SetLength(64L * 1024 * 1024);
 Manifest(cancellable, "0.13.0"); var cancelZip = Zip(cancellable, "cancellable");
 using (var cancelExtraction = new CancellationTokenSource())
 {
  string cancelOutput = Path.Combine(root, "cancel-extraction");
  string cancelFile = Path.Combine(cancelOutput, "app", "cancel-fixture.bin");
  var extraction = Task.Run(() => UpdatePackage.Extract(cancelZip, cancelOutput, cancelExtraction.Token));
  while (!extraction.IsCompleted && (!File.Exists(cancelFile) || new FileInfo(cancelFile).Length == 0)) await Task.Delay(1);
  Check(!extraction.IsCompleted, "cancellation fixture reaches an active ZIP entry");
  cancelExtraction.Cancel();
  try { await extraction; throw new Exception("Extraction cancellation ignored"); }
  catch (OperationCanceledException) { Check(true, "cancel extraction during a file copy"); }
  Check(!Directory.Exists(cancelOutput), "cancelled extraction removes partial output");
 }
 foreach (string bad in new[] { "../escape.txt", "C:/escape.txt", "/absolute.txt", "new package/app/../escape.txt", "new package/app/file:stream", "new package/app/CON.txt", "new package/app/space ." })
 {
  var malicious = Zip(package, "traversal-" + Guid.NewGuid().ToString("N"), archive => { using var writer = new StreamWriter(archive.CreateEntry(bad).Open()); writer.Write("bad"); });
  Reject(() => UpdatePackage.Extract(malicious, Path.Combine(root, "reject-" + Guid.NewGuid().ToString("N"))), "reject ZIP traversal/Windows alias " + bad);
 }
 var link = Zip(package, "symlink", archive => { var entry = archive.CreateEntry("new package/app/link"); entry.ExternalAttributes = unchecked((int)0xA1FF0000); using var writer = new StreamWriter(entry.Open()); writer.Write("outside"); });
 Reject(() => UpdatePackage.Extract(link, Path.Combine(root, "reject-link")), "reject ZIP symlinks");
 var duplicate = Zip(package, "duplicate", archive => { using var writer = new StreamWriter(archive.CreateEntry("new package/APP/Touchline Editor.exe").Open()); writer.Write("duplicate"); });
 Reject(() => UpdatePackage.Extract(duplicate, Path.Combine(root, "reject-duplicate")), "reject case-insensitive duplicate ZIP paths");
 Reject(() => UpdatePackage.Extract(zip with { Sha256 = new string('0', 64) }, Path.Combine(root, "reject-hash")), "recheck ZIP hash before install");
 File.AppendAllText(Path.Combine(extracted, "app", "Touchline Editor.exe"), "tampered");
 Reject(() => UpdatePackage.Verify(extracted), "reject per-file manifest mismatch");

 byte[] archiveBytes = File.ReadAllBytes(zip.Archive); string checksumText = zip.Sha256 + "  " + UpdateFeed.ArchiveName(UpdateVersion.Parse(zip.Version));
 var release = UpdateFeed.Select(Feed("0.13.0", size: archiveBytes.Length, digest: "sha256:" + zip.Sha256), current, false)!;
 using (var download = new UpdateDownload(new FixtureHandler(request => new HttpResponseMessage(HttpStatusCode.OK) { Content = request.RequestUri!.AbsolutePath.EndsWith(".sha256") ? new StringContent(checksumText) : new ByteArrayContent(archiveBytes) })))
 {
  var received = await download.DownloadAsync(release, Path.Combine(root, "download"), null, CancellationToken.None);
  Check(UpdatePackage.HashFile(received.Archive) == zip.Sha256, "bounded stream download validates published SHA256");
 }
 using (var download = new UpdateDownload(new FixtureHandler(request => new HttpResponseMessage(HttpStatusCode.OK) { Content = request.RequestUri!.AbsolutePath.EndsWith(".sha256") ? new StringContent(checksumText) : new ByteArrayContent(Encoding.UTF8.GetBytes("corrupt")) })))
  await RejectAsync(() => download.DownloadAsync(release, Path.Combine(root, "bad-download"), null, CancellationToken.None), "reject truncated/corrupt download");
 byte[] changedBytes = archiveBytes.ToArray(); changedBytes[changedBytes.Length / 2] ^= 1;
 using (var download = new UpdateDownload(new FixtureHandler(request => new HttpResponseMessage(HttpStatusCode.OK) { Content = request.RequestUri!.AbsolutePath.EndsWith(".sha256") ? new StringContent(checksumText) : new ByteArrayContent(changedBytes) })))
  await RejectAsync(() => download.DownloadAsync(release, Path.Combine(root, "bad-hash"), null, CancellationToken.None), "reject same-length content with wrong SHA256");
 Check(!Directory.EnumerateFiles(root, "*.partial", SearchOption.AllDirectories).Any(), "failed downloads remove partial files");
 using (var cancel = new CancellationTokenSource())
 using (var download = new UpdateDownload(new FixtureHandler(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(checksumText) })))
 {
  cancel.Cancel();
  try { await download.DownloadAsync(release, Path.Combine(root, "cancel-download"), null, cancel.Token); throw new Exception("Cancellation ignored"); }
  catch (OperationCanceledException) { Check(true, "download cancellation never schedules installation"); }
 }
 using (var download = new UpdateDownload(new FixtureHandler(_ => { var response = new HttpResponseMessage(HttpStatusCode.Redirect); response.Headers.Location = new Uri("https://evil.test/update.zip"); return response; })))
  await RejectAsync(() => download.DownloadAsync(release, Path.Combine(root, "redirect"), null, CancellationToken.None), "reject foreign redirect");
 using (var download = new UpdateDownload(new FixtureHandler(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(new string('0', 64) + "  " + UpdateFeed.ArchiveName(release.Version)) })))
  await RejectAsync(() => download.DownloadAsync(release, Path.Combine(root, "digest-conflict"), null, CancellationToken.None), "reject GitHub digest/checksum disagreement");

 string installed = Package("installed", "0.12.1");
 Directory.CreateDirectory(Path.Combine(installed, "my snapshots")); File.WriteAllText(Path.Combine(installed, "my snapshots", "선수단.json"), "personal-data");
 var before = Snapshot(installed); string backup = UpdateInstall.Install(installed, package);
 Check(UpdatePackage.Verify(installed, "0.13.0", true).Version == "0.13.0" && File.ReadAllText(Path.Combine(installed, "my snapshots", "선수단.json")) == "personal-data", "install preserves untracked user data");
 Check(Same(before, backup), "backup preserves complete original package");
 foreach (string point in new[] { "prepared", "old-moved", "new-moved" })
 {
  string rollback = Package("rollback-" + point, "0.12.1"); File.WriteAllText(Path.Combine(rollback, "personal.txt"), "keep"); var snapshot = Snapshot(rollback);
  Reject(() => UpdateInstall.Install(rollback, package, stage => { if (stage == point) throw new IOException("injected failure"); }), "injected failure at " + point);
  Check(Same(snapshot, rollback), "byte-exact rollback at " + point);
 }
 string locked = Package("locked", "0.12.1"); var lockedBefore = Snapshot(locked);
 using (var handle = new FileStream(Path.Combine(locked, "Touchline Editor.exe"), FileMode.Open, FileAccess.Read, FileShare.Read))
  Reject(() => UpdateInstall.Install(locked, package), "running executable blocks replacement");
 Check(Same(lockedBefore, locked), "locked installation unchanged");
 string collisionOld = Package("personal-collision-old", "0.12.1"), collisionNew = Package("personal-collision-new", "0.13.0");
 foreach (string folder in new[] { collisionOld, collisionNew }) { Directory.CreateDirectory(Path.Combine(folder, "docs")); File.WriteAllText(Path.Combine(folder, "docs", "new-file.txt"), folder == collisionOld ? "my personal file" : "new managed file"); }
 Manifest(collisionNew, "0.13.0");
 var collisionBefore = Snapshot(collisionOld);
 Reject(() => UpdateInstall.Install(collisionOld, collisionNew), "new managed path cannot overwrite an untracked personal file");
 Check(Same(collisionBefore, collisionOld), "personal-file collision leaves original byte-exact");
 string collision = Package("collision", "0.12.1");
 string oldManifestPath = Path.Combine(collision, "package-manifest.json");
 using (var doc = JsonDocument.Parse(File.ReadAllText(oldManifestPath)))
 {
  var items = doc.RootElement.GetProperty("files").EnumerateArray().Where(e => e.GetProperty("path").GetString() != "app/Touchline Editor.dll").Select(e => e.Clone()).ToArray();
  File.WriteAllText(oldManifestPath, JsonSerializer.Serialize(new { version = "0.12.1", layout = "app-engine-v1", algorithm = "SHA256", files = items }));
 }
 // Required runtime files are mandatory even before attempting to replace the old package.
 Reject(() => UpdateInstall.Install(collision, package), "reject incomplete current manifest");
 Check(!Directory.EnumerateDirectories(root, "*.incoming-*").Any(), "transaction temporary directories cleaned");
 Console.WriteLine(JsonSerializer.Serialize(new { success = true, checks, network = "offline fixture handler", gameWrites = false }));
}
finally { Directory.Delete(root, true); }

sealed class FixtureHandler(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
{
 protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
 {
  if (request.Headers.Authorization != null) throw new Exception("Updater must not send credentials.");
  cancellationToken.ThrowIfCancellationRequested(); return Task.FromResult(respond(request));
 }
}
