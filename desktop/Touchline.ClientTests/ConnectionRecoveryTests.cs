using Touchline.Wpf;

internal static class ConnectionRecoveryTests
{
 public static async Task RunAsync(Action<bool,string> check)
 {
  var session=new BridgeSession();
  string missing=Path.Combine(Path.GetTempPath(),"touchline-missing-"+Guid.NewGuid().ToString("N")+".exe");
  bool failed=false;try{await session.EnsureReadyAsync(()=>missing);}catch(FileNotFoundException){failed=true;}
  check(failed&&session.Client==null,"connection_startup_missing_engine_remains_retryable");
  try {
   var status=await session.EnsureReadyAsync(()=>Environment.ProcessPath!);
   check(status.GetProperty("command").GetString()=="status"&&session.Client!=null,"connection_retry_after_missing_engine_handshakes");
   var first=session.Client!;
   try{await first.CallAsync("fail");}catch(BridgeException){}
   await session.EnsureReadyAsync(()=>throw new Exception("must not resolve or replace live engine"));
   check(ReferenceEquals(first,session.Client),"connection_backend_error_reuses_live_engine");
   BridgeException? exitFailure=null;
   try{await first.CallAsync("exit");}catch(BridgeException error){exitFailure=error;}
   check(exitFailure!=null&&SharedDiagnosticReport.Classify(exitFailure.Message)=="engine_transport","connection_exited_worker_reports_engine_transport");
   for(int i=0;i<40&&!first.HasExited;i++)await Task.Delay(25);
   check(first.HasExited,"connection_owned_worker_really_exited");
   status=await session.EnsureReadyAsync(()=>Environment.ProcessPath!);
   check(!ReferenceEquals(first,session.Client)&&status.GetProperty("command").GetString()=="status","connection_exited_worker_restarts_with_status_only");

   // Keep the exited worker's diagnostic pump draining so disposal yields.
   // Two simultaneous recovery callers must still create only one successor.
   var exited=session.Client!;
   using var diagnosticEntered=new ManualResetEventSlim();
   using var releaseDiagnostic=new ManualResetEventSlim();
   exited.Diagnostic+=_=>{diagnosticEntered.Set();releaseDiagnostic.Wait(TimeSpan.FromSeconds(10));};
   await exited.CallAsync("diagnostic");
   check(diagnosticEntered.Wait(TimeSpan.FromSeconds(5)),"connection_recovery_fixture_diagnostic_started");
   try{await exited.CallAsync("exit");}catch(BridgeException){}
   for(int i=0;i<40&&!exited.HasExited;i++)await Task.Delay(25);
   check(exited.HasExited,"connection_concurrent_recovery_fixture_exited");
   int starts=0;
   string ResolveRecoveryEngine(){Interlocked.Increment(ref starts);return Environment.ProcessPath!;}
   var firstRecovery=session.EnsureReadyAsync(ResolveRecoveryEngine);
   var secondRecovery=session.EnsureReadyAsync(ResolveRecoveryEngine);
   var intermediate=session.Client;
   releaseDiagnostic.Set();
   try {
    await Task.WhenAll(firstRecovery,secondRecovery);
    check(starts==1,"connection_concurrent_recovery_starts_one_owned_successor");
    check(!ReferenceEquals(exited,session.Client),"connection_concurrent_recovery_owns_replacement");
   } finally {if(intermediate!=null&&!ReferenceEquals(intermediate,session.Client))await intermediate.DisposeAsync();}

   // A startup exception after asynchronous disposal must release the lock
   // for an already queued retry rather than strand the connection screen.
   var startupFailureWorker=session.Client!;
   using var failureDiagnosticEntered=new ManualResetEventSlim();
   using var releaseFailureDiagnostic=new ManualResetEventSlim();
   startupFailureWorker.Diagnostic+=_=>{failureDiagnosticEntered.Set();releaseFailureDiagnostic.Wait(TimeSpan.FromSeconds(10));};
   await startupFailureWorker.CallAsync("diagnostic");
   check(failureDiagnosticEntered.Wait(TimeSpan.FromSeconds(5)),"connection_startup_retry_fixture_diagnostic_started");
   try{await startupFailureWorker.CallAsync("exit");}catch(BridgeException){}
   for(int i=0;i<40&&!startupFailureWorker.HasExited;i++)await Task.Delay(25);
   check(startupFailureWorker.HasExited,"connection_startup_retry_fixture_exited");
   int attempts=0;
   string ResolveRetryEngine()=>Interlocked.Increment(ref attempts)==1?missing:Environment.ProcessPath!;
   var failedStartup=session.EnsureReadyAsync(ResolveRetryEngine);
   var queuedRetry=session.EnsureReadyAsync(ResolveRetryEngine);
   releaseFailureDiagnostic.Set();
   bool startupFailed=false;
   try{await failedStartup;}catch(FileNotFoundException){startupFailed=true;}
   status=await queuedRetry;
   check(startupFailed&&attempts==2,"connection_startup_failure_releases_lock_for_queued_retry");
   check(status.GetProperty("command").GetString()=="status"&&session.Client!=null&&!ReferenceEquals(startupFailureWorker,session.Client),"connection_queued_retry_after_startup_failure_owns_healthy_worker");

   var live=session.Client!;string marker=Path.Combine(Path.GetTempPath(),"touchline-recovery-"+Guid.NewGuid().ToString("N"));
   try {
    try{await live.CallAsync("uncertain_write",new{marker});}catch(BridgeException){}
    bool refused=false;try{await session.EnsureReadyAsync(()=>throw new Exception("must not replace writing worker"));}catch(BridgeException){refused=true;}
    check(refused&&ReferenceEquals(live,session.Client)&&!live.HasExited,"connection_broken_live_worker_is_not_replaced");
    for(int i=0;i<100&&!File.Exists(marker);i++)await Task.Delay(40);
    check(File.Exists(marker)&&File.ReadAllText(marker)=="completed","connection_recovery_does_not_interrupt_or_repeat_accepted_write");
   }finally{if(File.Exists(marker))File.Delete(marker);}
  } finally {if(session.Client!=null)await session.Client.DisposeAsync();}
  foreach(string state in new[]{"error","unsupported_build","engine_error"})check(ConnectionPresentation.IsProblem(state),"connection_problem_state_"+state);
  check(ConnectionPresentation.For("error","FM 접근 실패 (os error 5)").Label.Contains("권한"),"connection_denied_has_permission_guidance");
  check(new[]{"os error 50","os error 500","os error 51","bios error 5","os error 5_suffix"}.All(reason=>!ConnectionPresentation.For("error",reason).Label.Contains("권한")),"connection_unrelated_os_codes_never_offer_permission_guidance");
  check(new[]{"OS ERROR 5","액세스가 거부되었습니다.","Access is denied."}.All(reason=>ConnectionPresentation.For("error",reason).Label.Contains("권한")),"connection_exact_and_translated_denial_keep_permission_guidance");
  check(ConnectionPresentation.For("unsupported_build","x").Guidance.Contains("검증되지"),"connection_unknown_build_remains_blocked");
  string report=ConnectionPresentation.Report("0.13.2","error","read 0x1234ABCD1234 token=secret bearer abcdef");
  check(report.Contains("0.13.2")&&!report.Contains("1234ABCD")&&!report.Contains("secret")&&!report.Contains("abcdef"),"connection_share_report_excludes_addresses_tokens");
  var reload=new LiveReloadState();reload.Enable();
  check(reload.Observe(new(){State="error",Monitoring=true}).Invalidate,"connection_error_invalidates_stale_screen");
  // Connection succeeds but building the first search screen throws. The
  // backend remains connected, so only an explicit UI invalidation can retry.
  reload.Enable();var connected=new LiveConnectionObservation{State="connected",Monitoring=true,Generation=7,SessionGeneration=7};
  reload.Observe(connected);reload.InvalidateView();
  var restore=reload.Observe(connected);
  check(restore.RestoreConnected&&!restore.Invalidate&&!restore.Reconnect,"connection_initial_render_failure_restores_already_connected_summary");
  // SetSummary resets the watcher before rendering; a second render failure
  // must mark it pending again rather than stranding the blank/error screen.
  reload.Enable();reload.InvalidateView();
  check(reload.Observe(connected).RestoreConnected,"connection_repeated_render_failure_remains_retryable");
  reload.Enable();
  check(reload.Observe(connected)==default&&!reload.Invalidated,"connection_successful_render_finishes_restore");
  reload.Disable();reload.InvalidateView();check(!reload.Invalidated,"connection_disabled_snapshot_cannot_arm_restore");
 }
}
