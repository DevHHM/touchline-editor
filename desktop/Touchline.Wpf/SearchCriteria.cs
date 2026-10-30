using System.Text.Json;

namespace Touchline.Wpf;

public sealed record SearchReloadState(string Kind, SearchCriteria Criteria, string SortKey, bool Descending);

public sealed record SearchChoice(string Key, string Label);

public sealed record SearchNationChoice(uint? Id, string Name, int? Count = null)
{
 public string Label => Count.HasValue ? $"{Name} ({Count:N0})" : Name;
}

/// <summary>Primary-nationality identities, independent of translated display names or club country.</summary>
public static class SearchNationalityChoices
{
 public const string AllLabel = "모든 국적";
 public const string Hint = "선수·스태프의 주 국적으로 검색합니다. 소속 구단 국가나 추가 국적은 포함하지 않습니다. 국가명을 입력하거나 목록에서 선택하세요.";
 public static bool Supports(string kind) => kind is "Player" or "Staff" or "Person";
 public static string DisplayName(uint id, string? name) => string.IsNullOrWhiteSpace(name) ? $"국적 ID {id}" : name.Trim();
 public static List<SearchNationChoice> FromFacets(JsonElement facets)
 {
  if (!facets.TryGetProperty("nations", out var nations) || nations.ValueKind != JsonValueKind.Array) return [];
  var result = new List<SearchNationChoice>();
  foreach (var nation in nations.EnumerateArray())
  {
   if (nation.ValueKind != JsonValueKind.Object || !nation.TryGetProperty("id", out var idValue) || idValue.ValueKind != JsonValueKind.Number || !idValue.TryGetUInt32(out var id) || id == 0 || !nation.TryGetProperty("name", out var nameValue) || nameValue.ValueKind != JsonValueKind.String || string.IsNullOrWhiteSpace(nameValue.GetString())) continue;
   int? count = nation.TryGetProperty("count", out var countValue) && countValue.ValueKind == JsonValueKind.Number && countValue.TryGetInt32(out var parsed) && parsed >= 0 ? parsed : null;
   result.Add(new(id, nameValue.GetString()!.Trim(), count));
  }
  return Build(result).Where(choice => choice.Id.HasValue).ToList();
 }
 public static List<SearchNationChoice> Build(IEnumerable<SearchNationChoice> nations, uint? selectedId = null, string? selectedName = null)
 {
  var choices = nations.Where(choice => choice.Id is > 0 && !string.IsNullOrWhiteSpace(choice.Name)).DistinctBy(choice => choice.Id).ToList();
  if (selectedId is > 0 && !choices.Any(choice => choice.Id == selectedId)) choices.Add(new(selectedId, DisplayName(selectedId.Value, selectedName)));
  return [new(null, AllLabel), .. choices.OrderBy(choice => choice.Name, StringComparer.CurrentCultureIgnoreCase).ThenBy(choice => choice.Id)];
 }
 public static SearchNationChoice Resolve(IEnumerable<SearchNationChoice> choices, SearchNationChoice? selected, string text)
 {
  string value = text.Trim();
  if (value.Length == 0 || value == AllLabel) return new(null, AllLabel);
  if (selected != null && (string.Equals(selected.Name, value, StringComparison.CurrentCultureIgnoreCase) || string.Equals(selected.Label, value, StringComparison.CurrentCultureIgnoreCase))) return selected;
  var matches = choices.Where(choice => string.Equals(choice.Name, value, StringComparison.CurrentCultureIgnoreCase) || string.Equals(choice.Label, value, StringComparison.CurrentCultureIgnoreCase)).ToArray();
  if (matches.Length == 1) return matches[0];
  throw new InvalidOperationException(matches.Length > 1 ? "같은 이름의 국가가 여러 개입니다. 주 국적 목록에서 대상을 선택하세요." : "입력한 주 국적을 찾을 수 없습니다. 목록에서 선택하거나 ‘모든 국적’으로 바꿔주세요.");
 }
 public static void Validate(string kind, uint? id, string? name)
 {
  if (id == 0 || id.HasValue && !Supports(kind)) throw new InvalidOperationException("현재 검색에서 사용할 수 없는 주 국적입니다.");
  if (name != null && name.Length > 200) throw new InvalidOperationException("주 국적 이름은 200자 이내여야 합니다.");
 }
}

/// <summary>
/// Filter choices use the same football order as result rows. Keep the original
/// key and label (including counts), and retain a saved selection even when the
/// current facet list does not contain it.
/// </summary>
public static class SearchRoleChoices
{
 public static List<SearchChoice> Build(string kind, IEnumerable<SearchChoice> choices, string selected = "", string all = "모든 직무 / 포지션")
 {
  var distinct = choices.DistinctBy(choice => choice.Key, StringComparer.Ordinal).ToList();
  var allChoice = distinct.FirstOrDefault(choice => choice.Key.Length == 0) ?? new SearchChoice("", all);
  var roles = distinct.Where(choice => choice.Key.Length > 0).ToList();
  if (selected.Length > 0 && !roles.Any(choice => choice.Key == selected)) roles.Add(new(selected, selected));
  IEnumerable<SearchChoice> ordered = kind == "Player"
   ? roles.OrderBy(choice => PositionOrder.Rank(choice.Key)).ThenBy(choice => choice.Key, StringComparer.OrdinalIgnoreCase).ThenBy(choice => choice.Key, StringComparer.Ordinal)
   : roles.OrderBy(choice => choice.Key, StringComparer.CurrentCultureIgnoreCase).ThenBy(choice => choice.Key, StringComparer.Ordinal);
  return [allChoice, .. ordered];
 }
}

public sealed class SearchNumericCondition
{
 public string Key { get; set; } = "";
 public double? Min { get; set; }
 public double? Max { get; set; }
 public SearchNumericCondition() { }
 public SearchNumericCondition(string key, double? min = null, double? max = null) { Key = key; Min = min; Max = max; }
 public SearchNumericCondition Clone() => new(Key, Min, Max);
}

/// <summary>A detached search draft. Opening or cancelling the criteria dialog never mutates its caller.</summary>
public sealed class SearchCriteria
{
 public string Name { get; set; } = "";
 public string Club { get; set; } = "";
 public string Scope { get; set; } = "all";
 public string? FootballCategory { get; set; }
 public uint? NationalityId { get; set; }
 public string NationalityName { get; set; } = "";
 public string Role { get; set; } = "";
 public string Employment { get; set; } = "all";
 public string Group { get; set; } = "";
 public string AgeMin { get; set; } = "";
 public string AgeMax { get; set; } = "";
 public string CaMin { get; set; } = "";
 public string CaMax { get; set; } = "";
 public string PaMin { get; set; } = "";
 public string PaMax { get; set; } = "";
 public List<SearchNumericCondition> Conditions { get; set; } = [];
 public List<SearchConditionRule> Rules { get; set; } = [];

 public SearchCriteria Clone() => new()
 {
  Name = Name, Club = Club, Scope = Scope, FootballCategory = FootballCategory, NationalityId = NationalityId, NationalityName = NationalityName, Role = Role, Employment = Employment, Group = Group,
  AgeMin = AgeMin, AgeMax = AgeMax, CaMin = CaMin, CaMax = CaMax, PaMin = PaMin, PaMax = PaMax,
  Conditions = Conditions.Select(condition => condition.Clone()).ToList(), Rules = Rules.Select(rule => rule.Clone()).ToList()
 };
}

internal static class FootballSearchCategories
{
 internal const string Hint = "스태프는 본인 성별이 아니라 소속 구단의 축구 구분을 따릅니다. 미확인 대상과 구분을 알 수 없는 무소속 스태프는 전체에서 확인하세요.";
 internal static readonly SearchChoice[] Choices = [new("", "전체"), new("men", "남자축구"), new("women", "여자축구")];
 internal static bool Supports(string kind) => kind is "Player" or "Staff" or "Club";
 internal static bool IsValid(string? value) => value is null or "men" or "women";
 internal static string Label(string? value) => value switch { "men" => "남자축구", "women" => "여자축구", _ => "전체" };
}
