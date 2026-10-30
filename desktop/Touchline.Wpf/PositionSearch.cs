using System.Globalization;

namespace Touchline.Wpf;

/// <summary>One position predicate: any selected, catalog-verified proficiency meets the minimum.</summary>
public static class PositionSearch
{
 public const string Source = "position", Key = "any";
 public static readonly string[] Codes = ["GK", "DL", "DC", "DR", "WBL", "DM", "WBR", "ML", "MC", "MR", "AML", "AMC", "AMR", "ST"];
 public static readonly SearchChoice[] Proficiencies = Enumerable.Range(1,20).Reverse().Select(n => new SearchChoice(n.ToString(CultureInfo.InvariantCulture), n == 20 ? "자연스러움 (20)" : $"숙련도 {n} 이상")).ToArray();
 public static bool IsPosition(SearchConditionRule rule) => rule.Source == Source && rule.Key == Key;
 public static string FieldKey(string code) => "position_" + code.ToLowerInvariant();
 public static string Code(string key) => key.StartsWith("position_",StringComparison.Ordinal) ? key[9..].ToUpperInvariant() : key;
 public static bool IsKey(string key) => Codes.Any(code => FieldKey(code) == key);
 public static List<string> Available(IEnumerable<SearchConditionDefinition> catalog) => Codes.Select(FieldKey).Where(key => catalog.Any(d => d.Source == "field" && d.Key == key && d.Type == "number" && d.Min <= 1 && d.Max >= 20)).ToList();
 public static SearchConditionRule NewRule(IEnumerable<SearchConditionDefinition> catalog) => new() { Source=Source, Key=Key, Operator="gte", Value="20", Positions=Available(catalog).Take(1).ToList() };
 public static SearchConditionRequest Compile(SearchConditionRule rule,IReadOnlyList<SearchConditionDefinition> catalog,string join)
 {
  if(!IsPosition(rule)||rule.Operator!="gte"||!int.TryParse(rule.Value,NumberStyles.None,CultureInfo.InvariantCulture,out int minimum)||minimum<1||minimum>20||rule.Value2.Length!=0)
   throw new InvalidOperationException("포지션 조건은 숙련도 1–20의 최소값을 선택하세요.");
  if(!catalog.Any(d=>d.Source==Source&&d.Key==Key)||rule.Positions==null||rule.Positions.Count is <1 or >14||rule.Positions.Distinct(StringComparer.Ordinal).Count()!=rule.Positions.Count)
   throw new InvalidOperationException("포지션을 하나 이상 선택하세요. 중복된 포지션은 사용할 수 없습니다.");
  var available=Available(catalog);
  if(rule.Positions.Any(key=>!IsKey(key)||!available.Contains(key)))throw new InvalidOperationException("현재 자료에서 지원하지 않는 포지션입니다. 선택을 다시 확인하세요.");
  return new(true,join,Source,Key,"gte",minimum,positions:Codes.Select(FieldKey).Where(rule.Positions.Contains).ToArray());
 }
 public static string Describe(SearchConditionRule rule) => $"{(rule.Enabled?"":"꺼짐 · ")}포지션 {string.Join(" / ",rule.Positions.Select(Code))} · {(rule.Value=="20"?"자연스러움 (20)":"숙련도 "+rule.Value+" 이상")} · 하나 이상";
}
