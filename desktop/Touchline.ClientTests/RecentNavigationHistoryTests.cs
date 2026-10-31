using System.Text.Json;
using Touchline.Wpf;

internal static class RecentNavigationHistoryTests
{
 public static void Run(Action<bool, string> check)
 {
  var history = new RecentNavigationHistory();
  EntityRecord Person(uint id, string name = "선수") => new() { Kind = "Player", Id = id, Name = name, EnglishName = "Player " + id, Ca = 123, Pa = 199 };
  history.Visit(Person(1));
  check(history.Visits.Count == 0, "recent_visit_requires_loaded_session");
  history.BeginSession();
  for (uint id = 1; id <= 25; id++) history.Visit(Person(id));
  check(history.Visits.Count == RecentNavigationHistory.MaximumVisits && history.Visits[0].Id == 25 && history.Visits[^1].Id == 6, "recent_visits_bounded_most_recent_first");
  history.Visit(Person(10));
  check(history.Visits.Count == 20 && history.Visits[0].Id == 10 && history.Visits.Count(item => item.Id == 10) == 1, "recent_visit_deduplicates_and_promotes");
  var current = history.Visits[0];
  check(history.CanReopen(current) && current.Matches(Person(10)) && !current.Matches(Person(10, "다른 선수")), "recent_direct_reopen_requires_exact_current_entity_identity");
  long generation = history.SessionGeneration; history.UpdateConnection(true);
  check(history.CanReopen(current) && history.SessionGeneration == generation, "recent_status_or_date_refresh_does_not_replace_loaded_session");
  history.Visit(new() { Kind = "Club", Id = 10, Name = "구단" });
  check(history.Visits.Count(item => item.Id == 10) == 2, "recent_visits_keep_kind_qualified_uid");
  history.InvalidateSession();
  check(!history.CanReopen(current), "recent_save_invalidation_immediately_revokes_uid_reopen");
  history.BeginSession();
  check(!history.CanReopen(current), "recent_foreign_save_even_same_uid_and_name_requires_search");
  history.Visit(Person(10));
  check(history.CanReopen(history.Visits[0]) && history.Visits.Count(item => item.Kind == "Player" && item.Id == 10) == 1, "recent_revisited_profile_gets_current_session_without_duplicate");
  var beforeReplacement = history.Visits[0]; history.UpdateConnection(true, replacement: true);
  check(!history.CanReopen(beforeReplacement), "recent_manual_connect_or_snapshot_replacement_revokes_old_uid");
  history.UpdateConnection(false); history.UpdateConnection(true);
  check(!history.CanReopen(beforeReplacement), "recent_disconnection_and_restore_never_reenable_previous_uid");
  var criteria = new SearchCriteria { Name = "  손흥민  ", Scope = "all", Conditions = [new("passing", 10)], Rules = [new() { Key = "position", Positions = ["ST"] }] };
  history.Search(new("Player", criteria, "name", false));
  criteria.Conditions[0].Min = 19; criteria.Rules[0].Positions.Clear(); criteria.Name = "changed";
  check(history.Searches[0].Criteria.Name == "손흥민" && history.Searches[0].Criteria.Conditions[0].Min == 10 && history.Searches[0].Criteria.Rules[0].Positions.Count == 1, "recent_search_detaches_mutable_draft_and_trims_query");
  var replay = history.Searches[0].ToState(); replay.Criteria.Name = "mutated replay";
  check(history.Searches[0].Criteria.Name == "손흥민", "recent_search_replay_is_detached");
  history.Search(history.Searches[0].ToState());
  check(history.Searches.Count == 1, "recent_search_same_criteria_and_sort_deduplicated");
  history.Search(new("Staff", history.Searches[0].Criteria, "name", false));
  check(history.Searches.Count == 2, "recent_search_kind_is_part_of_identity");
  for (int index = 0; index < 25; index++) history.Search(new("Player", new() { Name = "query " + index }, "name", false));
  check(history.Searches.Count == RecentNavigationHistory.MaximumSearches && history.Searches[0].Criteria.Name == "query 24" && history.Searches[^1].Criteria.Name == "query 5", "recent_searches_bounded_most_recent_first");
  history.Search(new("Unknown", new(), "name", false));
  history.Search(new("Player", new() { Conditions = null! }, "name", false));
  history.Visit(new() { Kind = "Player", Id = 1, Name = new string('x', 201) });
  check(history.Searches.Count == 20 && history.Visits.Count == 20, "recent_invalid_kinds_and_oversized_or_null_drafts_ignored");
  string directory = Path.Combine(Path.GetTempPath(), "touchline-recent-test-" + Guid.NewGuid().ToString("N"));
  string path = Path.Combine(directory, "history.json");
  try
  {
   check(RecentNavigationHistory.Load(path).Visits.Count == 0, "recent_missing_file_defaults_empty");
   history.Save(path);
   string json = File.ReadAllText(path);
   var restored = RecentNavigationHistory.Load(path); restored.BeginSession();
   check(restored.Visits.Count == 20 && restored.Searches.Count == 20 && restored.Searches[0].Criteria.Name == "query 24", "recent_history_persistent_roundtrip");
   check(!restored.CanReopen(restored.Visits[0]) && !json.Contains("SessionToken") && !json.Contains("\"Ca\"") && !json.Contains("\"Pa\""), "recent_restart_never_restores_uid_permission_or_ability_values");
   foreach (string malformed in new[] { "{broken", "null", "{\"Version\":99}", "{\"Visits\":[null],\"Searches\":[null]}", "{\"Searches\":[{\"Kind\":\"Player\",\"Criteria\":{\"Name\":null}}]}" })
   {
    File.WriteAllText(path, malformed);
    var empty = RecentNavigationHistory.Load(path);
    check(empty.Visits.Count + empty.Searches.Count == 0, "recent_corrupt_or_unsupported_file_is_ignored");
   }
   File.WriteAllText(path, new string(' ', 1024 * 1024 + 1));
   check(RecentNavigationHistory.Load(path).Visits.Count == 0, "recent_oversized_history_rejected_before_deserialization");
   restored.Clear(); restored.Save(path);
   check(RecentNavigationHistory.Load(path).Visits.Count + RecentNavigationHistory.Load(path).Searches.Count == 0 && Directory.GetFiles(directory, "*.tmp").Length == 0, "recent_clear_persists_and_atomic_save_leaves_no_temporary_files");
  }
  finally { if (File.Exists(path)) File.Delete(path); if (Directory.Exists(directory)) Directory.Delete(directory); }
 }
}
