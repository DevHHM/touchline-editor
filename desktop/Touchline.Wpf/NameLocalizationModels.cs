using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public sealed class NameLocalizationRow : INotifyPropertyChanged
{
 [JsonPropertyName("id")] public uint Id { get; set; }
 [JsonPropertyName("name")] public string Name { get; set; } = "";
 [JsonPropertyName("english_name")] public string EnglishName { get; set; } = "";
 [JsonPropertyName("first_name")] public string FirstName { get; set; } = "";
 [JsonPropertyName("last_name")] public string LastName { get; set; } = "";
 [JsonPropertyName("common_name")] public string CommonName { get; set; } = "";
 [JsonPropertyName("installed_name")] public string? InstalledName { get; set; }
 [JsonPropertyName("suggested_name")] public string? SuggestedName { get; set; }
 [JsonPropertyName("suggestion_source")] public string? SuggestionSource { get; set; }
 [JsonPropertyName("error")] public string? Error { get; set; }
 private bool _selected;
 private string _koreanName = "";
 [JsonIgnore] public bool Selected { get => _selected; set { if (_selected == value) return; _selected = value; Changed(); } }
 [JsonIgnore] public string KoreanName { get => _koreanName; set { value ??= ""; if (_koreanName == value) return; _koreanName = value; Changed(); } }
 [JsonIgnore] public bool Editable => string.IsNullOrWhiteSpace(Error);
 [JsonIgnore] public string OriginalParts => $"이름: {FirstName}\n성: {LastName}\n통용명: {(CommonName.Length == 0 ? "없음" : CommonName)}";
 [JsonIgnore] public string BaselineName => InstalledName ?? Name;
 [JsonIgnore] public string SourceLabel => !string.IsNullOrWhiteSpace(Error) ? Error : SuggestionSource switch
 {
  "current_hangul" or "current_name" or "common_hangul" or "current_korean" => "현재 한글 이름",
  "existing_lnc" or "lnc" => "기존 한글화 파일",
  "touchline" or "installed" => "Touchline 설치 이름",
  null or "" or "none" => "직접 입력",
  _ => SuggestionSource
 };
 public event PropertyChangedEventHandler? PropertyChanged;
 private void Changed([CallerMemberName] string? name = null) => PropertyChanged?.Invoke(this, new(name));
}

public sealed class NameLocalizationTarget { [JsonPropertyName("path")] public string Path { get; set; } = ""; }
public sealed class NameLocalizationContext
{
 [JsonPropertyName("live")] public bool Live { get; set; }
 [JsonPropertyName("can_install")] public bool CanInstall { get; set; }
 [JsonPropertyName("reason")] public string Reason { get; set; } = "";
 [JsonPropertyName("rows")] public List<NameLocalizationRow> Rows { get; set; } = [];
 [JsonPropertyName("targets")] public List<NameLocalizationTarget> Targets { get; set; } = [];
 [JsonPropertyName("restart_required")] public bool RestartRequired { get; set; }
}
public sealed class NameLocalizationChange
{
 [JsonPropertyName("id")] public uint Id { get; set; }
 [JsonPropertyName("name")] public string Name { get; set; } = "";
 [JsonPropertyName("before")] public string Before { get; set; } = "";
 [JsonPropertyName("after")] public string After { get; set; } = "";
 [JsonPropertyName("first_name")] public string FirstName { get; set; } = "";
 [JsonPropertyName("last_name")] public string LastName { get; set; } = "";
}
public sealed class NameLocalizationConflict
{
 [JsonPropertyName("id")] public uint Id { get; set; }
 [JsonPropertyName("line")] public int Line { get; set; }
 [JsonPropertyName("old_name")] public string OldName { get; set; } = "";
}
public sealed class NameLocalizationFile
{
 [JsonPropertyName("path")] public string Path { get; set; } = "";
 [JsonPropertyName("operation")] public string Operation { get; set; } = "";
 [JsonPropertyName("conflicts")] public List<NameLocalizationConflict> Conflicts { get; set; } = [];
 [JsonIgnore] public string Description => $"{(Operation switch { "create" => "생성", "update" => "수정", "remove_conflict" => "기존 표기 충돌 정리", "restore" => "이전 파일 복원", "remove" => "생성 파일 제거", _ => Operation })} · {Path}" + (Conflicts.Count == 0 ? "" : "\n" + string.Join("\n", Conflicts.Select(item => $"  UID {item.Id} · {item.Line}행 · 기존 표기 {item.OldName}")));
}
public sealed class NameLocalizationPreview
{
 [JsonPropertyName("token")] public string Token { get; set; } = "";
 [JsonPropertyName("can_apply")] public bool CanApply { get; set; }
 [JsonPropertyName("entries")] public List<NameLocalizationChange> Entries { get; set; } = [];
 [JsonPropertyName("files")] public List<NameLocalizationFile> Files { get; set; } = [];
 [JsonPropertyName("warnings")] public List<string> Warnings { get; set; } = [];
 [JsonPropertyName("restart_required")] public bool RestartRequired { get; set; }
}
public sealed class NameLocalizationReceipt
{
 [JsonPropertyName("id")] public string Id { get; set; } = "";
 [JsonPropertyName("time")] public string Time { get; set; } = "";
 [JsonPropertyName("status")] public string Status { get; set; } = "";
 [JsonPropertyName("entries")] public List<NameLocalizationChange> Entries { get; set; } = [];
 [JsonPropertyName("files")] public List<NameLocalizationFile> Files { get; set; } = [];
 [JsonPropertyName("can_undo")] public bool CanUndo { get; set; }
 [JsonPropertyName("undo_of")] public string? UndoOf { get; set; }
 [JsonPropertyName("message")] public string Message { get; set; } = "";
 [JsonIgnore] public int Count => Entries.Count;
 [JsonIgnore] public string StatusLabel => !string.IsNullOrWhiteSpace(UndoOf) && Status is "applied" or "undone" ? "되돌림" : Status switch { "applied" or "installed" => "설치됨", "undone" => "되돌림", "failed" => "실패", "failed_rolled_back" => "실패 · 복구됨", "failed_rollback_incomplete" => "실패 · 복구 확인 필요", "prepared" => "적용 전 백업", _ => Status };
 [JsonIgnore] public string TimeLabel
 {
  get
  {
   try { if (long.TryParse(Time, out long milliseconds)) return DateTimeOffset.FromUnixTimeMilliseconds(milliseconds).LocalDateTime.ToString("yyyy-MM-dd HH:mm:ss"); }
   catch (ArgumentOutOfRangeException) { }
   return DateTimeOffset.TryParse(Time, out var date) ? date.LocalDateTime.ToString("yyyy-MM-dd HH:mm:ss") : Time;
  }
 }
 [JsonIgnore] public string Detail => string.Join("\n", Entries.Select(entry => $"{entry.Name} · {entry.Before} → {entry.After}")) + "\n\n" + string.Join("\n", Files.Select(file => file.Description)) + "\n" + Message;
}
