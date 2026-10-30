using System.Globalization;
using System.Text.Json;
using Touchline.Wpf;

internal static class SearchColumnTests
{
 internal static void Run(Action<bool, string> check)
 {
  EditableField[] fields = [
   new() { Key = "passing", Label = "패스", Section = "기술", Hint = "정확한 패스 능력" },
   new() { Key = "contract.weekly_wage", Label = "급료", Section = "계약" },
   new() { Key = "contract.expires", Label = "계약 종료", Section = "계약" },
   new() { Key = "score_assistant", Label = "수석코치", Section = "평가" },
   new() { Key = "unclassified", Label = "추가 정보", Section = " " },
   new() { Key = "ca", Label = "중복 CA", Section = "기본" },
   new() { Key = "passing", Label = "중복 패스", Section = "기술" }
  ];
  var player = SearchColumns.Available("Player", fields);
  var many = Enumerable.Range(0,70).Select(i => new SearchColumnSpec("scalar_"+i,"값 "+i,"검증",100,true)).Prepend(new("name","이름","기본",200)).ToList();
  check(SearchColumns.Normalize(many.Select(c=>c.Key),many).Count==65, "search_columns_saved_numeric_columns_are_capped_at_64");
  check(player.Select(c => c.Key).Distinct().Count() == player.Count && player.Count(c => c.Key == "ca") == 1 && player.Count(c => c.Key == "passing") == 1, "search_columns_available_deduplicates_scalars_and_builtin_keys");
  check(player.Single(c => c.Key == "nationality").Label == "국적" && !player.Single(c => c.Key == "nationality").Numeric && player.Single(c => c.Key == "role").Label == "포지션", "search_columns_player_nationality_and_position_are_basic_columns");
  check(player.Single(c => c.Key == "contract.weekly_wage") is { Numeric: true, Label: "주급 (£/주)", Width: 135 } && player.Single(c => c.Key == "contract.expires").Width == 115 && player.Single(c => c.Key == "score_assistant").Width == 145 && player.Single(c => c.Key == "unclassified").Section == "기타 정보", "search_columns_scalar_metadata_preserves_units_and_widths");
  check(SearchColumns.Available("Staff", []).Single(c => c.Key == "role").Label == "현재 직무" && !SearchColumns.Available("City", []).Any(c => c.Key == "nationality") && SearchColumns.Available("City", []).Single(c => c.Key == "group").Label == "국가", "search_columns_entity_specific_basics_do_not_confuse_city_with_person_nationality");
  check(SearchColumns.Default("Player").Take(3).SequenceEqual(new[] { "name", "nationality", "age" }) && SearchColumns.Default("Staff").Contains("nationality") && !SearchColumns.Default("Person").Contains("ca"), "search_columns_default_person_lists_include_nationality");
  string[] selected = ["passing", "name", "stale_field", "nationality", "passing", "club", "NAME"];
  check(SearchColumns.Normalize(selected, player).SequenceEqual(new[] { "name", "passing", "nationality", "club" }) && selected.Length == 7 && SearchColumns.Normalize([], player).SequenceEqual(new[] { "name" }), "search_columns_normalization_keeps_order_requires_name_and_removes_unknown_or_duplicate_keys");
  check(SearchColumns.Normalize(SearchColumns.Default("City"), SearchColumns.Available("City", [])).SequenceEqual(new[] { "name", "group", "role" }), "search_columns_normalization_removes_inapplicable_default_for_city");

  var culture = CultureInfo.CurrentCulture;
  try
  {
   CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo("en-US");
   check(SearchColumns.Format("person.dob", 20040229d) == "2004-02-29" && SearchColumns.Format("contract.expires", 20300630d) == "2030-06-30", "search_columns_dates_format_as_iso_dates");
   check(new[] { 20230229d, 20241301d, 0d, -1d, 20240229.5d }.All(n => SearchColumns.Format("person.dob", n) == "미확인"), "search_columns_invalid_and_fractional_dates_are_not_silently_rounded");
   check(SearchColumns.Format("contract.weekly_wage", 12500d) == "£12,500" && SearchColumns.Format("club.balance", -1250000d) == "£-1,250,000" && SearchColumns.Format("player.market_value", -1d) == "미설정", "search_columns_money_preserves_units_negative_balance_and_unset_price");
   check(new[] { "club.cash", "club.transfer_base", "club.transfer_current", "sponsor.0.annual", "sponsor.3.total" }.All(key => SearchColumns.IsMoney(key) && SearchColumns.Format(key, 1250000d) == "£1,250,000") && SearchColumns.Format("club.cash", -1250000d) == "£-1,250,000" && !SearchColumns.IsMoney("club.reputation") && !SearchColumns.IsMoney("sponsor.0.start_year"), "search_columns_actual_club_finance_schema_preserves_currency_and_negative_cash");
   check(SearchColumns.Format("balance", 16d) == "16" && !SearchColumns.IsMoney("balance") && SearchColumns.Format("contract.squad_number", 12d) == "12" && SearchColumns.Format("contract.future_numeric", 8d) == "8", "search_columns_nonmoney_balance_and_contract_numeric_values_stay_numeric");
   check(SearchColumns.Format("contract.listed", 1d) == "예" && SearchColumns.Format("player.injured", 0d) == "아니오" && SearchColumns.Format("trait_17", 1d) == "예" && SearchColumns.Format("passing", 13.625d) == "13.63", "search_columns_booleans_and_fractional_attributes_format_separately");
   check(new object?[] { null, double.NaN, double.PositiveInfinity, "14" }.All(v => SearchColumns.Format("passing", v) == "—"), "search_columns_missing_or_nonfinite_values_never_become_zero");
  }
  finally { CultureInfo.CurrentCulture = culture; }

  var modern = JsonSerializer.Deserialize<EntityRecord>("""{"id":1,"kind":"Player","name":"김민수","nationality":"대한민국","nationality_id":776,"club":"레체","club_id":1141}""", JsonDefaults.Options)!;
  var legacy = JsonSerializer.Deserialize<EntityRecord>("""{"id":2,"kind":"Staff","name":"코치","club":"레체","club_id":1141}""", JsonDefaults.Options)!;
  check(modern.Nationality == "대한민국" && modern.NationalityId == 776 && modern.NationalityLabel == "대한민국" && modern.Club == "레체", "entity_nationality_dto_deserializes_separately_from_employing_club");
  check(legacy.Nationality == "" && legacy.NationalityId == null && legacy.NationalityLabel == "미확인", "entity_legacy_snapshot_without_nationality_is_backward_compatible");
  var unknown = JsonSerializer.Deserialize<EntityRecord>("""{"nationality":null,"nationality_id":null}""", JsonDefaults.Options)!;
  check(unknown.NationalityLabel == "미확인" && new EntityRecord { Nationality = "   " }.NationalityLabel == "미확인", "entity_null_or_whitespace_nationality_is_unknown");
  using (var serialized = JsonDocument.Parse(JsonSerializer.Serialize(modern, JsonDefaults.Options)))
   check(serialized.RootElement.GetProperty("nationality").GetString() == "대한민국" && serialized.RootElement.GetProperty("nationality_id").GetUInt32() == 776 && !serialized.RootElement.TryGetProperty("NationalityLabel", out _), "entity_nationality_serialization_uses_backend_keys_without_display_property");

  string directory = Path.Combine(Path.GetTempPath(), "Touchline-SearchColumns-" + Guid.NewGuid().ToString("N"));
  Directory.CreateDirectory(directory);
  try
  {
   string path = Path.Combine(directory, "layouts.json");
   var settings = new SearchColumnSettings(path);
   check(settings.Get("Player").Keys.Count == 0 && settings.LoadError == null && !File.Exists(path), "search_columns_missing_settings_have_clean_default_without_writing");
   var playerLayout = new SearchColumnLayout { View = "player-custom", Keys = ["name", "nationality", "passing"], Widths = new() { ["name"] = 250, ["passing"] = 90 } };
   var staffLayout = new SearchColumnLayout { View = "staff-custom", Keys = ["name", "score_assistant"], Widths = new() { ["name"] = 270 } };
   settings.Save("Player", playerLayout);
   settings.Save("Staff", staffLayout);
   playerLayout.Keys.Add("ca"); playerLayout.Widths["name"] = 700;
   var reopened = new SearchColumnSettings(path);
   check(reopened.Get("Player").Keys.SequenceEqual(new[] { "name", "nationality", "passing" }) && reopened.Get("Player").Widths["name"] == 250 && reopened.Get("Staff").Keys.SequenceEqual(new[] { "name", "score_assistant" }) && reopened.Get("Staff").View == "staff-custom" && !File.Exists(path + ".tmp"), "search_columns_persistence_keeps_player_staff_layouts_separate_and_copies_saved_draft");
   var draft = reopened.Get("Player"); draft.Keys.Clear(); draft.Widths.Clear(); draft.View = "cancelled";
   check(reopened.Get("Player").Keys.Count == 3 && reopened.Get("Player").Widths.Count == 2 && reopened.Get("Player").View == "player-custom", "search_columns_cancelled_draft_cannot_mutate_cached_layout");
   var copy = staffLayout.Copy(); copy.Keys.Clear(); copy.Widths.Clear();
   check(staffLayout.Keys.Count == 2 && staffLayout.Widths.Count == 1, "search_columns_layout_copy_is_deep_for_mutable_lists_and_widths");

   string sanitizedPath = Path.Combine(directory, "sanitized.json");
   File.WriteAllText(sanitizedPath, """{"Player":{"View":null,"Keys":null,"Widths":{"valid":200,"min":48,"max":800,"tiny":47,"huge":801}},"Staff":null}""");
   var sanitized = new SearchColumnSettings(sanitizedPath);
   check(sanitized.Get("Player").View == "default" && sanitized.Get("Player").Keys.Count == 0 && sanitized.Get("Player").Widths.Keys.Order().SequenceEqual(new[] { "max", "min", "valid" }) && sanitized.Get("Staff").Keys.Count == 0, "search_columns_settings_sanitize_null_values_and_unusable_widths");
   string invalidPath = Path.Combine(directory, "invalid.json"); File.WriteAllText(invalidPath, "{not JSON");
   var invalid = new SearchColumnSettings(invalidPath);
   check(invalid.Get("Player").Keys.Count == 0 && !string.IsNullOrWhiteSpace(invalid.LoadError), "search_columns_invalid_settings_file_recovers_with_diagnostic");
   invalid.Save("Staff", staffLayout);
   check(new SearchColumnSettings(invalidPath).Get("Staff").Keys.SequenceEqual(staffLayout.Keys), "search_columns_invalid_settings_can_be_replaced_by_valid_selection");
   string oversizedPath = Path.Combine(directory, "oversized.json"); File.WriteAllText(oversizedPath, new string(' ', 512 * 1024 + 1));
   var oversized = new SearchColumnSettings(oversizedPath);
   bool oversizedRecovered;
   try { oversizedRecovered = oversized.Get("Player").Keys.Count == 0 && !string.IsNullOrWhiteSpace(oversized.LoadError); }
   catch { oversizedRecovered = false; }
   check(oversizedRecovered, "search_columns_oversized_settings_file_recovers_without_exception");
  }
  finally { Directory.Delete(directory, true); }
 }
}
