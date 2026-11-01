using System.IO.Compression;
using System.Text.Json;
using Touchline.Wpf;

internal static class DiagnosticsTests
{
 public static async Task RunAsync(Action<bool,string> check)
 {
  string root=Path.GetFullPath(Path.Combine(Path.GetTempPath(),"touchline-diagnostics-test-"+Guid.NewGuid().ToString("N")));
  Directory.CreateDirectory(root);
  try {
   string logs=Path.Combine(root,"Logs");
   using(var log=new DiagnosticsLog(logs,"test",2048)) {
    check(log.IsAvailable,"diagnostics_initializes_utf8_session_file");
    string secret="Authorization=\"Bearer private-token\" api_key=second-secret token:third-secret 0x7FF123456789 C:\\Users\\Private User\\game\\file https://private.example/?token=fourth-secret";
    log.Failure("connection","connect_failed",new IOException(secret,new UnauthorizedAccessException("접근이 거부되었습니다")));
    string records=ReadAll(logs);
    check(!records.Contains("private-token")&&!records.Contains("second-secret")&&!records.Contains("third-secret")&&!records.Contains("fourth-secret")&&!records.Contains("7FF123")&&!records.Contains("Private User")&&!records.Contains("private.example"),"diagnostics_redacts_credentials_addresses_profile_and_urls_before_disk");
    check(records.Contains("UnauthorizedAccessException")&&records.Contains("IOException"),"diagnostics_preserves_cause_types");
    int before=Events(logs,"state_changed");
    for(int i=0;i<30;i++)log.Write("connection","state_changed","loading","info","connection_state");
    log.Write("connection","state_changed","ready","info","connection_state");
    check(Events(logs,"state_changed")==before+2,"diagnostics_deduplicates_repeated_poll_state_but_keeps_transition");
    var observed=new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
    log.RecordWritten+=json=>observed.TrySetResult(json);
    log.RecordWritten+=_=>throw new Exception("listener failure");
    log.Write("connection","notification_test","token=private-token");
    check(!(await observed.Task.WaitAsync(TimeSpan.FromSeconds(3))).Contains("private-token")&&log.IsAvailable,"diagnostics_notifications_are_sanitized_and_listener_failures_isolated");
    for(int i=0;i<45;i++)log.Write("connection","rotation_test",new string('x',800));
    var parts=Directory.GetFiles(logs,$"client-{log.Session}-*.jsonl");
    check(parts.Length<=4&&parts.Sum(p=>new FileInfo(p).Length)<=4*2048,"diagnostics_long_running_session_stays_bounded");
    log.Write("connection","after_rotation","still works");
    check(ReadAll(logs).Contains("after_rotation"),"diagnostics_rotation_keeps_writing_valid_records");
    string old="client-"+new string('a',32)+"-0000.jsonl";
    File.WriteAllText(Path.Combine(logs,old),"{}\n");File.SetLastWriteTimeUtc(Path.Combine(logs,old),DateTime.UtcNow.AddDays(-10));
    foreach(string part in parts)File.SetLastWriteTimeUtc(part,DateTime.UtcNow.AddDays(-10));
    using(var second=new DiagnosticsLog(logs,"test")) {
     check(!File.Exists(Path.Combine(logs,old))&&parts.All(File.Exists),"diagnostics_retention_removes_old_closed_session_keeps_active_other_session");
     check(log.Session!=second.Session&&Directory.GetFiles(logs,"*.active").Length==2,"diagnostics_multiple_instances_keep_distinct_session_locks");
    }
    string engineSession=new string('b',32), engineName=$"engine-{engineSession}-0000.jsonl";
    File.WriteAllText(Path.Combine(logs,engineName),JsonSerializer.Serialize(new{timestamp=DateTimeOffset.UtcNow.ToString("O"),version="test",session=engineSession,source="engine",level="error",category="connection",@event="failed",message="token=private-token 0x12345678",causes=new[]{new{type="OsError",message="access denied"}},args=new{name="DO_NOT_EXPORT_PLAYER"}})+"\nmalformed line\n");
    File.WriteAllText(Path.Combine(logs,"private-save.json"),"DO_NOT_EXPORT_SAVE");
    File.WriteAllText(Path.Combine(logs,"wpf-errors.log"),"DO_NOT_EXPORT_OLDLOG");
    string sub=Path.Combine(logs,"nested");Directory.CreateDirectory(sub);File.WriteAllText(Path.Combine(sub,engineName),"DO_NOT_EXPORT_NESTED");
    string external=Path.Combine(root,"private-target.txt");File.WriteAllText(external,"DO_NOT_EXPORT_LINK");
    string link=Path.Combine(logs,$"client-{new string('c',32)}-0000.jsonl");
    File.CreateSymbolicLink(link,external);
    string zipPath=log.Export(Path.Combine(root,"diagnostics.zip"));
    using(var zip=ZipFile.OpenRead(zipPath)) {
     string body=string.Join("\n",zip.Entries.Select(entry=>{using var reader=new StreamReader(entry.Open());return reader.ReadToEnd();}));
     check(zip.Entries.Any(e=>e.Name=="manifest.json")&&zip.Entries.Any(e=>e.Name==engineName)&&zip.Entries.All(e=>e.Name=="manifest.json"||e.Name.EndsWith(".jsonl")),"diagnostics_export_includes_recognized_client_engine_logs_and_manifest_only");
     check(!body.Contains("DO_NOT_EXPORT")&&!body.Contains("private-token")&&!body.Contains("0x12345678")&&!body.Contains("malformed line"),"diagnostics_export_strips_unknown_fields_and_resanitizes_records");
     check(zip.Entries.All(e=>e.Name!=Path.GetFileName(link)),"diagnostics_export_rejects_symbolic_link_log");
    }
    bool refused=false;try{log.Export(zipPath);}catch(IOException){refused=true;}
    check(refused&&File.Exists(zipPath)&&new FileInfo(zipPath).Length>0,"diagnostics_export_failure_never_deletes_existing_destination");
    refused=false;try{log.Export(Path.Combine(logs,"inside.zip"));}catch(IOException){refused=true;}
    check(refused&&!File.Exists(Path.Combine(logs,"inside.zip")),"diagnostics_export_refuses_destination_inside_logs");
    File.Delete(link);
    string linkedDirectory=Path.Combine(root,"linked-logs");Directory.CreateSymbolicLink(linkedDirectory,logs);
    using(var redirected=new DiagnosticsLog(linkedDirectory,"test"))check(!redirected.IsAvailable,"diagnostics_rejects_symlink_log_directory");
    Directory.Delete(linkedDirectory);
    string enginePath=Environment.ProcessPath!;
    await using(var bridge=new BridgeClient(enginePath,log)) {
     try{await bridge.CallAsync("connect",new{failForDiagnostics=true,password="REQUEST_SECRET",name="PLAYER_NAME_SECRET"});}catch(BridgeException){}
     try{await bridge.CallAsync("fail",new{password="REQUEST_SECRET",name="PLAYER_NAME_SECRET"});}catch(BridgeException){}
     var response=await bridge.CallAsync("status");
     check(response.GetProperty("command").GetString()=="status","diagnostics_command_logging_preserves_bridge_retry_behavior");
    }
    records=ReadAll(logs);
    check(records.Contains("\"event\":\"connect\"")&&records.Contains("\"event\":\"fail\"")&&records.Contains("engine_started")&&!records.Contains("REQUEST_SECRET")&&!records.Contains("PLAYER_NAME_SECRET")&&!records.Contains("backend-secret"),"diagnostics_fake_bridge_failure_persists_without_arguments_results_or_secret");
   }
   check(Directory.GetFiles(logs,"*.active").Length==0,"diagnostics_shutdown_releases_active_marker");
   string contextLogs=Path.Combine(root,"ContextLogs");
   using(var log=new DiagnosticsLog(contextLogs,"0.14.3")){
    log.ObserveGameContext(JsonSerializer.SerializeToElement(new{connected=true,live=true,profile="FM27 / 6ADFB11C-19E69000",storefront="steam",player="PRIVATE_PLAYER",path="PRIVATE_PATH"}));
    log.Failure("application","handled",new NullReferenceException("PRIVATE_MESSAGE"));
    string Last()=>ReadAll(contextLogs).Split('\n').Last(line=>line.Contains("\"event\":\"handled\""));
    var report=SharedDiagnosticReport.FromLocalRecord(Last(),2)!;
    check(report.Build=="6ADFB11C-19E69000"&&report.BuildSource=="verified_session"&&report.Storefront=="steam"&&!ReadAll(contextLogs).Contains("PRIVATE_PLAYER")&&!ReadAll(contextLogs).Contains("PRIVATE_PATH"),"diagnostics_known_live_context_retains_allowlisted_build_storefront_without_summary_data");
    string verifiedFingerprint=report.Fingerprint;
    var verified=JsonSerializer.SerializeToElement(new{connected=true,live=true,profile="FM27 / 6ADFB11C-19E69000",storefront="steam",diagnostic_context=new{pe_timestamp=0x6ADFB11Cu,image_size=0x19E69000u,storefront="unknown"}});
    log.ObserveConnectionContext(JsonSerializer.SerializeToElement(new{state="connected",summary=verified,diagnostic_context=new{pe_timestamp=0x6ADFB11Cu,image_size=0x19E69000u,storefront="unknown"}}));
    log.Failure("application","handled",new NullReferenceException("after poll"));report=SharedDiagnosticReport.FromLocalRecord(Last(),2)!;
    check(report.BuildSource=="verified_session"&&report.Fingerprint==verifiedFingerprint,"diagnostics_successful_poll_preserves_verified_session_provenance_and_fingerprint");
    log.ObserveConnectionContext(verified);log.Failure("application","handled",new NullReferenceException("after connect"));
    check(SharedDiagnosticReport.FromLocalRecord(Last(),2)!.BuildSource=="verified_session","diagnostics_successful_connect_summary_preserves_verified_build");
    log.ObserveGameContext(JsonSerializer.SerializeToElement(new{connected=true,live=false,profile="FM27 / 6ADFB11C-19E69000",storefront="steam"}));
    log.Failure("application","handled",new NullReferenceException("no context"));report=SharedDiagnosticReport.FromLocalRecord(Last(),2)!;
    check(report.Build==null&&report.BuildSource=="unknown"&&report.Storefront=="unknown","diagnostics_snapshot_context_never_claims_verified_live_build");
    log.ObserveGameContext(JsonSerializer.SerializeToElement(new{connected=true,live=true,profile="PRIVATE_PROFILE",storefront="PRIVATE_STORE"}));
    log.Failure("application","handled",new NullReferenceException("unknown context"));report=SharedDiagnosticReport.FromLocalRecord(Last(),2)!;
    check(report.Build==null&&report.Storefront=="unknown"&&!ReadAll(contextLogs).Contains("PRIVATE_PROFILE")&&!ReadAll(contextLogs).Contains("PRIVATE_STORE"),"diagnostics_unknown_build_and_storefront_are_not_recorded_as_free_text");
    log.ObserveGameContext(JsonSerializer.SerializeToElement(new{connected=true,live=true,profile="FM27 / 6ADFB11C-19E69000"}));log.ClearGameContext();
    log.Failure("application","handled",new NullReferenceException("cleared"));check(SharedDiagnosticReport.FromLocalRecord(Last(),2)!.Build==null,"diagnostics_session_invalidation_clears_known_build_context");
    log.ObserveConnectionContext(JsonSerializer.SerializeToElement(new{diagnostic_context=new{pe_timestamp=0x12345678u,image_size=0x01234000u,storefront="unknown",module_path="PRIVATE_MODULE",pid=99999,base_address="PRIVATE_ADDRESS"}}));
    log.Failure("application","handled",new InvalidOperationException("UnsupportedBuild"));report=SharedDiagnosticReport.FromLocalRecord(Last(),2)!;
    check(report.Build=="12345678-01234000"&&report.BuildSource=="observed_pe"&&!ReadAll(contextLogs).Contains("PRIVATE_MODULE")&&!ReadAll(contextLogs).Contains("PRIVATE_ADDRESS"),"diagnostics_failed_connection_retains_only_numeric_pe_fingerprint_without_claiming_verified_profile");
    log.ObserveConnectionContext(JsonSerializer.SerializeToElement(new{diagnostic_context=new{pe_timestamp="PRIVATE_TIMESTAMP",image_size=-1,storefront="PRIVATE_STORE"}}));
    log.Failure("application","handled",new InvalidOperationException("invalid context"));report=SharedDiagnosticReport.FromLocalRecord(Last(),2)!;
    check(report.Build==null&&report.Storefront=="unknown"&&!ReadAll(contextLogs).Contains("PRIVATE_TIMESTAMP"),"diagnostics_malformed_connection_context_is_ignored_and_clears_stale_identity");
    int beforeBuildChange=Events(contextLogs,"context_dedup");
    log.ObserveConnectionContext(JsonSerializer.SerializeToElement(new{diagnostic_context=new{pe_timestamp=0x12345678u,image_size=0x01234000u}}));log.Failure("application","context_dedup",new IOException("same error"),"same_context_error");
    log.ObserveConnectionContext(JsonSerializer.SerializeToElement(new{diagnostic_context=new{pe_timestamp=0x12345679u,image_size=0x01234000u}}));log.Failure("application","context_dedup",new IOException("same error"),"same_context_error");
    check(Events(contextLogs,"context_dedup")==beforeBuildChange+2,"diagnostics_dedup_keeps_identical_failures_from_different_build_contexts");
    await using(var queuedBridge=new BridgeClient(Environment.ProcessPath!,log)) {
     var poll=queuedBridge.CallAsync("connection_poll",new{delayedContextForDiagnostics=true});
     var snapshot=queuedBridge.CallAsync("load_snapshot",new{failSnapshotForDiagnostics=true});
     await poll;bool refused=false;try{await snapshot;}catch(BridgeException){refused=true;}
     log.Failure("application","handled",new InvalidOperationException("after rejected snapshot"));
     report=SharedDiagnosticReport.FromLocalRecord(Last(),2)!;
     check(refused&&report.Build==null&&report.BuildSource=="unknown","diagnostics_queued_snapshot_clears_prior_inflight_poll_context_after_lock");
    }
    await using(var bridge=new BridgeClient(Environment.ProcessPath!,log)) {
     bool rejected=false;try{await bridge.CallAsync("connect",new{observedPeForDiagnostics=true});}catch(BridgeException){rejected=true;}
     string commandRecord=ReadAll(contextLogs).Split('\n').Last(line=>line.Contains("\"event\":\"connect\""));
     report=SharedDiagnosticReport.FromLocalRecord(commandRecord,2)!;
     check(rejected&&report.Build=="12345678-01234000"&&report.BuildSource=="observed_pe"&&report.Code=="unsupported_build"&&report.FailureStage=="connect",
      "diagnostics_bridge_failed_connect_keeps_observed_pe_before_logging_exception");
     check(!ReadAll(contextLogs).Contains("PRIVATE_PE_PATH")&&!ReadAll(contextLogs).Contains("PRIVATE_PE_ADDRESS"),"diagnostics_bridge_context_excludes_extra_path_and_address_fields");
     bool lost=false;try{await bridge.CallAsync("connect",new{malformedForDiagnostics=true});}catch(BridgeException){lost=true;}
     commandRecord=ReadAll(contextLogs).Split('\n').Last(line=>line.Contains("\"event\":\"connect\""));
     report=SharedDiagnosticReport.FromLocalRecord(commandRecord,2)!;
     check(lost&&report.Build==null&&report.BuildSource=="unknown","diagnostics_new_connect_transport_failure_never_reuses_previous_pe_context");
    }
   }
   string denied=Path.Combine(root,"blocked");File.WriteAllText(denied,"keep");
   using(var broken=new DiagnosticsLog(denied,"test")) {
    broken.Failure("connection","failed",new IOException("original"));
    check(!broken.IsAvailable&&!string.IsNullOrWhiteSpace(broken.LastFailure)&&File.ReadAllText(denied)=="keep","diagnostics_io_failure_is_best_effort_and_does_not_change_existing_file");
    await using var bridge=new BridgeClient(Environment.ProcessPath!,broken);
    bool original=false;try{await bridge.CallAsync("fail");}catch(BridgeException error){original=error.Message=="테스트 거절";}
    check(original,"diagnostics_failed_disk_never_masks_original_bridge_error");
    File.Delete(denied);broken.Write("lifecycle","disk_restored");
    check(broken.IsAvailable&&ReadAll(denied).Contains("disk_restored"),"diagnostics_recovers_after_log_directory_becomes_available");
   }
  }finally{
   // This test owns its unique temp directory; never remove an external target.
   if(root.StartsWith(Path.GetFullPath(Path.GetTempPath()),StringComparison.OrdinalIgnoreCase)&&Path.GetFileName(root).StartsWith("touchline-diagnostics-test-",StringComparison.Ordinal))Directory.Delete(root,true);
  }
 }
 private static string ReadAll(string dir)=>string.Join("\n",Directory.GetFiles(dir,"*.jsonl").Where(p=>!File.GetAttributes(p).HasFlag(FileAttributes.ReparsePoint)).Select(p=>{using var stream=new FileStream(p,FileMode.Open,FileAccess.Read,FileShare.ReadWrite);using var reader=new StreamReader(stream);return reader.ReadToEnd();}));
 private static int Events(string dir,string name)=>ReadAll(dir).Split('\n').Count(line=>line.Contains("\"event\":\""+name+"\""));
}
