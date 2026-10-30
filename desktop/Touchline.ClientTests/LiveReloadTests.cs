using System.Text.Json;
using Touchline.Wpf;

internal static class LiveReloadTests
{
 public static void Run(Action<bool, string> check)
 {
  var state = new LiveReloadState();
  state.Enable();
  var initial = JsonSerializer.Deserialize<LiveConnectionObservation>("""
   {"state":"connected","monitoring":true,"changed":false,"generation":4,"session_generation":4,"data_revision":20}
   """);
  check(initial != null && state.Observe(initial) == default && state.SessionGeneration == 4, "reload_reads_separate_session_and_data_revisions");

  // Applying a scalar, relation, batch or undo may invalidate backend preview
  // tokens. None of those operations replaces the live FM session or profile.
  bool preserved = true;
  for (int revision = 21; revision <= 28; ++revision)
  {
   var decision = state.Observe(new() { State = "connected", Monitoring = true, Generation = 4, SessionGeneration = 4, DataRevision = revision });
   preserved &= decision == default && !state.Invalidated;
  }
  check(preserved, "reload_repeated_applies_and_undos_keep_current_profile");
  var day = state.Observe(new() { State = "connected", Monitoring = true, Generation = 4, SessionGeneration = 4, DataRevision = 29, DateChanged = true });
  check(day.RefreshDate && !day.Invalidate && !day.RestoreConnected && !state.Invalidated, "reload_date_after_apply_only_updates_date");

  // An older engine sends its data revision through generation. It still
  // supplies explicit changed/loading states when a real save is replaced.
  state.Enable();
  state.Observe(new() { State = "connected", Monitoring = true, Generation = 90 });
  var legacyApply = state.Observe(new() { State = "connected", Monitoring = true, Generation = 91 });
  check(legacyApply == default && !state.Invalidated && state.Generation == 91, "reload_legacy_apply_epoch_does_not_announce_new_save");
  var legacyLoad = state.Observe(new() { State = "loading", Monitoring = true, Generation = 92, Changed = true });
  check(legacyLoad.Invalidate && !legacyLoad.RestoreConnected && state.Invalidated, "reload_legacy_actual_load_still_discards_old_drafts");
  var legacyReady = state.Observe(new() { State = "ready", Monitoring = true, Generation = 92, CanReconnect = true });
  check(legacyReady.Reconnect && !legacyReady.Invalidate, "reload_legacy_real_load_still_reconnects");
  var legacyDone = state.Observe(new() { State = "reconnected", Monitoring = true, Generation = 93, Changed = true });
  check(!legacyDone.Invalidate && !legacyDone.RestoreConnected && !state.Invalidated, "reload_reconnected_reply_does_not_discard_restored_profile");

  // A connected session change can be observed when the reconnect completed
  // between polls. It must restore the supplied summary without waiting for
  // can_reconnect, which is false for an already connected engine.
  state.Enable();
  state.Observe(new() { State = "connected", Monitoring = true, Generation = 4, SessionGeneration = 4, DataRevision = 20 });
  var skippedTransition = state.Observe(new() { State = "connected", Monitoring = true, Generation = 5, SessionGeneration = 5, DataRevision = 22 });
  check(skippedTransition.Invalidate && skippedTransition.RestoreConnected && !skippedTransition.Reconnect, "reload_new_explicit_session_restores_already_connected_summary");
  var retryRestore = state.Observe(new() { State = "connected", Monitoring = true, Generation = 5, SessionGeneration = 5, DataRevision = 22 });
  check(!retryRestore.Invalidate && retryRestore.RestoreConnected, "reload_connected_restore_can_retry_without_discarding_twice");
  state.Enable(); // MainWindow.SetSummary enables the watcher after restoring.
  check(state.Observe(new() { State = "connected", Monitoring = true, Generation = 5, SessionGeneration = 5, DataRevision = 22 }) == default && !state.Invalidated,
   "reload_restored_session_resumes_normal_polling");
  var flagged = state.Observe(new() { State = "connected", Monitoring = true, Generation = 5, SessionGeneration = 5, Changed = true });
  check(flagged.Invalidate && flagged.RestoreConnected, "reload_explicit_changed_flag_is_never_ignored");
  state.Disable();
  check(state.Generation == null && state.SessionGeneration == null && !state.Invalidated, "reload_disable_clears_both_generation_baselines");
 }
}
