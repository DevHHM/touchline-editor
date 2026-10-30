using System.Globalization;

namespace Touchline.Wpf;

/// <summary>A saved, ordered search expression. Values are drafts until catalog validation.</summary>
public sealed class SearchConditionRule
{
 public bool Enabled { get; set; } = true;
 public string Join { get; set; } = "and";
 public string Source { get; set; } = "field";
 public string Key { get; set; } = "";
 public string Operator { get; set; } = "gte";
 public string Value { get; set; } = "";
 public string Value2 { get; set; } = "";
 public List<string> Positions { get; set; } = [];
 public SearchConditionRule Clone() => new() { Enabled = Enabled, Join = Join, Source = Source, Key = Key, Operator = Operator, Value = Value, Value2 = Value2, Positions = Positions?.ToList() ?? [] };
}

public sealed record SearchConditionOption(string Value, string Label);
public sealed record SearchConditionDefinition(string Source, string Key, string Label, string Category, string Type,
 double Min = 0, double Max = 0, double Step = 1, IReadOnlyList<SearchConditionOption>? Options = null)
{
 public string Identity => Source + ":" + Key;
 public string RangeLabel => Type == "number" ? $"{Min:G}–{Max:G}" : Type == "reference" ? "목록에서 선택" : Type == "text" ? "문자열" : "선택";
}

public sealed record SearchConditionRequest(bool enabled, string join, string source, string key, string op, object value, object? value2 = null,
 [property: System.Text.Json.Serialization.JsonIgnore(Condition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] IReadOnlyList<string>? positions = null);

/// <summary>Only index metadata and actual field_catalog keys are exposed. No inferred game fields.</summary>
public static class SearchConditionCatalog
{
 public const int Limit = 64;
 public const string LogicHint = "‘그리고’ 조건끼리 먼저 묶고, ‘또는’으로 묶음 중 하나를 선택합니다. 기본 조건·빠른 능력치는 전체 식과 함께 적용됩니다.";
 public static readonly SearchChoice[] NumberOperators = [new("gte", "이상 ≥"), new("lte", "이하 ≤"), new("eq", "같음 ="), new("ne", "다름 ≠"), new("gt", "초과 >"), new("lt", "미만 <"), new("between", "범위")];
 public static readonly SearchChoice[] TextOperators = [new("contains", "포함"), new("not_contains", "포함 안 함"), new("eq", "같음"), new("ne", "다름")];
 public static readonly SearchChoice[] ChoiceOperators = [new("eq", "같음"), new("ne", "다름")];
 public static IReadOnlyList<SearchChoice> Operators(SearchConditionDefinition definition) => definition.Type == "number" ? NumberOperators : definition.Type == "text" ? TextOperators : ChoiceOperators;

 public static List<EditableField> MergeFields(IEnumerable<EditableField> sampleFields, IEnumerable<EditableField> columnFields)
 {
  var result = sampleFields.ToList(); var keys = result.Select(f => f.Key).ToHashSet(StringComparer.Ordinal);
  foreach (var field in columnFields) {
   if (string.IsNullOrWhiteSpace(field.Key) || !double.IsFinite(field.Min) || !double.IsFinite(field.Max) || field.Min > field.Max || !double.IsFinite(field.Step) || field.Step <= 0 || !keys.Add(field.Key)) continue;
   // column_fields carries verified searchable metadata even when the sample has no contract.
   // It is not a sampled value or an editable game field.
   result.Add(new EditableField { Key=field.Key,Label=field.Label,Section=field.Section,Hint=field.Hint,Min=field.Min,Max=field.Max,Step=field.Step,Editable=false,Value=double.NaN });
  }
  return result;
 }

 public static List<SearchConditionDefinition> Build(string kind, IEnumerable<EditableField> fields, IEnumerable<SearchChoice>? roles = null, IEnumerable<SearchChoice>? groups = null, IEnumerable<SearchNationChoice>? nations = null)
 {
  var result = new List<SearchConditionDefinition> {
   new("entity", "name", "이름 · 영문 이름", "기본 정보", "text"), new("entity", "id", "고유 번호 (UID)", "기본 정보", "number", 1, uint.MaxValue),
   new("entity", "club", "소속 구단 이름", "소속 · 계약", "text") };
  if (kind is "Player" or "Staff" or "Person") {
   result.AddRange([new("entity", "age", "나이", "기본 정보", "number", 0, 129), new("entity", "ca", "현재 능력 (CA)", "기본 정보", "number", 0, 200), new("entity", "pa", "잠재 능력 (PA)", "기본 정보", "number", 0, 200),
    new("entity", "employed", "고용 상태", "소속 · 계약", "boolean", Options: [new("true", "소속 있음"), new("false", "무소속")]),
    new("entity", "nationality_id", "주 국적", "국적", "reference", 1, uint.MaxValue, Options: (nations ?? []).Where(n => n.Id.HasValue).Select(n => new SearchConditionOption(n.Id!.Value.ToString(CultureInfo.InvariantCulture), n.Name)).ToArray())]);
   result.Add(new("entity", "role", kind == "Player" ? "현재 포지션 표기" : "현재 직무", kind == "Player" ? "포지션" : "직무", "text"));
  }
  if (kind is "Player" or "Staff" or "City") result.Add(new("entity", "group", kind == "City" ? "소속 국가명" : "선수단 구분", kind == "City" ? "기본 정보" : "소속 · 계약", "choice", Options: (groups ?? []).Where(g => g.Key.Length > 0).Select(g => new SearchConditionOption(g.Key,g.Label)).ToArray()));
  if (kind is "Player" or "Staff" or "Club") result.Add(new("entity", "football_category", "축구 구분", "기본 정보", "choice", Options: [new("men", "남자축구"), new("women", "여자축구"), new("unknown", "미확인")]));
  foreach (var field in fields.Where(f => f.Key.Length > 0 && double.IsFinite(f.Min) && double.IsFinite(f.Max) && f.Min <= f.Max).DistinctBy(f => f.Key))
   result.Add(new("field", field.Key, field.Label, Category(field), "number", field.Min, field.Max, double.IsFinite(field.Step) && field.Step > 0 ? field.Step : 1));
  if(kind=="Player" && PositionSearch.Available(result).Count>0)result.Insert(0,new(PositionSearch.Source,PositionSearch.Key,"포지션 · 숙련도","포지션","position",1,20));
  return result;
 }
 private static string Category(EditableField field) => field.Key.StartsWith("score_") ? "직무 점수" : field.Key.StartsWith("position_") ? "포지션 숙련도" : field.Key.StartsWith("contract.") ? "계약" : string.IsNullOrWhiteSpace(field.Section) ? "상세 정보" : field.Section;
 public static SearchConditionRule NewRule(SearchConditionDefinition definition) => new() {
  Source = definition.Source, Key = definition.Key, Operator = Operators(definition)[0].Key,
  Value = definition.Type == "number" ? Math.Clamp(definition.Key.StartsWith("position_") || definition.Max == 20 ? 15 : definition.Min, definition.Min, definition.Max).ToString("G17", CultureInfo.InvariantCulture) : definition.Options?.FirstOrDefault()?.Value ?? "",
  Value2 = definition.Max.ToString("G17", CultureInfo.InvariantCulture) };
 public static SearchConditionDefinition Find(IEnumerable<SearchConditionDefinition> catalog, SearchConditionRule rule) => catalog.FirstOrDefault(d => d.Source == rule.Source && d.Key == rule.Key) ?? throw new InvalidOperationException($"현재 자료에서 지원하지 않는 검색 조건입니다: {rule.Source}:{rule.Key}. 제거하거나 사용을 해제하세요.");

 public static List<SearchConditionRequest> Compile(IEnumerable<SearchConditionRule> rules, IReadOnlyList<SearchConditionDefinition> catalog)
 {
  var rows = rules.ToArray(); if (rows.Length > Limit) throw new InvalidOperationException($"상세 조건은 최대 {Limit}개입니다.");
  var result = new List<SearchConditionRequest>();
  foreach (var row in rows) {
   if (row.Join is not ("and" or "or") || row.Source is not ("field" or "entity" or "position") || string.IsNullOrWhiteSpace(row.Key)) throw new InvalidOperationException("검색 조건의 연결 방식과 항목을 확인하세요.");
   if (!row.Enabled) continue;
   if(row.Source==PositionSearch.Source){result.Add(PositionSearch.Compile(row,catalog,result.Count==0?"and":row.Join));continue;}
   var d = Find(catalog,row);
   if (!Operators(d).Any(o => o.Key == row.Operator)) throw new InvalidOperationException(d.Label + ": 사용할 수 없는 비교 방식입니다.");
   object value = Parse(d, row.Value); object? second = row.Operator == "between" ? Parse(d,row.Value2) : null;
   if (second != null && Convert.ToDouble(value,CultureInfo.InvariantCulture) > Convert.ToDouble(second,CultureInfo.InvariantCulture)) throw new InvalidOperationException(d.Label + ": 최소값이 최대값보다 큽니다.");
   result.Add(new(true,result.Count == 0 ? "and" : row.Join,row.Source,row.Key,row.Operator,value,second));
  }
  return result;
 }
 private static object Parse(SearchConditionDefinition definition, string value)
 {
  value = value?.Trim() ?? "";
  if (definition.Type == "number") {
   if (!double.TryParse(value,NumberStyles.Float,CultureInfo.InvariantCulture,out var n) || !double.IsFinite(n) || n < definition.Min || n > definition.Max || definition.Source == "entity" && n != Math.Truncate(n)) throw new InvalidOperationException($"{definition.Label}: {definition.Min:G}–{definition.Max:G} 범위의 {(definition.Source == "entity" ? "정수" : "숫자")}를 입력하세요.");
   return n;
  }
  if (definition.Type == "text") { if (value.Length is < 1 or > 200) throw new InvalidOperationException(definition.Label + ": 1–200자를 입력하세요."); return value; }
  if (definition.Options == null || !definition.Options.Any(o => o.Value == value)) throw new InvalidOperationException(definition.Label + ": 목록에 있는 값을 선택하세요.");
  if (definition.Type == "boolean") return value == "true";
  if (definition.Type == "reference") return uint.Parse(value,CultureInfo.InvariantCulture);
  return value;
 }
 public static string Describe(SearchConditionRule row, IReadOnlyList<SearchConditionDefinition> catalog)
 {
  if(PositionSearch.IsPosition(row))return PositionSearch.Describe(row);
  var d = catalog.FirstOrDefault(d => d.Source == row.Source && d.Key == row.Key);
  string label = d?.Label ?? row.Key, op = (d == null ? [] : Operators(d)).FirstOrDefault(o => o.Key == row.Operator)?.Label ?? row.Operator;
  string val = d?.Options?.FirstOrDefault(o => o.Value == row.Value)?.Label ?? row.Value;
  return $"{(row.Enabled ? "" : "꺼짐 · ")}{label} {op} {val}{(row.Operator == "between" ? "–" + row.Value2 : "")}";
 }
 public static List<SearchConditionRule> PromoteLegacy(IEnumerable<SearchNumericCondition> conditions) => conditions.Select(c => new SearchConditionRule { Key = c.Key, Operator = c.Min.HasValue && c.Max.HasValue ? c.Min == c.Max ? "eq" : "between" : c.Max.HasValue ? "lte" : "gte", Value = (c.Min ?? c.Max)?.ToString("G17",CultureInfo.InvariantCulture) ?? "", Value2 = c.Max?.ToString("G17",CultureInfo.InvariantCulture) ?? "" }).ToList();
}
