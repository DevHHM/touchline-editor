namespace Touchline.Wpf;

public sealed record SearchReloadState(string Kind, SearchCriteria Criteria, string SortKey, bool Descending);

public sealed record SearchChoice(string Key, string Label);

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

 public SearchCriteria Clone() => new()
 {
  Name = Name, Club = Club, Scope = Scope, FootballCategory = FootballCategory, Role = Role, Employment = Employment, Group = Group,
  AgeMin = AgeMin, AgeMax = AgeMax, CaMin = CaMin, CaMax = CaMax, PaMin = PaMin, PaMax = PaMax,
  Conditions = Conditions.Select(condition => condition.Clone()).ToList()
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
