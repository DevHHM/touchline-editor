using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public sealed record RecentEntityVisit
{
 public string Kind { get; init; } = "";
 public uint Id { get; init; }
 public string Name { get; init; } = "";
 public string EnglishName { get; init; } = "";
 // No stable save identity is exposed by the bridge. Never persist permission
 // to open a UID: another save can reuse it, even with the same player name.
 [JsonIgnore] public string? SessionToken { get; init; }
 [JsonIgnore] public string SearchName => string.IsNullOrWhiteSpace(EnglishName) ? Name : EnglishName;
 public bool Matches(EntityRecord entity) => entity.Kind == Kind && entity.Id == Id
  && entity.Name == Name && entity.EnglishName == EnglishName;
}

public sealed record RecentSearch
{
 public string Kind { get; init; } = "";
 public SearchCriteria Criteria { get; init; } = new();
 public string SortKey { get; init; } = "name";
 public bool Descending { get; init; }
 public SearchReloadState ToState() => new(Kind, Criteria.Clone(), SortKey, Descending);
}

/// <summary>Local bounded history of profile labels and submitted criteria; no native addresses or cached results.</summary>
public sealed class RecentNavigationHistory
{
 public const int MaximumVisits = 20, MaximumSearches = 20;
 private const int MaximumFileBytes = 1024 * 1024;
 private readonly List<RecentEntityVisit> _visits = [];
 private readonly List<RecentSearch> _searches = [];
 private string? _sessionToken;
 public long SessionGeneration { get; private set; }
 public IReadOnlyList<RecentEntityVisit> Visits => _visits;
 public IReadOnlyList<RecentSearch> Searches => _searches;
 public void BeginSession() { ++SessionGeneration; _sessionToken = Guid.NewGuid().ToString("N"); }
 public void InvalidateSession() { ++SessionGeneration; _sessionToken = null; }
 public void UpdateConnection(bool connected, bool replacement = false)
 {
  if (!connected) InvalidateSession();
  else if (replacement || _sessionToken == null) BeginSession();
 }
 public bool CanReopen(RecentEntityVisit visit) => _sessionToken != null && visit.SessionToken == _sessionToken;
 public static bool ValidKind(string? kind) => kind is "Player" or "Staff" or "Club" or "City" or "Person";
 private static bool ValidText(string? text) => text != null && text.Length <= 200 && !text.Any(char.IsControl);
 private static bool ValidVisit(RecentEntityVisit? visit) => visit != null && ValidKind(visit.Kind)
  && ValidText(visit.Name) && ValidText(visit.EnglishName) && visit.SearchName.Length > 0;
 private static bool ValidSearch(RecentSearch? search)
 {
  if (search == null || !ValidKind(search.Kind) || !ValidText(search.SortKey) || search.SortKey.Length == 0 || search.Criteria == null) return false;
  var c = search.Criteria;
  return new[] { c.Name, c.Club, c.Scope, c.NationalityName, c.Role, c.Employment, c.Group, c.AgeMin, c.AgeMax, c.CaMin, c.CaMax, c.PaMin, c.PaMax }.All(ValidText)
   && c.Scope is "all" or "own" && c.Conditions is { Count: <= 64 } && c.Conditions.All(item => item != null && ValidText(item.Key))
   && c.Rules is { Count: <= 64 } && c.Rules.All(item => item != null
    && new[] { item.Join, item.Source, item.Key, item.Operator, item.Value, item.Value2 }.All(ValidText)
    && item.Positions is { Count: <= 20 } && item.Positions.All(ValidText))
   && JsonSerializer.Serialize(c).Length <= 32768;
 }
 public void Visit(EntityRecord entity)
 {
  if (_sessionToken == null) return;
  var visit = new RecentEntityVisit { Kind = entity.Kind, Id = entity.Id, Name = entity.Name, EnglishName = entity.EnglishName, SessionToken = _sessionToken };
  if (!ValidVisit(visit)) return;
  _visits.RemoveAll(item => item.Kind == visit.Kind && item.Id == visit.Id && item.Name == visit.Name && item.EnglishName == visit.EnglishName);
  _visits.Insert(0, visit);
  if (_visits.Count > MaximumVisits) _visits.RemoveRange(MaximumVisits, _visits.Count - MaximumVisits);
 }
 public void Search(SearchReloadState state)
 {
  var candidate = new RecentSearch { Kind = state.Kind, Criteria = state.Criteria, SortKey = state.SortKey, Descending = state.Descending };
  if (!ValidSearch(candidate)) return;
  candidate = candidate with { Criteria = candidate.Criteria.Clone() };
  candidate.Criteria.Name = candidate.Criteria.Name.Trim(); candidate.Criteria.Club = candidate.Criteria.Club.Trim();
  string key = JsonSerializer.Serialize(candidate);
  _searches.RemoveAll(item => JsonSerializer.Serialize(item) == key);
  _searches.Insert(0, candidate);
  if (_searches.Count > MaximumSearches) _searches.RemoveRange(MaximumSearches, _searches.Count - MaximumSearches);
 }
 public void Clear() { _visits.Clear(); _searches.Clear(); }
 private sealed class Document
 {
  public Document() { }
  public int Version { get; set; } = 1;
  public List<RecentEntityVisit>? Visits { get; set; } = [];
  public List<RecentSearch>? Searches { get; set; } = [];
 }
 public static RecentNavigationHistory Load(string path)
 {
  var history = new RecentNavigationHistory();
  try
  {
   if (!File.Exists(path) || new FileInfo(path).Length > MaximumFileBytes) return history;
   var document = JsonSerializer.Deserialize<Document>(File.ReadAllText(path));
   if (document?.Version != 1) return history;
   foreach (var visit in (document.Visits ?? []).Where(ValidVisit).DistinctBy(item => (item.Kind, item.Id, item.Name, item.EnglishName)).Take(MaximumVisits)) history._visits.Add(visit with { SessionToken = null });
   foreach (var search in (document.Searches ?? []).Where(ValidSearch).DistinctBy(item => JsonSerializer.Serialize(item)).Take(MaximumSearches)) history._searches.Add(search);
  }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException or NotSupportedException) { return new(); }
  return history;
 }
 public void Save(string path)
 {
  string fullPath = Path.GetFullPath(path);
  Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
  string temporary = fullPath + "." + Guid.NewGuid().ToString("N") + ".tmp";
  try
  {
   File.WriteAllText(temporary, JsonSerializer.Serialize(new Document { Visits = _visits, Searches = _searches }));
   File.Move(temporary, fullPath, true);
  }
  finally { if (File.Exists(temporary)) File.Delete(temporary); }
 }
}
