using System.Text.Json;
using Touchline.Wpf;

internal static class NationalityFilterTests
{
 internal static void Run(Action<bool, string> check)
 {
  bool Reject(Action action) { try { action(); return false; } catch (InvalidOperationException) { return true; } }
  var legacy = JsonSerializer.Deserialize<SearchCriteria>("{\"Name\":\"Legacy\"}", JsonDefaults.Options)!;
  check(legacy.NationalityId == null && legacy.NationalityName == "", "nationality_legacy_saved_search_defaults_to_all");
  var source = new SearchCriteria { Name = "Player", NationalityId = 776, NationalityName = "대한민국", FootballCategory = "men", Conditions = [new("passing", 15)] };
  var clone = source.Clone(); clone.NationalityId = 765; clone.NationalityName = "이탈리아"; clone.Conditions[0].Min = 18;
  check(source.NationalityId == 776 && source.NationalityName == "대한민국" && source.Conditions[0].Min == 15, "nationality_detached_draft_keeps_original_on_cancel");
  var restored = JsonSerializer.Deserialize<SearchCriteria>(JsonSerializer.Serialize(source, JsonDefaults.Options), JsonDefaults.Options)!;
  check(restored.NationalityId == source.NationalityId && restored.NationalityName == source.NationalityName && restored.FootballCategory == "men" && restored.Conditions[0].Min == 15, "nationality_saved_search_restores_id_name_and_other_conditions");
  var state = new SearchReloadState("Player", source.Clone(), "nationality", false);
  check(state.Criteria.NationalityId == 776 && state.Criteria.NationalityName == "대한민국", "nationality_save_reload_state_retains_primary_nation");
  using var facets = JsonDocument.Parse("""{"nations":[{"id":765,"name":"이탈리아","count":7},{"id":776,"name":"대한민국","count":12},{"id":776,"name":"Duplicate","count":99},{"id":0,"name":"Invalid"},{"id":44,"name":" "},{"id":"bad","name":"Wrong"},null]}""");
  var nations = SearchNationalityChoices.FromFacets(facets.RootElement);
  check(nations.Count == 2 && nations.Single(nation => nation.Id == 776).Name == "대한민국" && nations.Single(nation => nation.Id == 776).Count == 12, "nationality_facets_use_ids_and_filter_invalid_duplicates");
  using var noNations = JsonDocument.Parse("{\"roles\":[]}");
  check(SearchNationalityChoices.FromFacets(noNations.RootElement).Count == 0, "nationality_missing_facets_in_legacy_snapshot_remain_empty");
  var choices = SearchNationalityChoices.Build(nations);
  check(choices[0].Id == null && choices[0].Name == SearchNationalityChoices.AllLabel, "nationality_all_choice_always_first");
  var korean = choices.Single(choice => choice.Id == 776);
  check(SearchNationalityChoices.Resolve(choices, null, "대한민국").Id == 776 && SearchNationalityChoices.Resolve(choices, korean, korean.Label).Id == 776, "nationality_exact_name_or_selected_display_resolves_id");
  check(SearchNationalityChoices.Resolve(choices, korean, "").Id == null && SearchNationalityChoices.Resolve(choices, korean, SearchNationalityChoices.AllLabel).Id == null, "nationality_clear_input_and_all_remove_filter");
  check(Reject(() => SearchNationalityChoices.Resolve(choices, korean, "없는 나라")) && Reject(() => SearchNationalityChoices.Resolve(choices, korean, "대한")), "nationality_unrecognized_or_partial_text_never_silently_ignored");
  var sameNames = new[] { new SearchNationChoice(1, "같은 이름"), new SearchNationChoice(2, "같은 이름") };
  check(Reject(() => SearchNationalityChoices.Resolve(sameNames, null, "같은 이름")) && SearchNationalityChoices.Resolve(sameNames, sameNames[1], "같은 이름").Id == 2, "nationality_same_name_requires_identity_selection");
  var missing = SearchNationalityChoices.Build([], 777, "저장한 국가");
  check(missing.Count == 2 && missing[1].Id == 777 && missing[1].Name == "저장한 국가" && SearchNationalityChoices.Resolve(missing, missing[1], missing[1].Name).Id == 777, "nationality_saved_selection_survives_absent_current_facets");
  check(SearchNationalityChoices.Build([], 888)[1].Name == "국적 ID 888", "nationality_missing_saved_label_shows_id_not_all");
  check(new[] { "Player", "Staff", "Person" }.All(SearchNationalityChoices.Supports) && !SearchNationalityChoices.Supports("Club") && !SearchNationalityChoices.Supports("City"), "nationality_primary_person_only_not_club_country");
  check(Reject(() => SearchNationalityChoices.Validate("Player", 0, "")) && Reject(() => SearchNationalityChoices.Validate("Club", 776, "대한민국")), "nationality_invalid_zero_and_wrong_kind_rejected");
 }
}
