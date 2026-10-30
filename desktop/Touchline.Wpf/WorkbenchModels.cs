using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public sealed record WorkbenchRef([property: JsonPropertyName("kind")] string Kind, [property: JsonPropertyName("id")] uint Id);
public sealed record WorkbenchRule([property: JsonPropertyName("key")] string Key, [property: JsonPropertyName("operation")] string Operation, [property: JsonPropertyName("value")] double Value);
public sealed class WorkbenchShortlist
{
 [JsonPropertyName("name")] public string Name { get; set; } = "";
 [JsonPropertyName("members")] public List<WorkbenchRef> Members { get; set; } = [];
}
public sealed class WorkbenchPreset
{
 [JsonPropertyName("name")] public string Name { get; set; } = "";
 [JsonPropertyName("kind")] public string Kind { get; set; } = "Player";
 [JsonPropertyName("rules")] public List<WorkbenchRule> Rules { get; set; } = [];
}
public sealed class WorkbenchLibrary
{
 [JsonPropertyName("lists")] public List<WorkbenchShortlist> Lists { get; set; } = [];
 [JsonPropertyName("presets")] public List<WorkbenchPreset> Presets { get; set; } = [];
}
public sealed class WorkbenchDraft : INotifyPropertyChanged
{
 private string _key = "", _operation = "set", _value = "", _minimum = "", _maximum = "";
 public string Key { get => _key; set => Set(ref _key, value); }
 public string Operation { get => _operation; set => Set(ref _operation, value); }
 public string Value { get => _value; set => Set(ref _value, value); }
 public string Minimum { get => _minimum; set => Set(ref _minimum, value); }
 public string Maximum { get => _maximum; set => Set(ref _maximum, value); }
 public event PropertyChangedEventHandler? PropertyChanged;
 private void Set(ref string field, string value, [CallerMemberName] string? name = null) { if (field == value) return; field = value; PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name)); }
}
public sealed record WorkbenchChange(string Name, string Kind, uint Id, string Field, string Before, string After);
public sealed record WorkbenchHistory(string Id, string Time, string Status, int Targets, int Changes);
