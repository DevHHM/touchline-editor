using System.IO;
using System.IO.Compression;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Touchline.Wpf;

/// <summary>Local diagnostics only. Callers must never supply protocol arguments, results or entity data.</summary>
public sealed class DiagnosticsLog : IDisposable
{
 public const int DefaultPartBytes = 256 * 1024;
 private const long RetainedBytes = 5 * 1024 * 1024;
 private static readonly Regex LogName = new(@"^(client|engine)-([a-f0-9]{32})-([0-9]{4})\.jsonl$", RegexOptions.CultureInvariant);
 private static readonly Regex Address = new(@"\b0x[0-9a-f]{6,16}\b", RegexOptions.IgnoreCase);
 private static readonly Regex Credentials = new(@"(?i)(bearer\s+|[\""']?(?:token|api[_-]?key|authorization|password|secret|access_token|refresh_token)[\""']?\s*[:=]\s*[\""']?)[^\s,;\""'}]+", RegexOptions.CultureInvariant);
 private static readonly Regex Profile = new(@"(?i)[a-z]:[\\/]+Users[\\/]+[^\\/\r\n\""']+", RegexOptions.CultureInvariant);
 private static readonly Regex Url = new(@"https?://[^\s\""'<>]+", RegexOptions.IgnoreCase);
 private readonly object _gate = new();
 private readonly Dictionary<string, string> _last = new(StringComparer.Ordinal);
 private readonly int _partBytes;
 private FileStream? _stream, _marker;
 private int _part;
 private bool _disposed;
 private string? _gameBuild;
 private string? _observedPeBuild;
 private string _storefront="unknown";
 public string DirectoryPath { get; }
 public string Version { get; }
 public string Session { get; } = Guid.NewGuid().ToString("N");
 public string? LastFailure { get; private set; }
 public bool IsAvailable { get { lock(_gate) return !_disposed && _stream != null && LastFailure == null; } }
 /// <summary>Sanitized immutable JSON, dispatched off the caller thread. Handlers should only enqueue bounded work.</summary>
 public event Action<string>? RecordWritten;
 public static DiagnosticsLog? Current { get; set; }

 /// <summary>Keep only fixed metadata from an already verified live summary; never probe the game.</summary>
 public void ObserveGameContext(JsonElement summary)
 {
  lock(_gate){
   _gameBuild=null;_observedPeBuild=null;_storefront="unknown";
   if(summary.ValueKind!=JsonValueKind.Object||!summary.TryGetProperty("connected",out var connected)||connected.ValueKind!=JsonValueKind.True||
      !summary.TryGetProperty("live",out var live)||live.ValueKind!=JsonValueKind.True)return;
   if(summary.TryGetProperty("profile",out var profile)&&profile.ValueKind==JsonValueKind.String)_gameBuild=SharedDiagnosticReport.RecognizedBuild(profile.GetString()??"");
   if(summary.TryGetProperty("storefront",out var storefront)&&storefront.ValueKind==JsonValueKind.String&&SharedDiagnosticReport.Storefronts.Contains(storefront.GetString()))_storefront=storefront.GetString()!;
  }
 }
 /// <summary>Failed-connection context contains PE header integers, never module paths or addresses.</summary>
 public void ObserveConnectionContext(JsonElement observation)
 {
  lock(_gate){
   _gameBuild=null;_observedPeBuild=null;_storefront="unknown";
   if(observation.ValueKind!=JsonValueKind.Object)return;
   // Poll responses carry the current verified session under summary. Do not
   // downgrade its provenance on every successful two-second observation.
   var session=observation.TryGetProperty("summary",out var nested)&&nested.ValueKind==JsonValueKind.Object?nested:observation;
   if(session.TryGetProperty("connected",out var connected)&&connected.ValueKind==JsonValueKind.True&&
      session.TryGetProperty("live",out var live)&&live.ValueKind==JsonValueKind.True){ObserveGameContext(session);if(_gameBuild!=null)return;}
   if(!observation.TryGetProperty("diagnostic_context",out var context)||context.ValueKind!=JsonValueKind.Object)return;
   if(context.TryGetProperty("pe_timestamp",out var timestamp)&&timestamp.ValueKind==JsonValueKind.Number&&timestamp.TryGetUInt32(out uint stamp)&&
      context.TryGetProperty("image_size",out var image)&&image.ValueKind==JsonValueKind.Number&&image.TryGetUInt32(out uint size)&&size>0)_observedPeBuild=$"{stamp:X8}-{size:X8}";
   if(context.TryGetProperty("storefront",out var storefront)&&storefront.ValueKind==JsonValueKind.String&&SharedDiagnosticReport.Storefronts.Contains(storefront.GetString()))_storefront=storefront.GetString()!;
  }
 }
 public void ClearGameContext(){lock(_gate){_gameBuild=null;_observedPeBuild=null;_storefront="unknown";}}

 public DiagnosticsLog(string directory, string version, int partBytes = DefaultPartBytes)
 {
  DirectoryPath = Path.GetFullPath(directory); Version = Sanitize(version); _partBytes = Math.Max(1024, partBytes);
  Write("lifecycle", "session_started");
 }
 public static string Sanitize(string? value)
 {
  string text = value ?? "";
  string profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
  if (!string.IsNullOrWhiteSpace(profile)) text = text.Replace(profile, "%USERPROFILE%", StringComparison.OrdinalIgnoreCase);
  text = Profile.Replace(text, "%USERPROFILE%");
  text = Regex.Replace(text,@"(?i)bearer\s+[^\s,;\""'}]+","Bearer [REDACTED]");
  text = Credentials.Replace(text, "$1[REDACTED]");
  text = Url.Replace(text, "[URL]");
  text = Address.Replace(text, "[ADDRESS]");
  text = new string(text.Where(c => !char.IsControl(c) || c is '\n' or '\r' or '\t').ToArray());
  return text.Length > 4096 ? text[..4096] + " [TRUNCATED]" : text;
 }
 private static string Name(string text) => Regex.IsMatch(text, @"^[a-z][a-z0-9_]{0,63}$", RegexOptions.CultureInvariant) ? text : "other";
 public void Failure(string category, string eventName, Exception error, string? dedupKey = null)
 {
  bool includeDetails = error is not BridgeException { DiagnosticDetailsAllowed:false };
  var causes = new List<object>();
  for(Exception? item = error; item != null && causes.Count < 8; item = item.InnerException)
   causes.Add(new { type = Sanitize(item.GetType().FullName), message = includeDetails ? Sanitize(item.Message) : "요청 오류 원문은 게임 데이터 보호를 위해 제외했습니다." });
  WriteCore(category, eventName, "error", includeDetails ? Sanitize(error.Message) : "요청 오류 원문은 게임 데이터 보호를 위해 제외했습니다.", causes, dedupKey);
 }
 public void Write(string category, string eventName, string? message = null, string level = "info", string? dedupKey = null)
  => WriteCore(category, eventName, level, message == null ? null : Sanitize(message), null, dedupKey);
 private void WriteCore(string category, string eventName, string level, string? message, object? causes, string? dedupKey)
 {
  string? written = null;
  try {
   lock(_gate) {
    if(_disposed) return;
    string signature = Name(category)+"|"+Name(eventName)+"|"+message+"|"+JsonSerializer.Serialize(causes)+"|"+_gameBuild+"|"+_observedPeBuild+"|"+_storefront;
    if(dedupKey != null && _last.TryGetValue(dedupKey, out var prior) && prior == signature && IsAvailable) return;
    EnsureOpen();
    string json = JsonSerializer.Serialize(new { timestamp = DateTimeOffset.UtcNow.ToString("O"), version = Version, session = Session, source = "client", level = Name(level), category = Name(category), @event = Name(eventName), message, causes, game_build=_gameBuild, observed_pe_build=_observedPeBuild, storefront=_storefront });
    byte[] bytes = Encoding.UTF8.GetBytes(json+"\n");
    if(_stream!.Length + bytes.Length > _partBytes && _stream.Length > 0) Rotate();
    _stream!.Write(bytes); _stream.Flush(); LastFailure = null; written = json;
    if(dedupKey != null) { if(_last.Count > 64) _last.Clear(); _last[dedupKey] = signature; }
   }
  } catch(Exception error) {
   lock(_gate) { LastFailure = Sanitize(error.Message); try{_stream?.Dispose();}catch{} _stream = null; }
  }
  if(written != null && RecordWritten is { } listeners) {
   string immutable = written;
   try { ThreadPool.QueueUserWorkItem(_ => { foreach(Action<string> handler in listeners.GetInvocationList()) try { handler(immutable); } catch { } }); } catch { }
  }
 }
 private void EnsureOpen()
 {
  if(_stream != null) return;
  RejectReparsePath(DirectoryPath);
  Directory.CreateDirectory(DirectoryPath);
  if(_marker == null) _marker = new FileStream(Path.Combine(DirectoryPath,$"client-{Session}.active"),FileMode.Create,FileAccess.ReadWrite,FileShare.Read);
  _stream = OpenPart(); try{Cleanup();}catch{ /* Retention failure must not disable diagnostics. */ }
 }
 private FileStream OpenPart() => new(Path.Combine(DirectoryPath,$"client-{Session}-{_part:D4}.jsonl"),FileMode.Append,FileAccess.Write,FileShare.Read);
 private void Rotate()
 {
  _stream!.Dispose(); _stream = null;
  // Four-digit filenames are a strict export boundary; a very long session can reuse old numbers safely.
  _part = (_part + 1) % 10000;
  string next = Path.Combine(DirectoryPath,$"client-{Session}-{_part:D4}.jsonl");
  if(File.Exists(next)) { RejectReparsePath(next); File.Delete(next); }
  _stream = OpenPart();
  foreach(var old in RecognizedFiles(DirectoryPath).Where(f=>LogName.Match(f.Name).Groups[2].Value==Session).OrderByDescending(f=>f.LastWriteTimeUtc).Skip(4)) TryDelete(old.FullName);
  try{Cleanup();}catch{ /* Another instance may rotate during enumeration. */ }
 }
 private void Cleanup()
 {
  var files = RecognizedFiles(DirectoryPath).Where(f=>f.Name.StartsWith("client-",StringComparison.Ordinal)).OrderBy(f=>f.LastWriteTimeUtc).ToList();
  long bytes = files.Sum(f=>f.Length);
  foreach(var file in files) {
   var session = LogName.Match(file.Name).Groups[2].Value;
   if(session==Session || SessionActive(DirectoryPath,"client",session)) continue;
   if(bytes <= RetainedBytes && file.LastWriteTimeUtc > DateTime.UtcNow.AddDays(-7)) continue;
   long length=file.Length; if(TryDelete(file.FullName)) bytes-=length;
  }
  foreach(string marker in Directory.EnumerateFiles(DirectoryPath,"client-*.active")) {
   if(!Regex.IsMatch(Path.GetFileName(marker),@"^client-[a-f0-9]{32}\.active$")) continue;
   string session=Path.GetFileName(marker)[7..39];
   if(session!=Session && !SessionActive(DirectoryPath,"client",session)) TryDelete(marker);
  }
 }
 private static bool SessionActive(string directory,string source,string session)
 {
  string path=Path.Combine(directory,$"{source}-{session}.active");
  try { if(!File.Exists(path)) return false; RejectReparsePath(path); using var probe=new FileStream(path,FileMode.Open,FileAccess.ReadWrite,FileShare.None); return false; }
  catch { return true; }
 }
 private static bool TryDelete(string path) { try { RejectReparsePath(path); File.Delete(path); return true; } catch { return false; } }
 internal static void RejectReparsePath(string path)
 {
  string? current=Path.GetFullPath(path);
  while(current!=null) { if((File.Exists(current)||Directory.Exists(current)) && File.GetAttributes(current).HasFlag(FileAttributes.ReparsePoint)) throw new IOException("진단 경로의 심볼릭 링크 또는 재분석 지점은 사용할 수 없습니다."); current=Path.GetDirectoryName(current); }
 }
 internal static IEnumerable<FileInfo> RecognizedFiles(string directory)
 {
  RejectReparsePath(directory);
  if(!Directory.Exists(directory)) return [];
  return new DirectoryInfo(directory).EnumerateFiles().Where(f=>LogName.IsMatch(f.Name)&&!f.Attributes.HasFlag(FileAttributes.ReparsePoint)).ToArray();
 }
 public static string? NormalizeRecord(string json, string expectedSource, string expectedSession)
 {
  try {
   using var doc=JsonDocument.Parse(json); var root=doc.RootElement;
   if(root.ValueKind!=JsonValueKind.Object || root.GetProperty("source").GetString()!=expectedSource || root.GetProperty("session").GetString()!=expectedSession) return null;
   if(!root.TryGetProperty("timestamp",out var stamp)||stamp.ValueKind!=JsonValueKind.String||!DateTimeOffset.TryParse(stamp.GetString(),out _)) return null;
   string Read(string key) => root.TryGetProperty(key,out var item)&&item.ValueKind==JsonValueKind.String?Sanitize(item.GetString()):"";
   var causes=new List<object>();
   if(root.TryGetProperty("causes",out var chain)&&chain.ValueKind==JsonValueKind.Array)
    foreach(var cause in chain.EnumerateArray().Take(8)) {
     if(cause.ValueKind!=JsonValueKind.Object) continue;
     string Part(string key)=>cause.TryGetProperty(key,out var item)&&item.ValueKind==JsonValueKind.String?Sanitize(item.GetString()):"";
     causes.Add(new {type=Part("type"),message=Part("message")});
    }
   string? build=Read("game_build") is "6ADFB11C-19E69000" or "6A229E8A-1EB41000"?Read("game_build"):null;
   string storefront=SharedDiagnosticReport.Storefronts.Contains(Read("storefront"))?Read("storefront"):"unknown";
   string? observedPe=SharedDiagnosticReport.IsPeBuild(Read("observed_pe_build"))?Read("observed_pe_build"):null;
   return JsonSerializer.Serialize(new { timestamp=stamp.GetString(),version=Read("version"),session=expectedSession,source=expectedSource,level=Name(Read("level")),category=Name(Read("category")),@event=Name(Read("event")),message=Read("message"),causes,game_build=build,observed_pe_build=observedPe,storefront });
  } catch { return null; }
 }
 public string Export(string destination)
 {
  RejectReparsePath(destination);
  string full=Path.GetFullPath(destination);
  if(full.StartsWith(Path.TrimEndingDirectorySeparator(DirectoryPath)+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase)) throw new IOException("ZIP은 로그 폴더 밖에 저장해 주세요.");
  var files=RecognizedFiles(DirectoryPath).OrderByDescending(f=>f.LastWriteTimeUtc).Take(80).ToList();
  if(files.Count==0) throw new IOException("내보낼 진단 로그가 없습니다. 로그 저장 상태를 확인해 주세요.");
  int exported=0; long budget=10*1024*1024; bool created=false;
  try {
   using(var output=new FileStream(full,FileMode.CreateNew,FileAccess.Write,FileShare.None))
   using(var zip=new ZipArchive(output,ZipArchiveMode.Create)) {
    created=true;
    foreach(var file in files) {
     if(file.Length>1024*1024 || file.Length>budget) continue;
     RejectReparsePath(file.FullName);
     var match=LogName.Match(file.Name); var normalized=new List<string>();
     using(var input=new FileStream(file.FullName,FileMode.Open,FileAccess.Read,FileShare.ReadWrite)) {
      if(input.Length>1024*1024)continue;
      byte[] snapshot=new byte[(int)input.Length];int read=0;
      while(read<snapshot.Length){int n=input.Read(snapshot,read,snapshot.Length-read);if(n==0)break;read+=n;}
      foreach(string line in Encoding.UTF8.GetString(snapshot,0,read).Split('\n')) { if(line.Length>65536) continue; var safe=NormalizeRecord(line,match.Groups[1].Value,match.Groups[2].Value); if(safe!=null)normalized.Add(safe); }
     }
     if(normalized.Count==0) continue;
     budget-=file.Length; exported++;
     using var writer=new StreamWriter(zip.CreateEntry(file.Name,CompressionLevel.Optimal).Open(),new UTF8Encoding(false));
     foreach(var line in normalized) writer.WriteLine(line);
    }
    if(exported==0) throw new IOException("검증된 진단 로그가 없어 내보내지 않았습니다.");
    using var manifest=new StreamWriter(zip.CreateEntry("manifest.json").Open(),new UTF8Encoding(false));
    manifest.Write(JsonSerializer.Serialize(new {format=1,editor_version=Version,exported_at=DateTimeOffset.UtcNow.ToString("O"),runtime=System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,os=Environment.OSVersion.VersionString,architecture=System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture.ToString(),log_files=exported}));
   }
   return full;
  } catch { if(created)try{File.Delete(full);}catch{} throw; }
 }
 public void Dispose()
 {
  Write("lifecycle","session_ended");
  lock(_gate) { if(_disposed)return;_disposed=true;try{_stream?.Dispose();}catch{}try{_marker?.Dispose();}catch{} _stream=_marker=null; TryDelete(Path.Combine(DirectoryPath,$"client-{Session}.active")); }
 }
}
