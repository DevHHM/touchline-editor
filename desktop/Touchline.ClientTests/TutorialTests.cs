using System.Text.Json;
using Touchline.Wpf;

internal static class TutorialTests
{
 public static void Run(Action<bool, string> check)
 {
  var initial = new TutorialPreferences();
  check(initial.IsValid && initial.ShouldShow && initial.LastAction == TutorialExitAction.None, "tutorial_new_preferences_require_first_launch");
  bool noneRejected = false, unknownRejected = false;
  try { initial.Dismiss(TutorialExitAction.None); } catch (ArgumentException) { noneRejected = true; }
  try { initial.Dismiss((TutorialExitAction)99); } catch (ArgumentException) { unknownRejected = true; }
  check(noneRejected && unknownRejected && initial.ShouldShow, "tutorial_only_explicit_skip_or_completion_can_dismiss");

  string directory = Path.Combine(Path.GetTempPath(), "touchline-tutorial-test-" + Guid.NewGuid().ToString("N"));
  string path = Path.Combine(directory, "tutorial.json");
  try
  {
   check(TutorialPreferences.Load(path).ShouldShow && !Directory.Exists(directory), "tutorial_missing_settings_default_without_creating_files");
   bool unsavedRejected = false;
   try { initial.Save(path); } catch (ArgumentException) { unsavedRejected = true; }
   check(unsavedRejected && !Directory.Exists(directory), "tutorial_undismissed_state_cannot_be_saved");
   foreach (var action in new[] { TutorialExitAction.Skipped, TutorialExitAction.Completed })
   {
    var dismissed = initial.Dismiss(action); dismissed.Save(path);
    var restored = TutorialPreferences.Load(path);
    check(!dismissed.ShouldShow && dismissed.DismissedTutorialVersion == TutorialPreferences.CurrentTutorialVersion && restored == dismissed && restored.LastAction == action,
     "tutorial_" + action.ToString().ToLowerInvariant() + "_persists_current_version");
    using var json = JsonDocument.Parse(File.ReadAllText(path));
    check(!json.RootElement.TryGetProperty("ShouldShow", out _) && !json.RootElement.TryGetProperty("IsValid", out _) && Directory.GetFiles(directory, "*.tmp").Length == 0,
     "tutorial_" + action.ToString().ToLowerInvariant() + "_stores_only_state_and_leaves_no_temp_files");
   }
   string[] malformed =
   [
    "{broken", "null", "[]", "{\"Version\":99}", "{\"Version\":0}",
    "{\"DismissedTutorialVersion\":-1}", "{\"DismissedTutorialVersion\":99,\"LastAction\":2}",
    "{\"DismissedTutorialVersion\":1,\"LastAction\":0}", "{\"DismissedTutorialVersion\":0,\"LastAction\":2}",
    "{\"DismissedTutorialVersion\":1,\"LastAction\":99}", "{\"Version\":\"bad\"}"
   ];
   for (int index = 0; index < malformed.Length; ++index)
   {
    File.WriteAllText(path, malformed[index]);
    var restored = TutorialPreferences.Load(path);
    check(restored.IsValid && restored.ShouldShow && restored.LastAction == TutorialExitAction.None && File.ReadAllText(path) == malformed[index], "tutorial_corrupt_future_or_invalid_settings_safe_" + index);
   }
   File.WriteAllText(path, "{}");
   check(TutorialPreferences.Load(path).ShouldShow, "tutorial_empty_settings_do_not_suppress_first_launch");
   string oversized = JsonSerializer.Serialize(initial.Dismiss(TutorialExitAction.Completed)) + new string(' ', 8193);
   File.WriteAllText(path, oversized);
   check(TutorialPreferences.Load(path).ShouldShow && File.ReadAllText(path) == oversized, "tutorial_oversized_settings_rejected_without_rewrite");
   var invalid = new TutorialPreferences { Version = 99, DismissedTutorialVersion = 1, LastAction = TutorialExitAction.Completed };
   bool invalidSaveRejected = false;
   try { invalid.Save(path); } catch (ArgumentException) { invalidSaveRejected = true; }
   check(invalid.ShouldShow && invalidSaveRejected && File.ReadAllText(path) == oversized, "tutorial_invalid_record_cannot_overwrite_settings");
  }
  finally { if (File.Exists(path)) File.Delete(path); if (Directory.Exists(directory)) Directory.Delete(directory); }

  string[][] unattended =
  [
   ["--ui-check"], ["--ui-check", "qa.json"], ["--ui-check=qa.json"], ["--qa-offscreen", "true"], ["--qa-offscreen=true"],
   ["--snapshot", "fixture.json"], ["--snapshot=fixture.json"], ["--connect", "readonly"], ["--smoke", "all"], ["--layout-matrix", "true"],
   ["--tutorial-check", "true"], ["--ability-visibility-check=true"], ["--automation"], ["--automation=true"], ["--automated"], ["--headless"], ["--no-tutorial"]
  ];
  foreach (var arguments in unattended)
   check(!TutorialPreferences.IsInteractiveLaunch(arguments), "tutorial_unattended_launch_suppressed_" + arguments[0].Replace("--", "", StringComparison.Ordinal).Replace('=', '_'));
  check(TutorialPreferences.IsInteractiveLaunch([]) && TutorialPreferences.IsInteractiveLaunch(["--no-update", "true"]), "tutorial_normal_launch_and_update_opt_out_remain_interactive");
 }
}
