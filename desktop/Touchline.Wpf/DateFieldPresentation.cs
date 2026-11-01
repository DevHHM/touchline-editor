using System.Globalization;

namespace Touchline.Wpf;

/// <summary>Calendar presentation for date fields actually supplied by the backend.</summary>
public static class DateFieldPresentation
{
 public static bool IsDate(string? key) => key is "person.dob" or "contract.starts" or "contract.expires";
 public static bool IsContractDate(string? key) => key is "contract.starts" or "contract.expires";
 public static string Label(string key) => key switch { "person.dob" => "생년월일", "contract.starts" => "계약 시작일", "contract.expires" => "계약 만료일", _ => key };
 public static bool TryParse(string? value, out DateTime date) => DateTime.TryParseExact(value, "yyyyMMdd", CultureInfo.InvariantCulture, DateTimeStyles.None, out date);
 public static bool TryParseInput(string? value, CultureInfo culture, out DateTime date)
 {
  if (DateTime.TryParseExact(value, ["yyyy-MM-dd", "yyyyMMdd"], CultureInfo.InvariantCulture, DateTimeStyles.AllowWhiteSpaces, out date)) return true;
  foreach (var formatCulture in new[] { culture, CultureInfo.GetCultureInfo(culture.Name) })
  {
   string pattern = System.Text.RegularExpressions.Regex.Replace(formatCulture.DateTimeFormat.ShortDatePattern, "y+", "yyyy");
   if (DateTime.TryParseExact(value, pattern, formatCulture, DateTimeStyles.AllowWhiteSpaces, out date)) return true;
  }
  date = default; return false;
 }
 public static bool TryParse(double value, out DateTime date)
 {
  date = default;
  return double.IsFinite(value) && value == Math.Truncate(value) && TryParse(value.ToString("0", CultureInfo.InvariantCulture), out date);
 }
 public static string Format(string? value) => TryParse(value, out var date) ? date.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) : "확인되지 않음";
 public static string FormatChange(string? key, object? value) => IsDate(key) ? Format(Convert.ToString(value, CultureInfo.InvariantCulture)) : AbilityVisibility.Current.Format(key, value);
 public static string? PeriodError(IEnumerable<EditableField> fields)
 {
  var start = fields.FirstOrDefault(field => field.Key == "contract.starts");
  var end = fields.FirstOrDefault(field => field.Key == "contract.expires");
  if (start == null || end == null || !(start.IsDirty || end.IsDirty) || !start.IsValid || !end.IsValid) return null;
  return TryParse(start.Value, out var startDate) && TryParse(end.Value, out var endDate) && startDate > endDate ? "계약 시작일은 만료일보다 늦을 수 없습니다." : null;
 }
 public static IReadOnlyList<string> ContractMessages(DetailRecord detail)
 {
  var messages = detail.Notes.Where(note => note.Contains("계약", StringComparison.Ordinal)).Distinct().ToList();
  bool start = detail.Fields.Any(field => field.Key == "contract.starts"), end = detail.Fields.Any(field => field.Key == "contract.expires");
  if (messages.Count == 0 && (!start || !end)) messages.Add(start || end ? "확인되지 않은 계약 날짜는 표시하거나 편집할 수 없습니다." : "현재 자료에서 계약 날짜를 확인할 수 없습니다. 계약 날짜가 지원되는 연결과 유효한 계약이 필요합니다.");
  return messages;
 }
}
