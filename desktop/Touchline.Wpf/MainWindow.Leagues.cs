using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private LeagueBrowserView? _leagueBrowser;
 private bool _returnToLeague;
 private async void Leagues_Click(object sender, RoutedEventArgs e)
 {
  if (!_ready || _busy || !CanLeave()) return;
  await Guard(() => ShowLeagueBrowserAsync(), true, "리그·구단 불러오는 중…");
 }
 private async Task ShowLeagueBrowserAsync(LeagueBrowserState? state = null, bool refresh = false)
 {
  if (_bridge == null || !_dataLoaded) { Status("FM에 연결하거나 스냅샷을 먼저 열어주세요."); return; }
  _searchTimer.Stop(); _scoreTimer.Stop(); ++_searchVersion;
  if (_searchView != null) await _searchView.StopAsync();
  await StopDetailViewsAsync(); ClearDetail();
  if (_leagueBrowser == null)
  {
   _leagueBrowser = new LeagueBrowserView(_bridge, async club =>
   {
    if (_busy || !CanLeave()) return;
    await Guard(() => OpenEntity("Club", club.Id), true);
   });
   LeagueScreen.Children.Add(_leagueBrowser);
  }
  SearchScreen.Visibility = ContentColumns.Visibility = Visibility.Collapsed;
  LeagueScreen.Visibility = Visibility.Visible; _returnToLeague = false; SetNavigation();
  Breadcrumb.Text = "데이터베이스  /  리그·구단"; SearchScreenButton.Content = "리그 목록";
  ReviewButton.IsEnabled = false;
  await _leagueBrowser.LoadAsync(state, refresh);
  Status("리그 소속 구단 탐색 · 구단을 두 번 클릭하면 기존 상세 화면으로 연결됩니다.");
 }
 private void DisposeLeagueBrowser()
 {
  _leagueBrowser?.Invalidate(); _leagueBrowser = null; _returnToLeague = false;
  LeagueScreen.Children.Clear(); LeagueScreen.Visibility = Visibility.Collapsed;
 }
 private async Task RefreshLeagueContextAsync()
 {
  if (_bridge == null || _leagueBrowser == null) return;
  bool visible = LeagueScreen.IsVisible; var current = _detail?.Entity; var state = _leagueBrowser.CaptureState();
  // League browsing also works for unemployed managers. The generic refresh
  // command refreshes the managed squad and requires own_club_id in live mode.
  // Refresh this catalog directly; do not clear the current page before success.
  await _leagueBrowser.LoadAsync(state, refresh: true);
  SetSummary(await _bridge.CallAsync("status"));
  if (visible) await ShowLeagueBrowserAsync(state);
  else if (current != null) await OpenEntity(current.Kind, current.Id, false);
 }
 private async Task RunLeagueBrowserQaAsync(string directory)
 {
  if (_live) throw new InvalidOperationException("리그 탐색 QA는 스냅샷으로 실행합니다.");
  await ShowLeagueBrowserAsync();
  var result = await _leagueBrowser!.QaAsync(directory);
  var state = _leagueBrowser.CaptureState();
  await RefreshCurrentAsync();
  if (!LeagueScreen.IsVisible || _leagueBrowser.CaptureState() != state) throw new InvalidOperationException("리그 새로고침이 선택을 잃었습니다.");
  if (App.Arg("--expect-no-own-club") == "true" && _ownClub != null) throw new InvalidOperationException("무소속 감독 회귀 QA fixture에 소속 구단이 있습니다.");
  var club = _leagueBrowser.QaSelectedClub ?? throw new InvalidOperationException("리그 구단 상세 QA 대상이 없습니다.");
  await OpenEntity("Club", club.Id);
  await RefreshCurrentAsync();
  if (LeagueScreen.IsVisible || !_returnToLeague || !Equals(SearchScreenButton.Content, "리그 목록으로") || _detail?.Entity.Id != club.Id) throw new InvalidOperationException("리그 → 구단 상세 연결 실패.");
  await ShowLeagueBrowserAsync();
  if (_leagueBrowser.CaptureState() != state) throw new InvalidOperationException("구단 상세에서 리그로 돌아올 때 선택이 바뀌었습니다.");
  double width = Width, height = Height;
  try
  {
   foreach (var size in new[] { (1510.0, 990.0), (1120.0, 740.0) })
   {
    Width = size.Item1; Height = size.Item2; await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
    _leagueBrowser.QaCheckBounds(); Capture(Path.Combine(directory, $"league-browser-{Width:0}x{Height:0}.png"));
   }
  }
  finally { Width = width; Height = height; }
  DisposeLeagueBrowser(); await ShowLeagueBrowserAsync(state);
  if (_leagueBrowser!.CaptureState() != state) throw new InvalidOperationException("리그 상태 재연결 복원 실패.");
  File.WriteAllText(Path.Combine(directory, "league-browser-ui.json"), JsonSerializer.Serialize(new { success = true, game_writes = false, result, club_detail = club.Id, state_restored = true, refresh_catalog_and_profile = true, own_club_id = _ownClub?.Id }, new JsonSerializerOptions { WriteIndented = true }));
  _qaChecks.Add("league_country_category_club_navigation");
 }
}
