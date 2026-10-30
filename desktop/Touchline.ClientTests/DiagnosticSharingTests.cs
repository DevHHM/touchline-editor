using System.Net;
using System.Net.Http;
using System.Text.Json;
using System.Text.Json.Nodes;
using Touchline.Wpf;

internal static class DiagnosticSharingTests
{
 private static readonly DiagnosticCollector Collector=new("https://diagnostics.example.test/v1/errors","https://diagnostics.example.test/privacy","private");
 private sealed record Sent(Uri? Uri,string Method,string ContentType,string Body);
 private sealed class FakeHandler : HttpMessageHandler
 {
  public List<Sent> Requests {get;}=[];
  public Func<CancellationToken,Task<HttpResponseMessage>> Respond {get;set;}=_=>Task.FromResult(new HttpResponseMessage(HttpStatusCode.Accepted));
  protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken cancellationToken)
  {
   string body=request.Content==null?"":await request.Content.ReadAsStringAsync(cancellationToken);
   Requests.Add(new(request.RequestUri,request.Method.Method,request.Content?.Headers.ContentType?.MediaType??"",body));
   return await Respond(cancellationToken);
  }
 }
 private static string Record(string version="0.13.2",string message="Access is denied (os error 5)",string category="connection",string eventName="failed",string source="client",string level="error") => JsonSerializer.Serialize(new{
  timestamp=DateTimeOffset.UtcNow.ToString("O"),version,session="LOCAL_SESSION_PRIVATE",source,level,category,@event=eventName,message,
  causes=new[]{new{type="PRIVATE_EXCEPTION_TYPE",message="PRIVATE_CAUSE_TEXT"}},args=new{token="PRIVATE_ARGS_TOKEN",name="PRIVATE_PLAYER_NAME"},path="C:\\Users\\PRIVATE_USER\\Save.fm"});
 private static int Pending(string root)=>Directory.Exists(root)?Directory.GetFiles(root,"pending-*.json").Length:0;
 private static DiagnosticSharingClient Client(string root,FakeHandler handler,DiagnosticCollector? collector=null)=>new(root,collector??Collector,handler,timer:false);

 public static async Task RunAsync(Action<bool,string> check)
 {
  string root=Path.GetFullPath(Path.Combine(Path.GetTempPath(),"touchline-sharing-tests-"+Guid.NewGuid().ToString("N")));Directory.CreateDirectory(root);
  try{
   string folder=Path.Combine(root,"no-consent");var handler=new FakeHandler();
   using(var client=Client(folder,handler)){
    client.EnqueueApproved(Record());await client.SendPendingAsync();
    check(client.Configured&&!client.Enabled&&handler.Requests.Count==0&&Pending(folder)==0,"sharing_no_requests_or_outbox_before_explicit_consent");
   }
   folder=Path.Combine(root,"unconfigured");handler=new();
   using(var client=new DiagnosticSharingClient(folder,null,handler,timer:false)){
    bool refused=false;try{client.SetEnabled(true);}catch(InvalidOperationException){refused=true;}
    client.EnqueueApproved(Record());await client.SendPendingAsync();
    check(refused&&!client.Configured&&!client.Enabled&&handler.Requests.Count==0&&Pending(folder)==0,"sharing_unconfigured_collector_cannot_enable_or_send");
   }

   folder=Path.Combine(root,"privacy");handler=new();
   using(var client=Client(folder,handler)){
    client.SetEnabled(true);
    string privateMessage="PRIVATE_PLAYER_NAME PRIVATE_SAVE_NAME C:\\Users\\PRIVATE_USER\\Documents\\Save.fm 0x7FF123456789 token=PRIVATE_TOKEN bearer PRIVATE_BEARER https://private.example/?secret=PRIVATE_URL_SECRET Access is denied (os error 5) build 6ADFB11C/19E69000";
    client.EnqueueApproved(Record(message:privateMessage,eventName:"PRIVATE_EVENT_TEXT"));
    string pending=File.ReadAllText(Directory.GetFiles(folder,"pending-*.json").Single());
    check(!pending.Contains("PRIVATE",StringComparison.Ordinal)&&!pending.Contains("0x7FF",StringComparison.Ordinal)&&!pending.Contains("C:\\",StringComparison.Ordinal),"sharing_outbox_contains_no_log_message_causes_arguments_names_paths_or_tokens");
    await client.SendPendingAsync();var sent=handler.Requests.Single();using var parsed=JsonDocument.Parse(sent.Body);var report=parsed.RootElement;
    string[] allowed=["schema","product","report_id","occurred_at","editor_version","source","category","event","code","build"];
    check(!sent.Body.Contains("PRIVATE",StringComparison.Ordinal)&&report.EnumerateObject().All(p=>allowed.Contains(p.Name))&&report.GetProperty("code").GetString()=="access_denied"&&report.GetProperty("event").GetString()=="connection_failed"&&report.GetProperty("build").GetString()=="6ADFB11C-19E69000","sharing_wire_payload_has_only_allowlisted_metadata_and_safe_error_codes");
    check(sent.Uri?.AbsoluteUri==Collector.IngestUrl&&sent.Method=="POST"&&sent.ContentType=="application/json"&&Pending(folder)==0,"sharing_202_accepted_removes_exact_pending_report");
    client.EnqueueApproved(Record(message:privateMessage+" different private detail",eventName:"another_private_event"));await client.SendPendingAsync();
    check(handler.Requests.Count==1&&Pending(folder)==0,"sharing_sent_fingerprint_deduplicates_private_message_variations");
    client.EnqueueApproved(Record(category:"command",eventName:"PRIVATE_COMMAND",message:privateMessage));client.EnqueueApproved(Record(level:"info"));client.EnqueueApproved(Record(source:"PRIVATE_SOURCE"));await client.SendPendingAsync();
    check(handler.Requests.Count==1&&Pending(folder)==0,"sharing_nonerror_unknown_source_and_unapproved_command_are_not_shared");
   }

   folder=Path.Combine(root,"pending-dedup");handler=new();
   using(var client=Client(folder,handler)){
    client.SetEnabled(true);for(int i=0;i<30;i++)client.EnqueueApproved(Record(message:"Access is denied (os error 5) private detail "+i));
    check(Pending(folder)==1,"sharing_pending_duplicate_error_creates_one_report");
    await client.SendPendingAsync();check(handler.Requests.Count==1&&Pending(folder)==0,"sharing_pending_duplicate_error_sends_once");
   }

   foreach(var status in new[]{HttpStatusCode.OK,HttpStatusCode.Redirect,HttpStatusCode.TooManyRequests,HttpStatusCode.InternalServerError}){
    folder=Path.Combine(root,"retry-"+(int)status);handler=new(){Respond=_=>Task.FromResult(new HttpResponseMessage(status){Headers={Location=new Uri("https://redirect.example.test/collect")}})};
    using var client=Client(folder,handler);client.SetEnabled(true);client.EnqueueApproved(Record());await client.SendPendingAsync();await client.SendPendingAsync();
    check(handler.Requests.Count==1&&Pending(folder)==1&&client.Status.Contains("5분",StringComparison.Ordinal),"sharing_non202_retained_with_backoff_no_redirect_or_immediate_retry_"+(int)status);
   }
   folder=Path.Combine(root,"network-error");handler=new(){Respond=_=>throw new HttpRequestException("PRIVATE_NETWORK_EXCEPTION")};
   using(var client=Client(folder,handler)){
    client.SetEnabled(true);client.EnqueueApproved(Record());await client.SendPendingAsync();await client.SendPendingAsync();
    check(handler.Requests.Count==1&&Pending(folder)==1&&!client.Status.Contains("PRIVATE",StringComparison.Ordinal),"sharing_transport_error_keeps_queue_backs_off_and_does_not_echo_exception");
   }

   folder=Path.Combine(root,"bounds");handler=new();
   using(var client=Client(folder,handler)){
    client.SetEnabled(true);for(int i=0;i<45;i++)client.EnqueueApproved(Record(version:$"0.13.{i}"));
    check(Pending(folder)==32,"sharing_offline_outbox_is_bounded_to_32_reports");
    await client.SendPendingAsync();await client.SendPendingAsync();client.EnqueueApproved(Record(version:"0.99.99"));
    check(handler.Requests.Count==20&&Pending(folder)==12,"sharing_daily_rate_limit_stops_after_20_accepted_reports");
    client.SetEnabled(false);await client.SendPendingAsync();client.EnqueueApproved(Record(version:"0.98.98"));
    check(!client.Enabled&&Pending(folder)==0&&handler.Requests.Count==20,"sharing_revocation_deletes_pending_reports_and_blocks_new_reports");
   }

   folder=Path.Combine(root,"revoke-inflight");handler=new();var began=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);bool cancelled=false;
   handler.Respond=async cancellation=>{began.TrySetResult();try{await Task.Delay(Timeout.InfiniteTimeSpan,cancellation);}catch(OperationCanceledException){cancelled=true;throw;}return new(HttpStatusCode.Accepted);};
   using(var client=Client(folder,handler)){
    client.SetEnabled(true);client.EnqueueApproved(Record());Task sending=client.SendPendingAsync();await began.Task.WaitAsync(TimeSpan.FromSeconds(3));client.SetEnabled(false);await sending.WaitAsync(TimeSpan.FromSeconds(3));await client.SendPendingAsync();
    check(cancelled&&handler.Requests.Count==1&&Pending(folder)==0&&!client.Enabled,"sharing_revocation_cancels_inflight_send_and_clears_outbox");
   }

   await TestRevocationPersistenceFailure(root,check);
   await TestConsentScopes(root,check);
   await TestPerErrorConsent(root,check);

   TestCollectorConfiguration(root,check);
   await TestTamperedOutbox(root,check);
  }finally{
   string temporaryRoot=Path.TrimEndingDirectorySeparator(Path.GetFullPath(Path.GetTempPath()))+Path.DirectorySeparatorChar;
   if(root.StartsWith(temporaryRoot,StringComparison.OrdinalIgnoreCase)&&Path.GetFileName(root).StartsWith("touchline-sharing-tests-",StringComparison.Ordinal))Directory.Delete(root,true);
  }
 }
 private static async Task TestPerErrorConsent(string root,Action<bool,string> check)
 {
  string folder=Path.Combine(root,"per-error-no");var handler=new FakeHandler();int prompts=0;
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,(report,token)=>{prompts++;return Task.FromResult(false);})){
   await gate.ObserveAsync(Record());await gate.ObserveAsync(Record(category:"command",eventName:"connect"));await client.SendPendingAsync();
   check(prompts==1&&handler.Requests.Count==0&&Pending(folder)==0&&!client.Enabled,"sharing_error_prompts_once_across_duplicate_log_sources_and_no_does_not_consent");
  }
  folder=Path.Combine(root,"per-error-missing-ui");handler=new();
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,null)){
   await gate.ObserveAsync(Record());await client.SendPendingAsync();
   check(handler.Requests.Count==0&&Pending(folder)==0&&!client.Enabled,"sharing_absent_prompt_handler_never_implies_consent");
  }
  folder=Path.Combine(root,"per-error-exception");handler=new();
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,(_,_)=>throw new IOException("prompt failed"))){
   await gate.ObserveAsync(Record());await client.SendPendingAsync();
   check(handler.Requests.Count==0&&Pending(folder)==0&&!client.Enabled,"sharing_prompt_failure_never_implies_consent");
  }
  folder=Path.Combine(root,"per-error-yes");handler=new();prompts=0;
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,(report,token)=>{prompts++;return Task.FromResult(prompts==1);})){
   await gate.ObserveAsync(Record());await gate.ObserveAsync(Record(version:"0.14.2"));await client.SendPendingAsync();
   check(prompts==2&&handler.Requests.Count==1&&Pending(folder)==0,"sharing_yes_approves_only_one_report_and_next_distinct_error_still_requires_prompt");
  }
  folder=Path.Combine(root,"per-error-persisted");handler=new();
  using(var prior=Client(folder,new()))prior.SetEnabled(true);
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,(_,_)=>Task.FromResult(false))){
   await gate.ObserveAsync(Record());await client.SendPendingAsync();
   check(client.Enabled&&handler.Requests.Count==0&&Pending(folder)==0,"sharing_existing_outbox_consent_cannot_approve_new_errors");
  }
  folder=Path.Combine(root,"per-error-legacy-auto");handler=new();Directory.CreateDirectory(folder);
  string legacyScope=Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes("1\n"+Collector.IngestUrl+"\n"+Collector.PrivacyUrl+"\n"+Collector.Destination)));
  File.WriteAllText(Path.Combine(folder,"consent.json"),JsonSerializer.Serialize(new{Enabled=true,Scope=legacyScope}));
  var legacyReport=SharedDiagnosticReport.FromLocalRecord(Record())!;File.WriteAllText(Path.Combine(folder,"pending-"+legacyReport.ReportId+".json"),JsonSerializer.Serialize(legacyReport,SharedDiagnosticReport.Json));
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,(_,_)=>Task.FromResult(true))){
   await client.SendPendingAsync();check(!client.Enabled&&handler.Requests.Count==0,"sharing_old_global_consent_and_queue_cannot_send_without_new_per_error_approval");
   await gate.ObserveAsync(Record(version:"0.14.3"));
   check(handler.Requests.Count==1&&handler.Requests[0].Body.Contains("0.14.3")&&Pending(folder)==0,"sharing_first_per_error_approval_discards_old_automatic_queue");
  }
  folder=Path.Combine(root,"per-error-revoke-prompt");handler=new();var began=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);var release=new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,(_,_)=>{began.TrySetResult();return release.Task;})){
   Task observing=gate.ObserveAsync(Record());await began.Task.WaitAsync(TimeSpan.FromSeconds(3));gate.Revoke();release.SetResult(true);await observing;
   await gate.ObserveAsync(Record(version:"0.14.4"));await client.SendPendingAsync();
   check(!client.Enabled&&handler.Requests.Count==0&&Pending(folder)==0,"sharing_revocation_during_prompt_blocks_late_yes_and_new_prompts_for_session");
  }
  folder=Path.Combine(root,"per-error-serialize");handler=new();int active=0,maxActive=0;prompts=0;
  var unblock=new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,async(_,token)=>{prompts++;active++;maxActive=Math.Max(maxActive,active);await unblock.Task.WaitAsync(token);active--;return false;})){
   var tasks=Enumerable.Range(0,10).Select(i=>gate.ObserveAsync(Record(version:$"0.15.{i}"))).ToArray();unblock.SetResult(true);await Task.WhenAll(tasks);
   check(prompts==3&&maxActive==1&&handler.Requests.Count==0,"sharing_concurrent_errors_have_one_dialog_at_a_time_and_bounded_pending_prompts");
  }
  folder=Path.Combine(root,"per-error-cancel-handler-failed");handler=new();began=new(TaskCreationOptions.RunContinuationsAsynchronously);release=new(TaskCreationOptions.RunContinuationsAsynchronously);
  using(var client=Client(folder,handler))using(var gate=new DiagnosticErrorConsentGate(client,(_,token)=>{token.Register(()=>throw new InvalidOperationException("UI already closed"));began.TrySetResult();return release.Task;})){
   client.SetEnabled(true);client.EnqueueApproved(Record());Task observing=gate.ObserveAsync(Record(version:"0.14.9"));await began.Task;
   gate.Revoke();release.SetResult(true);await observing;await client.SendPendingAsync();
   check(!client.Enabled&&Pending(folder)==0&&handler.Requests.Count==0,"sharing_failed_prompt_cancellation_callback_cannot_prevent_transport_revocation");
  }
  folder=Path.Combine(root,"per-error-unconfigured");handler=new();prompts=0;
  using(var client=new DiagnosticSharingClient(folder,null,handler,timer:false))using(var gate=new DiagnosticErrorConsentGate(client,(_,_)=>{prompts++;return Task.FromResult(true);})){
   await gate.ObserveAsync(Record());check(prompts==0&&handler.Requests.Count==0,"sharing_unconfigured_server_does_not_request_pointless_consent");
  }
 }
 private static async Task TestRevocationPersistenceFailure(string root,Action<bool,string> check)
 {
  string folder=Path.Combine(root,"revocation-locked-consent");var handler=new FakeHandler();
  var began=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);bool cancelled=false;
  handler.Respond=async token=>{began.TrySetResult();try{await Task.Delay(Timeout.InfiniteTimeSpan,token);}catch(OperationCanceledException){cancelled=true;throw;}return new(HttpStatusCode.Accepted);};
  using(var client=Client(folder,handler)){
   client.SetEnabled(true);client.EnqueueApproved(Record());Task sending=client.SendPendingAsync();await began.Task.WaitAsync(TimeSpan.FromSeconds(3));
   using(var locked=new FileStream(Path.Combine(folder,"consent.json"),FileMode.Open,FileAccess.Read,FileShare.Read)){
    bool failed=false;try{client.SetEnabled(false);}catch(IOException){failed=true;}
    await sending.WaitAsync(TimeSpan.FromSeconds(3));
    client.EnqueueApproved(Record(version:"0.14.1"));await client.SendPendingAsync();
    check(failed&&cancelled&&!client.Enabled&&handler.Requests.Count==1&&Pending(folder)==0,"sharing_revocation_with_locked_consent_cancels_immediately_and_blocks_all_new_sends");
    check(client.Status.Contains("저장하지 못")&&client.Status.Contains("다음 실행"),"sharing_failed_revocation_persistence_is_reported_without_claiming_saved");
   }
   // The stale true file remains readable after its lock is released; only explicit new consent may enable this process.
   await client.SendPendingAsync();check(!client.Enabled&&handler.Requests.Count==1,"sharing_stale_true_consent_cannot_override_session_revocation");
   client.SetEnabled(false);
  }
  using(var restarted=Client(folder,new()))check(!restarted.Enabled,"sharing_revocation_retry_persists_disabled_state_for_restart");

  folder=Path.Combine(root,"revoke-unreadable-checkbox");handler=new();
  using(var client=Client(folder,handler)){
   client.SetEnabled(true);client.EnqueueApproved(Record());
   int actions=0,errors=0;bool visibleChecked=true;DiagnosticConsentToggle? toggle=null;
   toggle=new(()=>client.Enabled,requested=>{actions++;client.SetEnabled(requested);},value=>{visibleChecked=value;toggle!.Change(value);},_=>errors++);
   using(var inaccessible=new FileStream(Path.Combine(folder,"consent.json"),FileMode.Open,FileAccess.Read,FileShare.None)){
    check(!client.Enabled,"sharing_checkbox_regression_fixture_hides_persisted_consent");
    toggle.Change(false);
    check(actions==1&&errors==1&&!visibleChecked,"sharing_uncheck_always_revokes_even_when_disk_enabled_is_false_and_sync_does_not_reenter");
   }
   client.EnqueueApproved(Record(version:"0.14.3"));await client.SendPendingAsync();
   check(!client.Enabled&&handler.Requests.Count==0&&Pending(folder)==0,"sharing_checkbox_revocation_stays_off_when_unreadable_true_file_recovers");
   client.SetEnabled(false);
  }

  folder=Path.Combine(root,"revocation-late-response");handler=new();began=new(TaskCreationOptions.RunContinuationsAsynchronously);
  var release=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
  handler.Respond=async _=>{began.TrySetResult();await release.Task;return new(HttpStatusCode.Accepted);};
  using(var client=Client(folder,handler)){
   client.SetEnabled(true);client.EnqueueApproved(Record());Task sending=client.SendPendingAsync();await began.Task.WaitAsync(TimeSpan.FromSeconds(3));client.SetEnabled(false);release.TrySetResult();await sending.WaitAsync(TimeSpan.FromSeconds(3));
   check(!client.Enabled&&client.Status.Contains("꺼짐")&&Pending(folder)==0,"sharing_late_accepted_response_does_not_replace_revoked_status");
  }
 }
 private static async Task TestConsentScopes(string root,Action<bool,string> check)
 {
  string folder=Path.Combine(root,"legacy-consent");Directory.CreateDirectory(folder);File.WriteAllText(Path.Combine(folder,"consent.json"),"{\"Enabled\":true}");
  var handler=new FakeHandler();using(var client=Client(folder,handler)){
   client.EnqueueApproved(Record());await client.SendPendingAsync();
   check(!client.Enabled&&handler.Requests.Count==0,"sharing_legacy_boolean_consent_requires_fresh_opt_in");
   client.SetEnabled(true);check(client.Enabled,"sharing_fresh_opt_in_upgrades_legacy_consent_scope");
  }
  using(var same=Client(folder,new()))check(same.Enabled,"sharing_same_collector_and_disclosure_keep_persisted_consent");
  var alternatives=new[]{Collector with{IngestUrl="https://other.example.test/v1/errors"},Collector with{PrivacyUrl="https://diagnostics.example.test/privacy-v2"},Collector with{Destination="public"}};
  for(int i=0;i<alternatives.Length;i++){
   folder=Path.Combine(root,"changed-scope-"+i);
   using(var original=Client(folder,new())){original.SetEnabled(true);original.EnqueueApproved(Record());}
   handler=new();using var changed=Client(folder,handler,alternatives[i]);
   changed.EnqueueApproved(Record(version:"0.14.1"));await changed.SendPendingAsync();
   check(!changed.Enabled&&handler.Requests.Count==0&&Pending(folder)==1,"sharing_scope_change_blocks_prior_consent_"+i);
   changed.SetEnabled(true);check(changed.Enabled&&Pending(folder)==0,"sharing_scope_opt_in_discards_reports_collected_for_previous_destination_"+i);
   changed.EnqueueApproved(Record(version:"0.14.2"));await changed.SendPendingAsync();
   check(handler.Requests.Count==1&&handler.Requests[0].Uri?.AbsoluteUri==alternatives[i].IngestUrl,"sharing_scope_change_sends_only_after_new_explicit_consent_"+i);
  }
 }
 private static void TestCollectorConfiguration(string root,Action<bool,string> check)
 {
  string path=Path.Combine(root,"collector.json");
  void Config(string endpoint,string privacy,string destination)=>File.WriteAllText(path,JsonSerializer.Serialize(new{ingest_url=endpoint,privacy_url=privacy,destination}));
  Config(Collector.IngestUrl,Collector.PrivacyUrl,"private");check(DiagnosticCollector.Read(path)==Collector,"sharing_valid_https_collector_configuration_is_accepted");
  string[] urls=["http://collector.example.test/errors","https://user:password@collector.example.test/errors","https://collector.example.test:8443/errors","https://collector.example.test/errors?token=secret","https://collector.example.test/errors#fragment","https://localhost/errors","https://127.0.0.1/errors","https://192.168.1.8/errors","https://collector.local/errors","https://api.github.com/repos/owner/repo/issues","not a URL"];
  foreach(string url in urls){
   Config(url,Collector.PrivacyUrl,"private");bool rejected=DiagnosticCollector.Read(path)==null;
   Config(Collector.IngestUrl,url,"private");rejected&=DiagnosticCollector.Read(path)==null;
   check(rejected,"sharing_rejects_unsafe_ingest_and_privacy_url_"+Array.IndexOf(urls,url));
  }
  Config(Collector.IngestUrl,Collector.PrivacyUrl,"unexplained");check(DiagnosticCollector.Read(path)==null,"sharing_unknown_destination_is_rejected");
  File.WriteAllText(path,new string('x',5000));check(DiagnosticCollector.Read(path)==null,"sharing_oversized_or_malformed_collector_file_is_rejected");
 }
 private static async Task TestTamperedOutbox(string root,Action<bool,string> check)
 {
  string folder=Path.Combine(root,"tampered");var handler=new FakeHandler();using var client=Client(folder,handler);client.SetEnabled(true);
  var template=JsonNode.Parse(JsonSerializer.Serialize(SharedDiagnosticReport.FromLocalRecord(Record())!,SharedDiagnosticReport.Json))!.AsObject();
  int serial=0;
  void Put(string key,JsonNode? value){var row=(JsonObject)template.DeepClone();string id=(++serial).ToString("x32");row["report_id"]=id;row[key]=value;File.WriteAllText(Path.Combine(folder,"pending-"+id+".json"),row.ToJsonString());}
  Put("code","PRIVATE_ERROR_MESSAGE");Put("event","PRIVATE_EVENT");Put("source","PRIVATE_SOURCE");Put("category","PRIVATE_CATEGORY");Put("build","PRIVATE_BUILD");Put("editor_version","PRIVATE_VERSION");Put("report_id","PRIVATE_REPORT_ID");Put("report_id",Guid.NewGuid().ToString("N"));Put("product","PRIVATE_PRODUCT");Put("schema",99);Put("occurred_at",DateTimeOffset.UtcNow.AddDays(-8).ToString("O"));Put("occurred_at",DateTimeOffset.UtcNow.AddDays(1).ToString("O"));
  File.WriteAllText(Path.Combine(folder,"pending-"+(++serial).ToString("x32")+".json"),new string('x',3000));
  await client.SendPendingAsync();check(handler.Requests.Count==0&&Pending(folder)==0,"sharing_tampered_unknown_values_stale_and_oversized_reports_are_removed_without_transmission");
  var extra=(JsonObject)template.DeepClone();string validId=Guid.NewGuid().ToString("N");extra["report_id"]=validId;extra["message"]="PRIVATE_MESSAGE_IN_TAMPERED_QUEUE";extra["args"]=new JsonObject{["token"]="PRIVATE_TOKEN_IN_TAMPERED_QUEUE"};extra["fingerprint"]="PRIVATE_FINGERPRINT_IN_TAMPERED_QUEUE";
  File.WriteAllText(Path.Combine(folder,"pending-"+validId+".json"),extra.ToJsonString());await client.SendPendingAsync();
  check(handler.Requests.Count<=1&&handler.Requests.All(request=>!request.Body.Contains("PRIVATE",StringComparison.Ordinal)),"sharing_unknown_queue_properties_cannot_escape_the_allowlisted_report_schema");
 }
}
