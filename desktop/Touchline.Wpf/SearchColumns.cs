using System.Globalization;
using System.IO;
using System.Text.Json;

namespace Touchline.Wpf;

public sealed record SearchColumnSpec(string Key, string Label, string Section, double Width, bool Numeric = false, string Hint = "");

/// <summary>Result columns are independent of search predicates. No game access.</summary>
public static class SearchColumns
{
 public const int MaxNumericColumns = 64;
 public static List<SearchColumnSpec> Available(string kind, IEnumerable<EditableField> fields)
 {
  var result = new List<SearchColumnSpec> { new("name", "이름", "기본 정보", 215), new("english_name", "원어 이름", "기본 정보", 180), new("id", "고유 번호 (UID)", "기본 정보", 105) };
  if (kind is "Player" or "Staff" or "Person")
  {
   result.AddRange([new("nationality", "국적", "기본 정보", 105, Hint:"주 국적 · 소속 구단의 국가와 별개입니다"), new("age", "나이", "기본 정보", 55), new("club", "소속 구단", "기본 정보", 175), new("role", kind == "Player" ? "포지션" : "현재 직무", "기본 정보", 140), new("group", "선수단 / 소속", "기본 정보", 110)]);
  }
  else result.AddRange([new("group", kind == "City" ? "국가" : "분류", "기본 정보", 130), new("role", "종류", "기본 정보", 100)]);
  if (kind is "Player" or "Staff") result.AddRange([new("ca", "CA", "기본 정보", 55), new("pa", "PA", "기본 정보", 55)]);
  if (kind is "Player" or "Staff" or "Club") result.Add(new("football_category", "축구 구분", "기본 정보", 95));
  var seen = result.Select(c => c.Key).ToHashSet(StringComparer.Ordinal);
  foreach (var f in fields.OrderBy(f => f.Section).ThenBy(f => f.Label))
   if (seen.Add(f.Key)) result.Add(new(f.Key, Label(f), string.IsNullOrWhiteSpace(f.Section) ? "기타 정보" : f.Section, f.Key.StartsWith("score_") ? 145 : IsMoney(f.Key) ? 135 : IsDate(f.Key) ? 115 : 100, true, f.Hint));
  return result;
 }
 private static string Label(EditableField f) => f.Key switch { "contract.weekly_wage" => "주급 (£/주)", "contract.expires" => "계약 만료일", "person.dob" => "생년월일", "player.market_value" => "시장 가치 (£)", "player.asking_price" => "요구 이적료 (£)", _ => f.Label };
 public static List<string> Default(string kind) => kind switch
 {
  "Player" => ["name", "nationality", "age", "role", "club", "group", "ca", "pa", "football_category"],
  "Staff" => ["name", "nationality", "age", "role", "club", "group", "ca", "pa", "football_category"],
  "Person" => ["name", "nationality", "age", "role", "club"],
  _ => ["name", "group", "role", "football_category"]
 };
 public static List<string> Normalize(IEnumerable<string> keys, IReadOnlyCollection<SearchColumnSpec> available)
 {
  var allowed = available.Select(c => c.Key).ToHashSet(StringComparer.Ordinal);
  var result = new List<string> { "name" }; var seen = new HashSet<string>(StringComparer.Ordinal) { "name" };
  var numeric = available.Where(c => c.Numeric).Select(c => c.Key).ToHashSet(StringComparer.Ordinal); int count = 0;
  foreach (string key in keys)
   if (allowed.Contains(key) && seen.Add(key) && (!numeric.Contains(key) || count < MaxNumericColumns)) { result.Add(key); if (numeric.Contains(key)) count++; }
  return result;
 }
 public static bool IsMoney(string key) => key.Contains("wage") || key.Contains("budget") || key.Contains("balance") && key.StartsWith("club.") || key is "player.market_value" or "player.asking_price" or "club.cash" or "club.transfer_base" or "club.transfer_current" || key.StartsWith("sponsor.") && (key.EndsWith(".annual") || key.EndsWith(".total"));
 public static bool IsDate(string key) => key is "person.dob" or "contract.expires";
 public static string Format(string key, object? value)
 {
  if (value == null || value is not double number || !double.IsFinite(number)) return "—";
  if (IsDate(key))
  {
   if (number != Math.Truncate(number)) return "미확인";
   string raw = number.ToString("0", CultureInfo.InvariantCulture);
   return DateTime.TryParseExact(raw, "yyyyMMdd", CultureInfo.InvariantCulture, DateTimeStyles.None, out var date) ? date.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) : "미확인";
  }
  if (IsMoney(key)) return number < 0 && key.StartsWith("player.") ? "미설정" : "£" + number.ToString("N0", CultureInfo.CurrentCulture);
  if (key is "player.injured" or "retire_after_current_club" || key.StartsWith("trait_") || key is "contract.listed" or "contract.listed_by_request" or "contract.not_for_sale" or "contract.set_for_release") return number == 0 ? "아니오" : "예";
  return number.ToString("0.##", CultureInfo.CurrentCulture);
 }
}

public sealed class SearchColumnLayout
{
 public string View { get; set; } = "default";
 public List<string> Keys { get; set; } = [];
 public Dictionary<string, double> Widths { get; set; } = [];
 public SearchColumnLayout Copy() => new() { View = View, Keys = [..Keys], Widths = new(Widths) };
}

public sealed class SearchColumnSettings(string path)
{
 public string Path { get; } = path;
 public string? LoadError { get; private set; }
 private Dictionary<string, SearchColumnLayout>? _layouts;
 private Dictionary<string, SearchColumnLayout> Load()
 {
  if (_layouts != null) return _layouts;
  try
  {
   if (File.Exists(Path) && new FileInfo(Path).Length > 512 * 1024) throw new InvalidDataException("열 설정 파일이 너무 큽니다.");
   _layouts = File.Exists(Path) ? JsonSerializer.Deserialize<Dictionary<string, SearchColumnLayout>>(File.ReadAllText(Path), JsonDefaults.Options) ?? [] : [];
  }
  catch (Exception ex) when (ex is IOException or InvalidDataException or JsonException or UnauthorizedAccessException) { LoadError = ex.Message; _layouts = []; }
  return _layouts;
 }
 public SearchColumnLayout Get(string kind) => Load().TryGetValue(kind, out var value) && value != null ? new() { View = value.View ?? "default", Keys = [..value.Keys ?? []], Widths = (value.Widths ?? []).Where(kv => double.IsFinite(kv.Value) && kv.Value >= 48 && kv.Value <= 800).ToDictionary() } : new();
 public void Save(string kind, SearchColumnLayout layout)
 {
  var data = new Dictionary<string, SearchColumnLayout>(Load()) { [kind] = layout.Copy() };
  Directory.CreateDirectory(System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(Path))!);
  string temporary = Path + ".tmp";
  File.WriteAllText(temporary, JsonSerializer.Serialize(data, new JsonSerializerOptions(JsonDefaults.Options) { WriteIndented = true }));
  File.Move(temporary, Path, true); _layouts = data;
 }
}
