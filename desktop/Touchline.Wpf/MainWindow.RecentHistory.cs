using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private RecentNavigationHistory _recentHistory = new();
 private static string RecentHistoryPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "recent-history.json");
 private void InitializeRecentHistory()
 {
  // Offline UI verification must never read or overwrite the user's history.
  if (_qaOutput == null) _recentHistory = RecentNavigationHistory.Load(RecentHistoryPath);
 }
 private void SaveRecentHistory()
 {
  if (_qaOutput != null || _qaRunning) return;
  try { _recentHistory.Save(RecentHistoryPath); }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException)
  { Status("최근 기록을 저장하지 못했습니다. 이번 연결에서는 계속 사용할 수 있습니다."); }
 }
 private static string HistoryLabel(string text) => (text.Length > 58 ? text[..55] + "…" : text).Replace("_", "__", StringComparison.Ordinal);
 private (SearchReloadState State, long Generation)? CaptureQuickSearch(string kind, Dictionary<string, object?> arguments)
 {
  if (!_dataLoaded || _offset != 0 || arguments["query"] is not string query || query.Length == 0) return null;
  var criteria = new SearchCriteria { Name = query, Scope = arguments.ContainsKey("club_id") ? "own" : "all" };
  if (kind == "Player" && arguments.TryGetValue("group", out var group)) criteria.Group = group?.ToString() ?? "";
  if (FootballSearchCategories.Supports(kind)) criteria.FootballCategory = (FootballBox.SelectedItem as ComboBoxItem)?.Tag?.ToString() is string category && category.Length > 0 ? category : null;
  return (new(kind, criteria, arguments["sort"]?.ToString() ?? "name", arguments["descending"] is true), _recentHistory.SessionGeneration);
 }
 private void RecordQuickSearch((SearchReloadState State, long Generation)? capture)
 {
  if (!_dataLoaded || !capture.HasValue || capture.Value.Generation != _recentHistory.SessionGeneration) return;
  _recentHistory.Search(capture.Value.State); SaveRecentHistory();
 }
 private void RecentHistory_Click(object sender, RoutedEventArgs args)
 {
  var menu = new ContextMenu { PlacementTarget = RecentHistoryButton, Placement = PlacementMode.Right, MaxHeight = 560 };
  menu.Items.Add(new MenuItem { Header = "최근 기록 · 이 PC에 저장", IsEnabled = false });
  var visits = new MenuItem { Header = "최근 프로필", IsEnabled = _recentHistory.Visits.Count > 0 };
  foreach (var visit in _recentHistory.Visits)
  {
   bool current = _recentHistory.CanReopen(visit);
   string caption = $"{Label(visit.Kind)} · {(string.IsNullOrWhiteSpace(visit.Name) ? visit.EnglishName : visit.Name)} · {visit.Id}";
   var item = new MenuItem { Header = HistoryLabel(caption), IsEnabled = _dataLoaded && !_busy,
    ToolTip = current ? "현재 연결의 프로필을 새로 읽습니다." : "이전 연결의 기록입니다. 현재 자료에서 이름으로 검색하고 대상을 다시 선택합니다." };
   item.Click += async (_, _) =>
   {
    if (!_dataLoaded || _busy || !CanLeave()) return;
    await Guard(async () =>
    {
     if (_recentHistory.CanReopen(visit)) await OpenEntity(visit.Kind, visit.Id, expectedVisit: visit);
     else await ReopenRecentSearchAsync(new(visit.Kind, new() { Name = visit.SearchName }, "name", false));
    }, true);
   };
   visits.Items.Add(item);
  }
  var searches = new MenuItem { Header = "실행한 검색", IsEnabled = _recentHistory.Searches.Count > 0 };
  foreach (var search in _recentHistory.Searches)
  {
   // Do not expose numerical CA/PA conditions in history labels while hidden.
   string query = string.IsNullOrWhiteSpace(search.Criteria.Name) ? "조건 검색" : search.Criteria.Name;
   var item = new MenuItem { Header = HistoryLabel($"{Label(search.Kind)} · {query}"), IsEnabled = _dataLoaded && !_busy,
    ToolTip = "현재 자료에서 저장된 조건을 다시 검증하고 검색합니다. 이전 검색 결과는 재사용하지 않습니다." };
   item.Click += async (_, _) =>
   {
    if (!_dataLoaded || _busy || !CanLeave()) return;
    await Guard(() => ReopenRecentSearchAsync(search.ToState()), true, "최근 검색을 다시 실행하는 중…");
   };
   searches.Items.Add(item);
  }
  menu.Items.Add(visits); menu.Items.Add(searches); menu.Items.Add(new Separator());
  menu.Items.Add(new MenuItem { Header = "연결이 바뀐 프로필은 이름으로 검색합니다", IsEnabled = false });
  var clear = new MenuItem { Header = "최근 기록 모두 지우기", IsEnabled = _recentHistory.Visits.Count + _recentHistory.Searches.Count > 0 };
  clear.Click += (_, _) => { _recentHistory.Clear(); SaveRecentHistory(); Status("최근 프로필과 검색 기록을 지웠습니다."); };
  menu.Items.Add(clear); RecentHistoryButton.ContextMenu = menu; menu.IsOpen = true;
 }
 private async Task ReopenRecentSearchAsync(SearchReloadState state)
 {
  long generation = _recentHistory.SessionGeneration;
  await ShowSearchAsync(state.Kind);
  if (!_dataLoaded || generation != _recentHistory.SessionGeneration || _searchView == null) return;
  await _searchView.RestoreReloadStateAsync(state);
 }
}
