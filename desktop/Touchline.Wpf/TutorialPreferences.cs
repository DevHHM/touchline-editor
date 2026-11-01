using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public enum TutorialExitAction { None, Skipped, Completed }

/// <summary>Local onboarding state. This helper has no WPF or game dependencies.</summary>
public sealed record TutorialPreferences
{
 public const int CurrentTutorialVersion = 1;
 public int Version { get; init; } = 1;
 public int DismissedTutorialVersion { get; init; }
 public TutorialExitAction LastAction { get; init; }
 [JsonIgnore] public bool IsValid => Version == 1 && DismissedTutorialVersion is >= 0 and <= CurrentTutorialVersion
  && (DismissedTutorialVersion == 0 ? LastAction == TutorialExitAction.None : LastAction is TutorialExitAction.Skipped or TutorialExitAction.Completed);
 [JsonIgnore] public bool ShouldShow => !IsValid || DismissedTutorialVersion < CurrentTutorialVersion;
 public static string SettingsPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "tutorial.json");

 public TutorialPreferences Dismiss(TutorialExitAction action)
 {
  if (action is not (TutorialExitAction.Skipped or TutorialExitAction.Completed)) throw new ArgumentException("An explicit skip or completion is required.", nameof(action));
  return new() { DismissedTutorialVersion = CurrentTutorialVersion, LastAction = action };
 }

 /// <summary>Check before any settings IO; unattended runs must not consume first-launch state.</summary>
 public static bool IsInteractiveLaunch(IEnumerable<string> arguments) => !arguments.Any(argument =>
 {
  string option = argument.Split('=', 2)[0];
  return option.StartsWith("--", StringComparison.Ordinal) &&
   (option.StartsWith("--qa", StringComparison.OrdinalIgnoreCase) || option.EndsWith("-check", StringComparison.OrdinalIgnoreCase)
    || option is "--snapshot" or "--connect" or "--smoke" or "--layout-matrix" or "--automation" or "--automated" or "--headless" or "--no-tutorial");
 });

 public static TutorialPreferences Load(string path)
 {
  try
  {
   if (File.Exists(path) && new FileInfo(path).Length <= 8192 && JsonSerializer.Deserialize<TutorialPreferences>(File.ReadAllText(path)) is { IsValid: true } saved) return saved;
  }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException or NotSupportedException) { }
  return new();
 }

 public void Save(string path)
 {
  if (!IsValid || ShouldShow) throw new ArgumentException("Only an intentional tutorial dismissal can be saved.");
  Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
  string temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
  try
  {
   File.WriteAllText(temporary, JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
   File.Move(temporary, path, true);
  }
  finally { if (File.Exists(temporary)) File.Delete(temporary); }
 }
}
