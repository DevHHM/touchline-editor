using System.IO;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

namespace Touchline.Wpf;

// Automatic reports deliberately contain codes, not exception text or local log contents.
public sealed record SharedDiagnosticReport(int Schema, string Product, string ReportId, string OccurredAt,
 string EditorVersion, string Source, string Category, string Event, string Code, string? Build,
 [property:JsonIgnore(Condition=JsonIgnoreCondition.WhenWritingNull)] string? FailureStage=null,
 [property:JsonIgnore(Condition=JsonIgnoreCondition.WhenWritingNull)] string? ExceptionType=null,
 [property:JsonIgnore(Condition=JsonIgnoreCondition.WhenWritingNull)] string? BuildSource=null,
 [property:JsonIgnore(Condition=JsonIgnoreCondition.WhenWritingNull)] string? Storefront=null)
{
 public static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower };
 public static readonly string[] FailureStages=["unknown","handled","unhandled_domain","unobserved_task","connect","connection_poll","connection_reconnect","connection_state","engine_start","engine_lifecycle"];
 public static readonly string[] ExceptionTypes=["none","unknown","bridge","aggregate","null_reference","invalid_operation","argument","argument_null","argument_out_of_range","io","file_not_found","directory_not_found","unauthorized_access","timeout","http_request","json","format","overflow","operation_canceled","task_canceled","not_supported","object_disposed","win32","xaml_parse"];
 public static readonly string[] BuildSources=["unknown","error_text","verified_session","observed_pe"];
 public static readonly string[] Storefronts=["unknown","steam","epic","xbox_game_pass"];
 [JsonIgnore] public string Fingerprint => Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes($"{EditorVersion}|{Source}|{Category}|{Event}|{Code}|{Build}"+(Schema==2?$"|v2|{FailureStage}|{ExceptionType}|{BuildSource}|{Storefront}":""))));
 public static string? RecognizedBuild(string text)
 {
  string? candidate=null;
  // Count unknown identities too. Hex boundaries include labelled tokens;
  // lookahead also detects overlapping pairs in concatenated failure reasons.
  foreach(Match match in Regex.Matches(text,@"(?<![0-9a-f])(?=([0-9a-f]{8}[/-][0-9a-f]{8})(?![0-9a-f]))",RegexOptions.IgnoreCase|RegexOptions.CultureInvariant)){
   string value=match.Groups[1].Value.ToUpperInvariant().Replace('/','-');
   if(candidate!=null&&candidate!=value)return null;
   candidate=value;
  }
  return candidate is "6ADFB11C-19E69000" or "6A229E8A-1EB41000"?candidate:null;
 }
 public static bool IsPeBuild(string? value)=>value!=null&&Regex.IsMatch(value,@"^[0-9A-F]{8}-[0-9A-F]{8}$",RegexOptions.CultureInvariant)&&!value.EndsWith("-00000000",StringComparison.Ordinal);
 public static string SafeExceptionType(string? type)=>type switch {
  "Touchline.Wpf.BridgeException"=>"bridge","System.AggregateException"=>"aggregate","System.NullReferenceException"=>"null_reference",
  "System.InvalidOperationException"=>"invalid_operation","System.ArgumentException"=>"argument","System.ArgumentNullException"=>"argument_null","System.ArgumentOutOfRangeException"=>"argument_out_of_range",
  "System.IO.IOException"=>"io","System.IO.FileNotFoundException"=>"file_not_found","System.IO.DirectoryNotFoundException"=>"directory_not_found","System.UnauthorizedAccessException"=>"unauthorized_access",
  "System.TimeoutException"=>"timeout","System.Net.Http.HttpRequestException"=>"http_request","System.Text.Json.JsonException"=>"json","System.FormatException"=>"format","System.OverflowException"=>"overflow",
  "System.OperationCanceledException"=>"operation_canceled","System.Threading.Tasks.TaskCanceledException"=>"task_canceled","System.NotSupportedException"=>"not_supported","System.ObjectDisposedException"=>"object_disposed",
  "System.ComponentModel.Win32Exception"=>"win32","System.Windows.Markup.XamlParseException"=>"xaml_parse",_=>"unknown"};
 public bool ValidMetadata()
 {
  string[] codes=["access_denied","unsupported_build","registry_validation","session_changed","engine_missing","engine_transport","timeout","other_error","application_error"];
  bool validEvent=Category switch {"connection"=>Event=="connection_failed","lifecycle"=>Event=="engine_failed","application"=>Event=="application_error"&&Code=="application_error","command"=>Event is "connect" or "connection_poll" or "connection_reconnect",_=>false};
  if(!validEvent||!codes.Contains(Code)||(Build!=null&&Build is not("6ADFB11C-19E69000" or "6A229E8A-1EB41000")&&!(Schema==2&&BuildSource=="observed_pe"&&IsPeBuild(Build))))return false;
  if(Schema==1)return FailureStage==null&&ExceptionType==null&&BuildSource==null&&Storefront==null;
  return Schema==2&&FailureStages.Contains(FailureStage)&&ExceptionTypes.Contains(ExceptionType)&&BuildSources.Contains(BuildSource)&&Storefronts.Contains(Storefront)&&
   (Build==null?BuildSource=="unknown":BuildSource is "error_text" or "verified_session" or "observed_pe");
 }
 public string PayloadPreview()=>JsonSerializer.Serialize(this,new JsonSerializerOptions(Json){WriteIndented=true});
 public static SharedDiagnosticReport? FromLocalRecord(string json,int schema=1)
 {
  if(json.Length>65536||schema is not(1 or 2))return null;
  try {
   using var doc=JsonDocument.Parse(json); var r=doc.RootElement;
   string Read(string key)=>r.TryGetProperty(key,out var v)&&v.ValueKind==JsonValueKind.String?v.GetString()!:"";
   string category=Read("category"), eventName=Read("event"), source=Read("source"), version=Read("version");
   if(Read("level")!="error" || source is not ("client" or "engine") ||
      category is not ("connection" or "lifecycle" or "application" or "command") ||
      !Regex.IsMatch(version,@"^\d{1,3}\.\d{1,3}\.\d{1,3}(?:\.\d{1,5})?$"))return null;
   // Command names supplied by API callers are not a telemetry field.
   if(category=="command" && eventName is not ("connect" or "connection_poll" or "connection_reconnect"))return null;
   string message=Read("message");
   string code=Classify(message);
   string safeEvent=category=="command"?eventName:category=="connection"?"connection_failed":category=="lifecycle"?"engine_failed":"application_error";
   if(category=="application")code="application_error";
   string? build=RecognizedBuild(message),buildSource=build==null?"unknown":"error_text";
   if(schema==2&&IsPeBuild(Read("observed_pe_build"))){build=Read("observed_pe_build");buildSource="observed_pe";}
   if(schema==2&&Read("game_build") is "6ADFB11C-19E69000" or "6A229E8A-1EB41000"){build=Read("game_build");buildSource="verified_session";}
   string stage=category switch {"application" when eventName is "handled" or "unhandled_domain" or "unobserved_task"=>eventName,"command"=>eventName,"connection"=>"connection_state","lifecycle" when eventName=="engine_start_failed"=>"engine_start","lifecycle"=>"engine_lifecycle",_=>"unknown"};
   string exception="none";
   if(r.TryGetProperty("causes",out var causes)&&causes.ValueKind==JsonValueKind.Array)
    foreach(var cause in causes.EnumerateArray().Take(8)){
     if(cause.ValueKind!=JsonValueKind.Object||!cause.TryGetProperty("type",out var type)||type.ValueKind!=JsonValueKind.String)continue;
     string safe=SafeExceptionType(type.GetString());if(exception=="none"||safe!="unknown")exception=safe;
    }
   string storefront=Storefronts.Contains(Read("storefront"))?Read("storefront"):"unknown";
   var stamp=DateTimeOffset.TryParse(Read("timestamp"),out var parsed)?parsed:DateTimeOffset.UtcNow;
   return new(schema,"touchline-editor",Guid.NewGuid().ToString("N"),stamp.ToUniversalTime().ToString("O"),version,source,category,safeEvent,code,build,
    schema==2?stage:null,schema==2?exception:null,schema==2?buildSource:null,schema==2?storefront:null);
  }catch{return null;}
 }
 public static bool IsAccessDenied(string text)=>Regex.IsMatch(text,@"\bos error 5\b",RegexOptions.IgnoreCase|RegexOptions.CultureInvariant)||
  text.Contains("Access is denied",StringComparison.OrdinalIgnoreCase)||text.Contains("액세스가 거부");
 public static string Classify(string text)
 {
  if(IsAccessDenied(text))return "access_denied";
  if(text.Contains("지원하지 않는 FM")||text.Contains("지원되지 않는 FM")||text.Contains("코드 검증 불일치")||text.Contains("UnsupportedBuild"))return "unsupported_build";
  if(text.Contains("인물 레지스트리")||text.Contains("RTTI")||text.Contains("객체 유형"))return "registry_validation";
  if(text.Contains("세션")||text.Contains("세이브가 변경"))return "session_changed";
  // A response timeout is still a timeout, including inside BridgeClient's
  // communication wrapper. Known transport failures can also say "읽을 수 없습니다";
  // that wording does not mean that the engine executable is missing.
  if(text.Contains("timeout",StringComparison.OrdinalIgnoreCase)||text.Contains("시간 초과"))return "timeout";
  if(text.Contains("Rust 엔진 연결이 종료되었습니다.")||text.Contains("Rust 엔진이 종료되었습니다.")||text.Contains("Rust 엔진과 통신하지 못했습니다.")||
     text.Contains("pipe",StringComparison.OrdinalIgnoreCase)||text.Contains("파이프")||text.Contains("응답"))return "engine_transport";
  if((text.Contains("engine",StringComparison.OrdinalIgnoreCase)||text.Contains("엔진"))&& (text.Contains("없")||text.Contains("찾을 수 없")||text.Contains("not found",StringComparison.OrdinalIgnoreCase)))return "engine_missing";
  return "other_error";
 }
}

public sealed record DiagnosticCollector(string IngestUrl, string PrivacyUrl, string Destination,int ReportSchema=1)
{
 public static DiagnosticCollector? Read(string path)
 {
  try {
   DiagnosticsLog.RejectReparsePath(path);
   if(!File.Exists(path)||new FileInfo(path).Length>4096)return null;
   using var document=JsonDocument.Parse(File.ReadAllText(path));var r=document.RootElement;
   var endpoint=r.GetProperty("ingest_url").GetString()??"";
   var privacy=r.GetProperty("privacy_url").GetString()??"";
   var destination=r.GetProperty("destination").GetString()??"";
   int schema=r.TryGetProperty("report_schema",out var format)?format.GetInt32():1;
   if(!AllowedUrl(endpoint)||!AllowedUrl(privacy)||destination is not ("private" or "public")||schema is not(1 or 2))return null;
   // Distributor changes this only after the collector supports v2. Existing deployments remain v1.
   return new(endpoint,privacy,destination,schema);
  }catch{return null;}
 }
 public static bool AllowedUrl(string value)=>Uri.TryCreate(value,UriKind.Absolute,out var uri)&&uri.Scheme=="https"&&
  uri.Port==443&&uri.UserInfo.Length==0&&uri.Fragment.Length==0&&uri.Query.Length==0&&
  uri.HostNameType==UriHostNameType.Dns&&!uri.IsLoopback&&uri.Host.Contains('.')&&
  !uri.Host.EndsWith(".local",StringComparison.OrdinalIgnoreCase)&&!uri.Host.Equals("api.github.com",StringComparison.OrdinalIgnoreCase);
}

/// <summary>Consent-only, bounded outbox. A collector URL is configured by the distributor, never guessed.</summary>
public sealed class DiagnosticSharingClient : IDisposable
{
 private readonly string _root;private readonly DiagnosticCollector? _collector;private readonly HttpClient _http;
 private readonly SemaphoreSlim _send=new(1,1);private readonly object _gate=new();private readonly Timer _timer;
 private CancellationTokenSource _cancel=new();private bool _disposed;private bool _sessionRevoked;private string _status="공유 꺼짐 · 오류마다 먼저 동의를 받습니다.";
 private DateTimeOffset _retryAfter=DateTimeOffset.MinValue;
 private sealed record Consent(bool Enabled,string? Scope=null);
 private string ConsentScope => _collector==null ? "" : Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(
  "2-per-report\n"+new Uri(_collector.IngestUrl).AbsoluteUri+"\n"+new Uri(_collector.PrivacyUrl).AbsoluteUri+"\n"+_collector.Destination+(_collector.ReportSchema==2?"\nreport-schema-2":""))));
 private sealed record RateState(string Day,int Count,List<string> Fingerprints);
 public event Action? StatusChanged;
 public bool Configured=>_collector!=null;
 public bool Enabled {get {try {
  if(_disposed||Volatile.Read(ref _sessionRevoked)||!Configured)return false;
  var file=Path.Combine(_root,"consent.json");DiagnosticsLog.RejectReparsePath(file);
  if(!File.Exists(file)||new FileInfo(file).Length>=4096)return false;
  var consent=JsonSerializer.Deserialize<Consent>(File.ReadAllText(file));
  // Legacy boolean-only consent and changed collector/privacy/publication scope require a fresh opt-in.
  return consent?.Enabled==true&&consent.Scope==ConsentScope&&!Volatile.Read(ref _sessionRevoked);
 }catch{return false;}}}
 public string Status {get{lock(_gate)return _status;}}
 public string Destination=>_collector?.Destination??"unconfigured";
 public int ReportSchema=>_collector?.ReportSchema??1;
 public DiagnosticSharingClient(string root,DiagnosticCollector? collector,HttpMessageHandler? handler=null,bool timer=true)
 {
  _root=Path.GetFullPath(root);_collector=collector;
  _http=new HttpClient(handler??new HttpClientHandler{AllowAutoRedirect=false}){Timeout=TimeSpan.FromSeconds(8)};
  _http.DefaultRequestHeaders.UserAgent.ParseAdd("Touchline-Diagnostics/1");
  _timer=new Timer(state=>{_ = SendPendingAsync();},null,timer?TimeSpan.FromSeconds(15):Timeout.InfiniteTimeSpan,timer?TimeSpan.FromMinutes(1):Timeout.InfiniteTimeSpan);
  SetStatus(!Configured?"중앙 수집 서버가 아직 설정되지 않았습니다.":Enabled?"승인한 대기 보고만 전송합니다. 새 오류는 별도 동의를 받습니다.":"공유 꺼짐 · 오류마다 먼저 동의를 받습니다.");
 }
 public void SetEnabled(bool enabled)
 {
  if(enabled&&!Configured)throw new InvalidOperationException("먼저 배포자의 중앙 수집 서버 설정이 필요합니다.");
  if(enabled){
   lock(_gate){
    bool alreadyEnabled=Enabled;
    if(!alreadyEnabled)Volatile.Write(ref _sessionRevoked,true);
    EnsureRoot();
    // Reports collected under an old destination or an earlier revoked consent are never replayed.
    if(!alreadyEnabled)foreach(var file in Pending())File.Delete(file);
    AtomicWrite(Path.Combine(_root,"consent.json"),JsonSerializer.Serialize(new Consent(true,ConsentScope)));
    Volatile.Write(ref _sessionRevoked,false);
   }
   SetStatus("승인한 보고서만 전송합니다. 새 오류는 별도 동의를 받습니다.");return;
  }
  Exception? persistenceError=null,cleanupError=null;
  lock(_gate){
   // Revoke before any filesystem operation; denied/locked preferences must never keep sending enabled.
   Volatile.Write(ref _sessionRevoked,true);
   try{_cancel.Cancel();}catch(Exception error){cleanupError=error;}
   _cancel.Dispose();_cancel=new();
   try{EnsureRoot();AtomicWrite(Path.Combine(_root,"consent.json"),JsonSerializer.Serialize(new Consent(false,ConsentScope)));}
   catch(Exception error){persistenceError=error;}
   try{foreach(var file in Pending())try{File.Delete(file);}catch(Exception error){cleanupError??=error;}}
   catch(Exception error){cleanupError??=error;}
  }
  string status=persistenceError!=null?"전송 꺼짐 · 해제 설정을 저장하지 못했습니다. 이번 실행에서는 전송하지 않으며, 다음 실행 때 설정을 다시 확인해 주세요.":
   cleanupError!=null?"전송 꺼짐 · 일부 대기 요약을 삭제하지 못했지만 전송은 차단했습니다.":"전송 꺼짐 · 대기 중인 오류 요약을 삭제했습니다.";
  SetStatus(status);
  if(persistenceError!=null||cleanupError!=null)throw new IOException(status,persistenceError??cleanupError);
 }
 // Only the per-error consent gate calls this in the application. A queue entry is one approved report.
 public void EnqueueApproved(string sanitizedRecord)
 {var report=SharedDiagnosticReport.FromLocalRecord(sanitizedRecord,ReportSchema);if(report!=null)EnqueueApproved(report);}
 internal void EnqueueApproved(SharedDiagnosticReport report)
 {
  try {
   if(_disposed||!Enabled)return;
   if(report.Schema!=ReportSchema||!report.ValidMetadata())return;
   lock(_gate){if(_disposed||!Enabled)return;EnsureRoot();
    var rate=ReadRate();if(rate.Count>=20||rate.Fingerprints.Contains(report.Fingerprint))return;
    var pending=Pending().ToArray();
    if(pending.Select(ReadReport).Any(r=>r?.Fingerprint==report.Fingerprint)||pending.Length>=32)return;
    AtomicWrite(Path.Combine(_root,$"pending-{report.ReportId}.json"),JsonSerializer.Serialize(report,SharedDiagnosticReport.Json));}
  }catch{SetStatus("오류 요약을 저장하지 못했습니다. 로컬 진단 로그를 확인하세요.");}
 }
 public async Task SendPendingAsync()
 {
  if(_disposed||!Enabled||DateTimeOffset.UtcNow<_retryAfter||!await _send.WaitAsync(0))return;
  try {
   string[] files;lock(_gate){EnsureRoot();files=Pending().ToArray();}
   foreach(var file in files){
    if(_disposed||!Enabled)break;
    var report=ReadReport(file);if(report==null||report.Schema!=ReportSchema){lock(_gate)File.Delete(file);continue;}
    RateState rate;lock(_gate){rate=ReadRate();if(rate.Count>=20){SetStatus("오늘 전송 한도에 도달했습니다. 하루 최대 20건입니다.");break;}if(rate.Fingerprints.Contains(report.Fingerprint)){File.Delete(file);continue;}}
    var payload=JsonSerializer.Serialize(report,SharedDiagnosticReport.Json);
    using var body=new StringContent(payload,Encoding.UTF8,"application/json");
    Task<HttpResponseMessage> request;
    lock(_gate){
     if(_disposed||!Enabled)break;
     // Start under the consent lock and retain this token. Revocation cancels this exact request.
     request=_http.PostAsync(_collector!.IngestUrl,body,_cancel.Token);
    }
    using var response=await request;
    if(_disposed||!Enabled)break; // A late successful response must not overwrite the revocation status.
    if(response.StatusCode!=System.Net.HttpStatusCode.Accepted){
     // No follow-up redirects, credential prompts or immediate retry loops.
     _retryAfter=DateTimeOffset.UtcNow.AddMinutes(5);SetStatus($"중앙 전송 대기 · HTTP {(int)response.StatusCode}. 5분 뒤 다시 시도합니다.");break;
    }
    lock(_gate){if(_disposed||!Enabled)break;rate=ReadRate();rate.Fingerprints.Add(report.Fingerprint);AtomicWrite(Path.Combine(_root,"rate.json"),JsonSerializer.Serialize(rate with{Count=rate.Count+1}));File.Delete(file);SetStatus("오류 요약을 중앙 수집 서버에 전송했습니다.");}
   }
  }catch(OperationCanceledException){if(Enabled){_retryAfter=DateTimeOffset.UtcNow.AddMinutes(5);SetStatus("중앙 전송 시간 초과 · 5분 뒤 다시 시도합니다.");}}
   catch{if(Enabled){_retryAfter=DateTimeOffset.UtcNow.AddMinutes(5);SetStatus("중앙 수집 서버에 연결하지 못했습니다. 5분 뒤 다시 시도합니다.");}}
   finally{_send.Release();}
 }
 private RateState ReadRate(){try{var file=Path.Combine(_root,"rate.json");DiagnosticsLog.RejectReparsePath(file);if(File.Exists(file)&&new FileInfo(file).Length<8192){var value=JsonSerializer.Deserialize<RateState>(File.ReadAllText(file));if(value?.Day==DateTime.UtcNow.ToString("yyyy-MM-dd")&&value.Count>=0&&value.Fingerprints.Count<=32)return value;}}catch{}return new(DateTime.UtcNow.ToString("yyyy-MM-dd"),0,[]);}
 private void EnsureRoot(){DiagnosticsLog.RejectReparsePath(_root);Directory.CreateDirectory(_root);}
 private IEnumerable<string> Pending(){if(!Directory.Exists(_root))return [];DiagnosticsLog.RejectReparsePath(_root);return Directory.EnumerateFiles(_root,"pending-*.json").Where(p=>Regex.IsMatch(Path.GetFileName(p),@"^pending-[a-f0-9]{32}\.json$")&&!File.GetAttributes(p).HasFlag(FileAttributes.ReparsePoint)).OrderBy(p=>p,StringComparer.Ordinal).ToArray();}
 private static SharedDiagnosticReport? ReadReport(string file){try{DiagnosticsLog.RejectReparsePath(file);if(new FileInfo(file).Length>2048)return null;var r=JsonSerializer.Deserialize<SharedDiagnosticReport>(File.ReadAllText(file),SharedDiagnosticReport.Json);if(r==null||r.Schema is not(1 or 2)||r.Product!="touchline-editor"||!Regex.IsMatch(r.ReportId,@"^[a-f0-9]{32}$")||r.Source is not("client" or "engine")||r.Category is not("connection" or "lifecycle" or "application" or "command")||!Regex.IsMatch(r.EditorVersion,@"^\d{1,3}\.\d{1,3}\.\d{1,3}(?:\.\d{1,5})?$")||!DateTimeOffset.TryParse(r.OccurredAt,out var date)||date<DateTimeOffset.UtcNow.AddDays(-7)||date>DateTimeOffset.UtcNow.AddMinutes(5)||Path.GetFileName(file)!=$"pending-{r.ReportId}.json")return null;if(!r.ValidMetadata())return null;return r with{OccurredAt=date.ToUniversalTime().ToString("O")};}catch{return null;}}
 private static void AtomicWrite(string path,string text){DiagnosticsLog.RejectReparsePath(path);var temp=path+"."+Guid.NewGuid().ToString("N")+".tmp";try{File.WriteAllText(temp,text,Encoding.UTF8);File.Move(temp,path,true);}finally{if(File.Exists(temp))File.Delete(temp);}}
 private void SetStatus(string value){lock(_gate)_status=value;try{StatusChanged?.Invoke();}catch{}}
 public void Dispose(){lock(_gate){if(_disposed)return;_disposed=true;_timer.Dispose();_cancel.Cancel();}_http.Dispose();}
}

/// <summary>Explicit checkbox actions must not be skipped because persisted preferences are unreadable.</summary>
public sealed class DiagnosticConsentToggle(Func<bool> readEnabled,Action<bool> setEnabled,Action<bool> synchronize,Action<Exception> reportError)
{
 private bool _synchronizing;
 public void Change(bool requested)
 {
  if(_synchronizing)return;
  _synchronizing=true;
  try {
   try{setEnabled(requested);}catch(Exception error){reportError(error);}
   finally{synchronize(readEnabled());}
  }finally{_synchronizing=false;}
 }
}

public static class DiagnosticSharing
{
 private static DiagnosticSharingClient? _client;private static DiagnosticsLog? _log;private static DiagnosticErrorConsentGate? _consent;
 public static bool Configured=>_client?.Configured==true;public static bool Enabled=>_client?.Enabled==true;
 public static string Status=>_client?.Status??"중앙 전송 초기화 전";
 public static event Action? StatusChanged;
 public static void Start(Func<SharedDiagnosticReport,DiagnosticCollector,CancellationToken,Task<bool>> ask){try{if(_client!=null)return;var collector=DiagnosticCollector.Read(Path.Combine(AppContext.BaseDirectory,"diagnostics-collector.json"));_client=new(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"TouchlineEditor","DiagnosticSharing"),collector);_client.StatusChanged+=Notify;_consent=new(_client,(report,token)=>collector==null?Task.FromResult(false):ask(report,collector,token));_log=DiagnosticsLog.Current;if(_log!=null)_log.RecordWritten+=Observe;}catch{}}
 private static void Observe(string record){if(_consent is{} consent)_=consent.ObserveAsync(record);}
 private static void Notify(){try{StatusChanged?.Invoke();}catch{}}
 public static void CancelPending(){if(_consent==null)throw new InvalidOperationException("중앙 오류 수집이 준비되지 않았습니다.");_consent.Revoke();}
 public static Task SendPendingAsync()=>_client?.SendPendingAsync()??Task.CompletedTask;
 public static void Stop(){if(_log!=null)_log.RecordWritten-=Observe;_consent?.Dispose();_client?.Dispose();_client=null;_log=null;_consent=null;}
}

/// <summary>Every new error requires an explicit decision. Only approved reports enter the retry outbox.</summary>
public sealed class DiagnosticErrorConsentGate(DiagnosticSharingClient client,Func<SharedDiagnosticReport,CancellationToken,Task<bool>>? ask) : IDisposable
{
 private readonly object _gate=new();private readonly SemaphoreSlim _prompt=new(1,1);private readonly HashSet<string> _seen=[];
 private CancellationTokenSource _cancel=new();private int _waiting;private bool _disposed;private bool _revoked;
 public async Task ObserveAsync(string record)
 {
  var report=SharedDiagnosticReport.FromLocalRecord(record,client.ReportSchema);if(report==null||!client.Configured||ask==null)return;
  CancellationToken token;
  lock(_gate){
   // One underlying failure may be logged by both bridge and connection state handlers.
   string promptKey=$"{report.EditorVersion}|{report.Code}|{report.Build}"+(report.Schema==2&&report.Category=="application"?$"|{report.FailureStage}|{report.ExceptionType}":"");
   if(_disposed||_revoked||_waiting>=3||_seen.Count>=128||!_seen.Add(promptKey))return;
   _waiting++;token=_cancel.Token;
  }
  bool acquired=false;
  try {
   await _prompt.WaitAsync(token);acquired=true;
   bool accepted=await ask(report,token);
   lock(_gate){
    // A late Yes after cancellation or shutdown cannot resurrect consent.
    if(!accepted||token.IsCancellationRequested||_disposed||_revoked)return;
    client.SetEnabled(true);client.EnqueueApproved(report);
   }
   await client.SendPendingAsync();
  }catch { /* Prompt, persistence and transport failure never imply consent or break the caller. */ }
  finally{if(acquired)_prompt.Release();lock(_gate)_waiting--;}
 }
 public void Revoke(){lock(_gate){_revoked=true;try{_cancel.Cancel();}catch{}client.SetEnabled(false);}}
 public void Dispose(){lock(_gate){if(_disposed)return;_disposed=true;try{_cancel.Cancel();}catch{}}}
}
