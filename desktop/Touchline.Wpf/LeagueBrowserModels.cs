using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public sealed class LeagueCatalog
{
 [JsonPropertyName("leagues")] public List<LeagueGroup> Leagues { get; set; } = [];
 [JsonPropertyName("total_clubs")] public int TotalClubs { get; set; }
 [JsonPropertyName("known_clubs")] public int KnownClubs { get; set; }
 [JsonPropertyName("unclassified_clubs")] public int UnclassifiedClubs { get; set; }
 [JsonPropertyName("live")] public bool Live { get; set; }
 [JsonPropertyName("warnings")] public List<string> Warnings { get; set; } = [];
}

public sealed class LeagueGroup
{
 [JsonPropertyName("key")] public string Key { get; set; } = "";
 [JsonPropertyName("id")] public uint? Id { get; set; }
 [JsonPropertyName("name")] public string Name { get; set; } = "";
 [JsonPropertyName("nation_id")] public uint? NationId { get; set; }
 [JsonPropertyName("nation")] public string Nation { get; set; } = "";
 [JsonPropertyName("football_category")] public string FootballCategory { get; set; } = "unknown";
 [JsonPropertyName("club_count")] public int ClubCount { get; set; }
 [JsonPropertyName("clubs")] public List<EntityRecord> Clubs { get; set; } = [];
 [JsonIgnore] public string NationKey => NationId is > 0 ? $"nation:{NationId}" : "unknown";
 [JsonIgnore] public string NationLabel => NationId is > 0 && !string.IsNullOrWhiteSpace(Nation) ? Nation : "국가 미확인";
 [JsonIgnore] public string CategoryLabel => FootballCategory switch { "men" => "남자축구", "women" => "여자축구", _ => "축구 구분 미확인" };
 [JsonIgnore] public string Subtitle => $"{NationLabel} · {CategoryLabel} · {ClubCount:N0}개 구단";
}

public sealed record LeagueNationChoice(string Key, string Name);
public sealed record LeagueBrowserState(string NationKey = "", string Category = "", string LeagueQuery = "", string ClubQuery = "", string? LeagueKey = null);

public static class LeagueBrowserData
{
 public static List<LeagueNationChoice> Nations(IEnumerable<LeagueGroup> leagues) =>
  [new("", "모든 국가"), ..leagues.GroupBy(group => group.NationKey).Select(group => new LeagueNationChoice(group.Key, group.First().NationLabel))
   .OrderBy(nation => nation.Key == "unknown").ThenBy(nation => nation.Name, StringComparer.CurrentCultureIgnoreCase)];

 public static List<LeagueGroup> FilterLeagues(IEnumerable<LeagueGroup> leagues, LeagueBrowserState state) => leagues
  .Where(group => (state.NationKey == "" || group.NationKey == state.NationKey) && (state.Category == "" || group.FootballCategory == state.Category))
  .Where(group => Matches(group.Name, state.LeagueQuery) || Matches(group.Id?.ToString(), state.LeagueQuery))
  .OrderBy(group => group.Id == null).ThenBy(group => group.NationLabel, StringComparer.CurrentCultureIgnoreCase)
  .ThenBy(group => group.Name, StringComparer.CurrentCultureIgnoreCase).ThenBy(group => group.Key, StringComparer.Ordinal).ToList();

 public static List<EntityRecord> FilterClubs(LeagueGroup? league, string query) => (league?.Clubs ?? [])
  .Where(club => club.Kind == "Club" && (Matches(club.DisplayName, query) || Matches(club.EnglishName, query) || Matches(club.Id.ToString(), query)))
  .OrderBy(club => club.DisplayName, StringComparer.CurrentCultureIgnoreCase).ThenBy(club => club.Id).ToList();

 private static bool Matches(string? value, string query) => string.IsNullOrWhiteSpace(query) || (value?.Contains(query.Trim(), StringComparison.CurrentCultureIgnoreCase) ?? false);
}
