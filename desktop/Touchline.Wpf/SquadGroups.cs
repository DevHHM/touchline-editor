using System.Text.RegularExpressions;

namespace Touchline.Wpf;

/// <summary>Groups explicit roster labels; never infers squad membership from age or job.</summary>
internal static class SquadGroups
{
 internal const string All = "*";
 internal const string Unknown = "소속팀 미확인";
 internal const string Loan = "임대 중";

 internal static string Key(string? raw)
 {
  string value = (raw ?? "").Trim();
  if (Regex.IsMatch(value, @"^임대\s*중(?:\s*·|$)")) return Loan;
  // Incoming loans still belong to their recorded squad. Only strip this exact metadata suffix.
  value = Regex.Replace(value, @"\s*·\s*임대\s*영입\s*$", "").Trim();
  if (value.Length == 0 || value is "소속팀 미확인" or "미확인" or "—" || value.Equals("Unknown", StringComparison.OrdinalIgnoreCase)) return Unknown;
  if (value.Equals("First", StringComparison.OrdinalIgnoreCase) || value.Equals("First Team", StringComparison.OrdinalIgnoreCase)) return "1군";
  if (Regex.IsMatch(value, @"^U\s*-?\s*\d{1,2}$", RegexOptions.IgnoreCase)) return "U" + Regex.Match(value, @"\d{1,2}$").Value;
  return value;
 }

 internal static IReadOnlyList<SquadGroupOption> Options(IEnumerable<string?> groups)
 {
  var counts = groups.Select(Key).GroupBy(value => value, StringComparer.Ordinal).Select(group => new SquadGroupOption(group.Key, Regex.IsMatch(group.Key, @"^U\d{1,2}$") ? "U-" + group.Key[1..] : group.Key, group.Count())).ToList();
  counts.Sort((a, b) => { int order = Rank(a.Key).CompareTo(Rank(b.Key)); return order != 0 ? order : StringComparer.CurrentCulture.Compare(a.Label, b.Label); });
  counts.Insert(0, new SquadGroupOption(All, "전체", counts.Sum(group => group.Count)));
  return counts;
 }

 internal static string DefaultKey(IEnumerable<SquadGroupOption> options, string? preferred = null)
 {
  var keys = options.Select(group => group.Key).ToHashSet(StringComparer.Ordinal);
  return preferred != null && keys.Contains(preferred) ? preferred : keys.Contains("1군") ? "1군" : All;
 }

 private static int Rank(string key)
 {
  if (key == "1군") return 0;
  if (Regex.IsMatch(key, @"^U\d{1,2}$")) return 100 - int.Parse(key[1..]);
  if (key == "2군" || key.Equals("Reserve", StringComparison.OrdinalIgnoreCase) || key.Equals("Reserves", StringComparison.OrdinalIgnoreCase)) return 110;
  if (key == Unknown) return 998;
  if (key == Loan) return 999;
  return 200;
 }
}

internal sealed record SquadGroupOption(string Key, string Label, int Count)
{
 public string Display => $"{Label}  {Count:N0}";
}
