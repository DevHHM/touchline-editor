using Touchline.Wpf;

internal static class SnapshotTransitionTests
{
 public static async Task RunAsync(Action<bool,string> check)
 {
  var watch=new LiveReloadState();
  watch.Enable();
  watch.Observe(new(){State="connected",Monitoring=true,Generation=7,SessionGeneration=7});
  int snapshots=0;
  var close=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
  var canceled=SnapshotTransition.RunAsync(()=>close.Task,watch.Disable,()=>{++snapshots;return Task.CompletedTask;});
  bool watchingWhileClosePending=watch.Enabled;
  close.SetException(new OperationCanceledException("Workbench close declined"));
  bool completed=await canceled;
  check(!completed&&watchingWhileClosePending&&watch.Enabled&&watch.Generation==7&&watch.SessionGeneration==7,
   "snapshot_declined_workbench_close_preserves_live_monitor_and_session");
  check(snapshots==0,"snapshot_declined_workbench_close_never_sends_load_snapshot");

  bool failed=false;
  try {await SnapshotTransition.RunAsync(()=>Task.FromException(new IOException("Workbench close failed")),watch.Disable,()=>{++snapshots;return Task.CompletedTask;});}
  catch(IOException) {failed=true;}
  check(failed&&watch.Enabled&&watch.SessionGeneration==7&&snapshots==0,"snapshot_failed_workbench_close_preserves_session_without_loading");

  var events=new List<string>();
  var acceptedClose=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
  var accepted=SnapshotTransition.RunAsync(async()=>{events.Add("closing");await acceptedClose.Task;events.Add("closed");},
   ()=>{events.Add("transition");watch.Disable();},()=>{events.Add("load_snapshot");++snapshots;return Task.CompletedTask;});
  bool stillWatchingBeforeClose=watch.Enabled;
  acceptedClose.SetResult();
  completed=await accepted;
  check(completed&&stillWatchingBeforeClose&&!watch.Enabled&&snapshots==1&&events.SequenceEqual(new[]{"closing","closed","transition","load_snapshot"}),
   "snapshot_success_disables_live_monitor_after_close_and_sends_once");

  // Cancellation after the snapshot path starts is not a declined workbench
  // close. Preserve the error rather than reporting an untouched session.
  watch.Enable();
  bool loadCanceled=false;
  try {await SnapshotTransition.RunAsync(()=>Task.CompletedTask,watch.Disable,()=>throw new OperationCanceledException("Snapshot request canceled"));}
  catch(OperationCanceledException) {loadCanceled=true;}
  check(loadCanceled&&!watch.Enabled,"snapshot_request_cancellation_is_not_mistaken_for_workbench_decline");
 }
}
