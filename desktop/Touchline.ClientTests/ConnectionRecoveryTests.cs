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
   try{await first.CallAsync("exit");}catch(BridgeException){}
   for(int i=0;i<40&&!first.HasExited;i++)await Task.Delay(25);
   check(first.HasExited,"connection_owned_worker_really_exited");
   status=await session.EnsureReadyAsync(()=>Environment.ProcessPath!);
   check(!ReferenceEquals(first,session.Client)&&status.GetProperty("command").GetString()=="status","connection_exited_worker_restarts_with_status_only");

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
