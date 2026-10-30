using System.Globalization;
using System.IO;
using System.Text.Json;

namespace Touchline.Wpf;

/// <summary>Presentation preference only. Never changes game data or edit drafts.</summary>
public sealed class AbilityVisibility
{
 public static AbilityVisibility Current { get; } = new();
 public bool HideCa { get; private set; }
 public bool HidePa { get; private set; }
 public bool AnyHidden => HideCa || HidePa;
 public event EventHandler? Changed;

 public static string? AbilityKey(string? key) => key?.Trim().ToLowerInvariant() switch
 {
  "ca" or "player.ca" or "staff.ca" or "current_ca" or "current_ability" => "ca",
  "pa" or "player.pa" or "staff.pa" or "potential_ca" or "potential_ability" => "pa",
  _ => null
 };
 public bool IsHidden(string? key) => AbilityKey(key) switch { "ca" => HideCa, "pa" => HidePa, _ => false };
 public string Format(string? key, object? value) => IsHidden(key) ? "숨김" : Convert.ToString(value, CultureInfo.InvariantCulture) is { Length: > 0 } text ? text : "—";
 public void Set(bool hideCa, bool hidePa)
 {
  if (HideCa == hideCa && HidePa == hidePa) return;
  HideCa = hideCa; HidePa = hidePa;
  Changed?.Invoke(this, EventArgs.Empty);
 }
 public static string DefaultPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "ability-visibility.json");
 public void Load(string path)
 {
  // Missing/corrupt preferences leave the current display intact.
  if (!File.Exists(path)) return;
  try
  {
   var saved = JsonSerializer.Deserialize<Preferences>(File.ReadAllText(path));
   if (saved?.Version == 1) Set(saved.HideCa, saved.HidePa);
  }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException) { }
 }
 public void Save(string path)
 {
  string fullPath = Path.GetFullPath(path);
  Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
  string temporary = fullPath + "." + Guid.NewGuid().ToString("N") + ".tmp";
  try
  {
   File.WriteAllText(temporary, JsonSerializer.Serialize(new Preferences(1, HideCa, HidePa), new JsonSerializerOptions { WriteIndented = true }));
   File.Move(temporary, fullPath, true);
  }
  finally { if (File.Exists(temporary)) File.Delete(temporary); }
 }
 private sealed record Preferences(int Version, bool HideCa, bool HidePa);
}
