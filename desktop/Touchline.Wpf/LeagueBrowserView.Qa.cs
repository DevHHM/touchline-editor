using System.Windows;
using System.Windows.Threading;

namespace Touchline.Wpf;

public sealed partial class LeagueBrowserView
{
 internal EntityRecord? QaSelectedClub => _clubs.Items.OfType<EntityRecord>().FirstOrDefault();
 internal async Task<object> QaAsync(string directory)
 {
  if (_catalog == null || _catalog.Live) throw new InvalidOperationException("리그 QA는 불러온 스냅샷만 사용합니다.");
  var catalog = _catalog;
  var known = catalog.Leagues.FirstOrDefault(group => group.Id.HasValue && group.Clubs.Count > 0) ?? throw new InvalidOperationException("리그 소속이 확인된 구단 fixture가 필요합니다.");
  RestoreState(new());
  if (_leagues.Items.Count != catalog.Leagues.Count) throw new InvalidOperationException("리그 전체 목록이 누락됐습니다.");
  foreach (var nation in LeagueBrowserData.Nations(catalog.Leagues).Where(nation => nation.Key != ""))
  {
   _nations.SelectedValue = nation.Key;
   var expected = catalog.Leagues.Where(group => group.NationKey == nation.Key).Select(group => group.Key).Order().ToArray();
   if (!_leagues.Items.OfType<LeagueGroup>().Select(group => group.Key).Order().SequenceEqual(expected)) throw new InvalidOperationException("대회 국가 필터가 실제 국가 ID와 다릅니다.");
  }
  RestoreState(new());
  foreach (string category in new[] { "men", "women", "unknown" })
  {
   _categories.SelectedValue = category;
   if (!_leagues.Items.OfType<LeagueGroup>().Select(group => group.Key).Order().SequenceEqual(catalog.Leagues.Where(group => group.FootballCategory == category).Select(group => group.Key).Order())) throw new InvalidOperationException("리그 남녀 구분 필터 불일치.");
  }
  RestoreState(new(LeagueKey: known.Key));
  var club = known.Clubs.First(); _clubSearch.Text = club.Id.ToString();
  if (!_clubs.Items.OfType<EntityRecord>().Any(row => row.Id == club.Id)) throw new InvalidOperationException("리그 내부 구단 ID 검색 실패.");
  _clubSearch.Text = "__no_club__";
  if (_clubs.Items.Count != 0 || _empty.Visibility != Visibility.Visible || _open.IsEnabled) throw new InvalidOperationException("빈 구단 결과에 이전 링크가 남았습니다.");
  _leagueSearch.Text = "__no_league__";
  if (_leagues.Items.Count != 0 || _clubs.Items.Count != 0) throw new InvalidOperationException("빈 리그 결과에 이전 구단이 남았습니다.");
  RestoreState(new(LeagueKey: known.Key));
  var state = CaptureState(); RestoreState(new()); RestoreState(state);
  if (CaptureState() != state || !_clubs.Items.OfType<EntityRecord>().Select(row => row.Id).Order().SequenceEqual(known.Clubs.Select(row => row.Id).Order())) throw new InvalidOperationException("리그 선택/검색 상태 복원 실패.");
  _clubs.SelectedIndex = 0;
  if (!_open.IsEnabled) throw new InvalidOperationException("구단 상세 링크가 활성화되지 않습니다.");
  await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
  return new { nations = LeagueBrowserData.Nations(catalog.Leagues).Count - 1, leagues = catalog.Leagues.Count, clubs = catalog.TotalClubs, unknown = catalog.UnclassifiedClubs, selected_key = known.Key, country_filter = true, category_filter = true, club_search = true, empty_results_clear_links = true, restore_state = true };
 }
 internal void QaCheckBounds()
 {
  foreach (FrameworkElement element in new FrameworkElement[] { _nations, _categories, _leagueSearch, _clubSearch, _leagues, _clubs, _open })
  {
   var bounds = element.TransformToAncestor(this).TransformBounds(new Rect(element.RenderSize));
   if (!element.IsVisible || element.ActualWidth < 30 || element.ActualHeight < 20 || bounds.Left < -1 || bounds.Top < -1 || bounds.Right > ActualWidth + 1 || bounds.Bottom > ActualHeight + 1) throw new InvalidOperationException("리그 탐색 컨트롤이 최소 창 크기에서 잘립니다: " + element.GetType().Name);
  }
  if (_clubs.ActualWidth < 260 || _clubs.ActualHeight < 100) throw new InvalidOperationException("구단 표가 최소 크기보다 작습니다.");
 }
}
