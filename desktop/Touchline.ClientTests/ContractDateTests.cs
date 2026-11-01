using System.Text.Json;
using Touchline.Wpf;

internal static class ContractDateTests
{
 public static void Run(Action<bool, string> check)
 {
  EditableField Date(string key, double value, bool editable = true) => new() { Key = key, Label = key, Section = "계약", Min = 19000101, Max = 21001231, Step = 1, Editable = editable, Value = value };
  var end = Date("contract.expires", 20280630);
  end.EditText = "20240229";
  check(end.IsDirty && end.IsValid && end.Value == 20240229 && end.Original == 20280630, "contract_date_valid_leap_day_draft_keeps_original");
  foreach (string invalid in new[] { "20230229", "20240431", "20241301", "20240229.5", "not a date", "" })
  {
   end.EditText = invalid;
   check(!end.IsValid && end.IsDirty && end.Value == 20240229, "contract_date_rejects_" + (invalid.Length == 0 ? "clear" : invalid.Replace(' ', '_')));
  }
  end.Reset();
  check(end.Value == 20280630 && !end.IsDirty && end.IsValid && end.EditText == "20280630", "contract_date_reset_restores_exact_original_after_invalid_input");
  var readOnly = Date("contract.expires", 20280630, false); readOnly.EditText = "20300630";
  check(!readOnly.IsDirty && readOnly.Value == 20280630, "contract_date_readonly_cannot_create_edit");
  var start = Date("contract.starts", 20240701); end.EditText = "20240630";
  check(DateFieldPresentation.PeriodError([start, end]) != null, "contract_date_start_after_expiry_blocks_review");
  end.EditText = "20240701";
  check(DateFieldPresentation.PeriodError([start, end]) == null && DateFieldPresentation.PeriodError([end]) == null, "contract_date_equal_day_and_missing_start_do_not_invent_duration");
  var unknown = new DetailRecord { Entity = new() { Kind = "Staff" }, Notes = ["현재 연결 버전에서는 계약 날짜 조회·편집을 지원하지 않습니다."] };
  check(DateFieldPresentation.ContractMessages(unknown).Single() == unknown.Notes[0] && unknown.Fields.Count == 0, "contract_dates_unsupported_backend_reason_preserved_without_fake_fields");
  unknown.Fields.Add(Date("contract.expires", 20300630)); unknown.Notes.Clear();
  check(DateFieldPresentation.ContractMessages(unknown).Single().Contains("확인되지 않은") && unknown.Fields.Count == 1, "contract_dates_partial_expiry_does_not_synthesize_start");
  check(new[] { 20230229d, 20240229.5, double.NaN, 0 }.All(value => !DateFieldPresentation.TryParse(value, out _)), "contract_dates_unknown_or_fractional_values_never_become_calendar_dates");
  using var json = JsonDocument.Parse("{\"before\":20280630,\"after\":20300630}");
  check(DateFieldPresentation.FormatChange("contract.expires", json.RootElement.GetProperty("before")) == "2028-06-30" && DateFieldPresentation.FormatChange("contract.expires", json.RootElement.GetProperty("after")) == "2030-06-30", "contract_date_review_formats_both_values_as_calendar_dates");
  check(DateFieldPresentation.TryParseInput("2030-06-30", System.Globalization.CultureInfo.GetCultureInfo("en-US"), out var iso) && iso == new DateTime(2030, 6, 30) && DateFieldPresentation.TryParseInput("6/30/2030", System.Globalization.CultureInfo.GetCultureInfo("en-US"), out var local) && local == iso, "contract_date_input_accepts_complete_iso_and_local_dates");
  check(new[] { "", "2030", "6/30", "6", "2030-02-29" }.All(text => !DateFieldPresentation.TryParseInput(text, System.Globalization.CultureInfo.GetCultureInfo("en-US"), out _)), "contract_date_partial_input_cannot_invent_month_or_year");
  var koreanOverride = (System.Globalization.CultureInfo)System.Globalization.CultureInfo.GetCultureInfo("ko-KR").Clone(); koreanOverride.DateTimeFormat.ShortDatePattern = "yyyy-MM-dd";
  check(DateFieldPresentation.TryParseInput("2000. 2. 29.", koreanOverride, out var koreanDate) && koreanDate == new DateTime(2000, 2, 29), "contract_date_input_accepts_native_picker_format_with_user_date_format_override");
  check(SearchColumns.IsDate("contract.starts") && SearchColumns.Format("contract.starts", 20240701d) == "2024-07-01", "contract_date_backend_supplied_start_uses_date_presentation");
 }
}
