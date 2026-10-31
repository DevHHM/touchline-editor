using Touchline.Wpf;

internal static class DesktopUsabilityTests
{
 public static void Run(Action<bool, string> check)
 {
  var main = new DesktopMonitor("main", new(0, 0, 3840, 2160), new(0, 0, 3840, 2112), 1.5);
  var left = new DesktopMonitor("left", new(-2560, 0, 2560, 1440), new(-2560, 32, 2560, 1408), 1);
  var saved = new DesktopWindowPreferences { NormalBounds = new(-2500, 50, 1510, 990), MonitorDevice = "left", DpiScale = 1, ListWidth = 375, Maximized = true };
  check(saved.SelectMonitor([main, left]) == left && saved.FitTo(left) == saved.NormalBounds, "window_restore_preserves_negative_secondary_monitor_and_top_taskbar");
  var fallback = saved.SelectMonitor([main]);
  var fitted = saved.FitTo(fallback);
  check(fallback == main && fitted.Left == 0 && fitted.Width == 2265 && fitted.Height == 1485 && fitted.Bottom <= main.WorkArea.Bottom, "window_removed_monitor_fits_primary_and_scales_100_to_150_percent");
  var detached = saved with { MonitorDevice = "removed", NormalBounds = new(-5000, -2000, 1800, 1100) };
  check(detached.SelectMonitor([main, left]) == left, "window_offscreen_restore_chooses_nearest_monitor_not_main");
  var overlap = saved with { MonitorDevice = "unknown", NormalBounds = new(-100, 100, 1600, 1000) };
  check(overlap.SelectMonitor([left, main]) == main, "window_restore_chooses_greatest_monitor_overlap");
  var small = new DesktopMonitor("laptop", new(0, 0, 1366, 768), new(0, 0, 1366, 728), 1.5);
  var smallFit = saved.FitTo(small);
  check(smallFit.Left >= 0 && smallFit.Top >= 0 && smallFit.Right <= 1366 && smallFit.Bottom <= 728, "window_high_dpi_small_display_keeps_titlebar_inside_workarea");
  var ultraWide = saved with { NormalBounds = new(-10000, -10000, 99999, 99999), ListWidth = 900 };
  check(ultraWide.FitTo(left) == left.WorkArea && ultraWide.ValidListWidth == 420, "window_oversized_bounds_and_sidebar_clamped");
  check((saved with { DpiScale = 0 }).IsValid == false && (saved with { NormalBounds = new(double.NaN, 0, 1510, 990) }).IsValid == false && (saved with { Version = 999 }).IsValid == false, "window_invalid_settings_rejected");
  check((saved with { ListWidth = double.NaN }).ValidListWidth == null && (saved with { ListWidth = 1 }).ValidListWidth == 240, "window_invalid_sidebar_width_rejected_or_clamped");
  string directory = Path.Combine(Path.GetTempPath(), "touchline-window-test-" + Guid.NewGuid().ToString("N"));
  string path = Path.Combine(directory, "window.json");
  try
  {
   check(DesktopWindowPreferences.Load(path) == null, "window_missing_preferences_use_defaults");
   saved.Save(path);
   check(DesktopWindowPreferences.Load(path) == saved, "window_preferences_atomic_roundtrip_preserves_maximized_and_list_width");
   (saved with { Maximized = false, ListWidth = 300 }).Save(path);
   check(DesktopWindowPreferences.Load(path)?.ListWidth == 300 && Directory.GetFiles(directory, "*.tmp").Length == 0, "window_preferences_replace_without_temporary_files");
   File.WriteAllText(path, "{incomplete");
   check(DesktopWindowPreferences.Load(path) == null, "window_corrupt_preferences_fall_back_without_startup_error");
  }
  finally { if (File.Exists(path)) File.Delete(path); if (Directory.Exists(directory)) Directory.Delete(directory); }
  string scaleDirectory = Path.Combine(Path.GetTempPath(), "touchline-scale-test-" + Guid.NewGuid().ToString("N"));
  string scalePath = Path.Combine(scaleDirectory, "ui-scale.json");
  try
  {
   check(UiScalePreferences.Load(scalePath).Percent == 100, "ui_scale_first_run_defaults_to_100");
   foreach (int percent in UiScalePreferences.Presets)
   {
    new UiScalePreferences { Percent = percent }.Save(scalePath);
    check(UiScalePreferences.Load(scalePath).Percent == percent, $"ui_scale_persists_{percent}_across_reloads");
   }
   foreach (string invalid in new[] { "{broken", "null", "{\"Version\":99,\"Percent\":150}", "{\"Version\":1,\"Percent\":0}", "{\"Version\":1,\"Percent\":9999}" })
   {
    File.WriteAllText(scalePath, invalid);
    check(UiScalePreferences.Load(scalePath).Percent == 100, "ui_scale_corrupt_or_unsupported_setting_uses_readable_default");
   }
   check(Directory.GetFiles(scaleDirectory, "*.tmp").Length == 0, "ui_scale_atomic_save_leaves_no_temporary_files");
  }
  finally { if (File.Exists(scalePath)) File.Delete(scalePath); if (Directory.Exists(scaleDirectory)) Directory.Delete(scaleDirectory); }
  check(UiScalePreferences.Step(100, 1) == 110 && UiScalePreferences.Step(100, -1) == 90 && UiScalePreferences.Step(150, 1) == 150 && UiScalePreferences.Step(80, -1) == 80, "ui_scale_keyboard_steps_and_bounds");
  check(EditorShortcuts.Resolve("F", ShortcutModifiers.Control) == EditorShortcut.Search && EditorShortcuts.Resolve("F", ShortcutModifiers.Control | ShortcutModifiers.Shift) == EditorShortcut.SearchFilters, "shortcuts_quick_and_advanced_search_distinct");
  check(EditorShortcuts.Resolve("F5", ShortcutModifiers.None) == EditorShortcut.Refresh && EditorShortcuts.Resolve("R", ShortcutModifiers.Control) == EditorShortcut.Refresh, "shortcuts_both_refresh_keys");
  check(EditorShortcuts.Resolve("Left", ShortcutModifiers.Alt) == EditorShortcut.Back && EditorShortcuts.Resolve("Right", ShortcutModifiers.Alt) == EditorShortcut.Forward, "shortcuts_profile_history_navigation");
  check(EditorShortcuts.Resolve("S", ShortcutModifiers.Control) == EditorShortcut.Review, "shortcuts_ctrl_s_only_opens_review");
  check(EditorShortcuts.Resolve("F", ShortcutModifiers.Control | ShortcutModifiers.Alt) == EditorShortcut.None && EditorShortcuts.Resolve("Left", ShortcutModifiers.None) == EditorShortcut.None && EditorShortcuts.Resolve("S", ShortcutModifiers.Control | ShortcutModifiers.Shift) == EditorShortcut.None, "shortcuts_text_navigation_altgr_and_modified_keys_not_stolen");
 }
}
