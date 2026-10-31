using System.IO;
using System.Text.Json;

namespace Touchline.Wpf;

public sealed record UiScalePreferences
{
 public static IReadOnlyList<int> Presets { get; } = Array.AsReadOnly(new[] { 80, 90, 100, 110, 125, 150 });
 public int Version { get; init; } = 1;
 public int Percent { get; init; } = 100;
 public bool IsValid => Version == 1 && Presets.Contains(Percent);
 public static int Step(int percent, int direction) => direction > 0
  ? Presets.FirstOrDefault(value => value > percent, Presets[^1])
  : Presets.LastOrDefault(value => value < percent, Presets[0]);

 public static UiScalePreferences Load(string path)
 {
  try
  {
   if (File.Exists(path) && JsonSerializer.Deserialize<UiScalePreferences>(File.ReadAllText(path)) is { IsValid: true } saved) return saved;
  }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException) { }
  return new();
 }

 public void Save(string path)
 {
  if (!IsValid) throw new ArgumentException("Invalid UI scale preference.");
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
