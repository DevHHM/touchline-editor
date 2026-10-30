using System.IO;
using System.Text.Json;

namespace Touchline.Wpf;

public sealed record UpdatePreferences
{
 public bool CheckAtStartup { get; init; } = true;
 public bool IncludePreview { get; init; }
 public DateTimeOffset? DeferUntil { get; init; }
 public static string DataDirectory => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "Updates");
 public static string SettingsPath => Path.Combine(DataDirectory, "settings.json");
 public static string ResultPath => Path.Combine(DataDirectory, "last-result.json");
 public static UpdatePreferences Load()
 {
  try { return File.Exists(SettingsPath) ? JsonSerializer.Deserialize<UpdatePreferences>(File.ReadAllText(SettingsPath)) ?? new() : new(); }
  catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException) { return new(); }
 }
 public void Save()
 {
  Directory.CreateDirectory(DataDirectory);
  string temporary = SettingsPath + ".tmp";
  File.WriteAllText(temporary, JsonSerializer.Serialize(this)); File.Move(temporary, SettingsPath, true);
 }
}
