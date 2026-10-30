using System.Text.Json;
using Touchline.Wpf;

internal static class LeagueBrowserTests
{
 internal static void Run(Action<bool, string> check)
 {
  var groups = new List<LeagueGroup>
  {
   new() { Key = "league:101:men", Id = 101, Name = "Premier League", NationId = 765, Nation = "잉글랜드", FootballCategory = "men", ClubCount = 1, Clubs = [new() { Id = 10, Kind = "Club", Name = "웨일스 구단", EnglishName = "Welsh Club", NationalityId = 999 }] },
   new() { Key = "league:102:women", Id = 102, Name = "Premier League", NationId = 765, Nation = "잉글랜드", FootballCategory = "women", ClubCount = 1, Clubs = [new() { Id = 20, Kind = "Club", Name = "여자 구단" }] },
   new() { Key = "league:201:men", Id = 201, Name = "세리에 A", NationId = 777, Nation = "이탈리아", FootballCategory = "men", ClubCount = 1, Clubs = [new() { Id = 30, Kind = "Club", Name = "레체" }] },
   new() { Key = "unknown:men", Name = "기타 / 리그 미확인", Nation = "추정하지 않을 국가", FootballCategory = "men", ClubCount = 1, Clubs = [new() { Id = 40, Kind = "Club", Name = "미확인 구단", NationalityId = 777 }] }
  };
  var nations = LeagueBrowserData.Nations(groups);
  check(nations.Count == 4 && nations[0].Key == "" && nations[^1] == new LeagueNationChoice("unknown", "국가 미확인"), "league_unknown_nation_preserved_without_country_inference");
  check(LeagueBrowserData.FilterLeagues(groups, new()).Count == 4, "league_default_includes_unclassified_clubs");
  check(LeagueBrowserData.FilterLeagues(groups, new(NationKey: "nation:765")).Select(group => group.Id).SequenceEqual(new uint?[] { 101, 102 }), "league_country_uses_competition_nation_not_club_nation");
  check(LeagueBrowserData.FilterLeagues(groups, new(Category: "women")).Single().Key == "league:102:women", "league_duplicate_name_men_women_keep_distinct_keys");
  check(LeagueBrowserData.FilterLeagues(groups, new(NationKey: "unknown")).Single().Clubs[0].Id == 40, "league_unknown_membership_never_dropped");
  check(LeagueBrowserData.FilterLeagues(groups, new(LeagueQuery: "premier")).Count == 2 && LeagueBrowserData.FilterLeagues(groups, new(LeagueQuery: "201")).Single().Name == "세리에 A", "league_query_matches_name_or_verified_id");
  check(LeagueBrowserData.FilterClubs(groups[0], "welsh").Single().Id == 10 && LeagueBrowserData.FilterClubs(groups[0], "10").Single().Id == 10, "league_club_query_matches_english_name_or_id");
  check(LeagueBrowserData.FilterClubs(null, "").Count == 0 && LeagueBrowserData.FilterClubs(groups[0], "없는 구단").Count == 0, "league_empty_results_remain_empty");
  var state = new LeagueBrowserState("nation:765", "women", "Premier", "구단", "league:102:women");
  check(JsonSerializer.Deserialize<LeagueBrowserState>(JsonSerializer.Serialize(state)) == state, "league_navigation_state_roundtrips_stable_key");
  var parsed = JsonSerializer.Deserialize<LeagueCatalog>("""{"leagues":[{"key":"unknown","name":"기타 / 리그 미확인","clubs":[{"kind":"Club","id":50,"name":"구단"}],"club_count":1}],"total_clubs":1,"known_clubs":0,"unclassified_clubs":1,"live":false,"warnings":["소속 미확인"]}""", JsonDefaults.Options)!;
  check(parsed.Leagues.Single().Id == null && parsed.UnclassifiedClubs == 1 && parsed.Leagues.Single().Clubs.Single().Id == 50 && parsed.Warnings.Count == 1, "league_legacy_snapshot_unclassified_contract_decodes");
 }
}
