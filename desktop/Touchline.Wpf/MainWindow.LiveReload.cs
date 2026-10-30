using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private readonly DispatcherTimer _liveWatchTimer = new() { Interval = TimeSpan.FromSeconds(2) };
 private readonly LiveReloadState _liveReload = new();
 private bool _livePollBusy, _liveReconnectBusy;
 private long _liveViewEpoch;
 private DateTime _nextLiveReconnect;
 private Window? _scalarReviewWindow;
 private LiveReloadContext? _reloadContext;
 private string? _reloadDraftPath;
 private sealed record LiveReloadContext(string Kind, uint? EntityId, string Tab, bool SearchVisible, SearchReloadState? Search, bool LeagueVisible, bool LeagueOrigin, LeagueBrowserState? League);

 private void InitializeLiveWatch() => _liveWatchTimer.Tick += async (_, _) => await PollLiveConnectionAsync();
 private void EnableLiveWatch()
 {
  if (_qaRunning || _closing) return;
  _liveReload.Enable(); _nextLiveReconnect = DateTime.MinValue; _liveWatchTimer.Start();
 }
 private void DisableLiveWatch() { _liveWatchTimer.Stop(); _liveReload.Disable(); }

 private async Task PollLiveConnectionAsync()
 {
  if (!_liveReload.Enabled || !_ready || _bridge == null || _closing || _qaRunning || _livePollBusy || _busy ||
      _relationsView?.CanPollConnection == false || _rosterView?.CanPollConnection == false || _toolWindows.Any(window => window.IsBusy) || _workbenches.Any(window => window.IsBusy)) return;
  _livePollBusy = true;
  try
  {
   var observation = Decode<LiveConnectionObservation>(await _bridge.CallAsync("connection_poll")) ?? throw new InvalidDataException("연결 상태 응답을 읽을 수 없습니다.");
   LogConnectionState(observation.State,observation.Reason);
   if (_closing) return;
   var decision = _liveReload.Observe(observation);
   if (decision.Stop) { _liveWatchTimer.Stop(); return; }
   if (decision.Invalidate) await InvalidateLoadedSessionAsync(observation.Reason);
   if (ConnectionPresentation.IsProblem(observation.State)) { ShowConnectionProblem(observation.State, observation.Reason); return; }
   if (decision.RestoreConnected)
   {
    if(DateTime.UtcNow<_nextLiveReconnect)return;
    if (observation.Summary.ValueKind != JsonValueKind.Object) throw new InvalidDataException("다시 연결된 세이브의 요약을 읽을 수 없습니다.");
    _liveReconnectBusy = true; SetBusy(true, "현재 세이브 화면을 다시 불러오는 중…");
    try { await RestoreReloadedSummaryAsync(observation.Summary); }
    finally { _liveReconnectBusy = false; SetBusy(false); }
    return;
   }
   if (observation.State is "loading" or "waiting" or "ready") ShowLiveWait(observation);
   if (decision.RefreshDate && observation.Summary.ValueKind == JsonValueKind.Object)
   {
    // Advancing a day is not a save reload. Keep edits and the selected profile.
    GameDateLabel.Text = $"{observation.GameDate ?? "—"}  ·  LIVE";
   }
   if (decision.Reconnect && DateTime.UtcNow >= _nextLiveReconnect) await ReconnectLoadedSessionAsync();
  }
  catch (Exception error)
  {
   App.Log(error);
   if (!_closing && _bridge?.HasExited == true)
   {
    try { await RecoverExitedEngineAsync(); }
    catch (Exception recoveryError) { App.Log(recoveryError); ShowConnectionProblem(_ready?"error":"engine_error", recoveryError.Message); }
   }
   else if (!_closing) ShowConnectionProblem("error", error.Message);
   _nextLiveReconnect = DateTime.UtcNow.AddSeconds(10);
  }
  finally { _livePollBusy = false; }
 }

 private void ShowLiveWait(LiveConnectionObservation observation)
 {
  LogConnectionState(observation.State,observation.Reason);
  ConnectionLabel.Text = observation.State == "waiting" ? "○  FM 실행 대기 · 자동 연결" : observation.State == "ready" ? "○  세이브 확인 완료 · 연결 준비" : "○  세이브 로드 대기 · 자동 연결";
  ConnectionLabel.Foreground = ResourceBrush("GoldBrush");
  Status(string.IsNullOrWhiteSpace(observation.Reason) ? "새 게임 데이터가 준비되면 자동으로 다시 연결합니다." : observation.Reason);
  if(!_dataLoaded&&_connectionProblemPanel!=null&&DetailHost.Children.Contains(_connectionProblemPanel)){
   DetailHost.Children.Clear();_connectionProblemPanel=null;_connectionProblemKey=null;
   DetailHost.Children.Add(Text("FM 연결을 기다리고 있습니다.",23,bold:true));
   var hint=Text(string.IsNullOrWhiteSpace(observation.Reason)?"FM에서 세이브 로딩이 끝나면 자동으로 연결합니다.":observation.Reason,14);hint.Margin=new Thickness(0,16,0,0);DetailHost.Children.Add(hint);
  }
 }

 private string? PreserveAbandonedDraft()
 {
  var dirty = _detail?.Fields.Where(field => field.IsDirty).Select(field => new { field.Key, field.Label, before = field.Original, draft = field.Value }).ToArray();
  bool relationDraft = _relationsView?.HasPendingChanges == true;
  if ((dirty == null || dirty.Length == 0) && !relationDraft) return null;
  try
  {
   var directory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "DiscardedDrafts");
   Directory.CreateDirectory(directory);
   var path = Path.Combine(directory, DateTime.UtcNow.ToString("yyyyMMdd-HHmmss-fffffff") + ".json");
   File.WriteAllText(path, JsonSerializer.Serialize(new
   {
    reason = "FM 세이브 변경으로 폐기된 초안입니다. 자동 적용하거나 새 세이브의 원본으로 사용하지 않습니다.",
    game_date = GameDateLabel.Text, entity = _detail?.Entity, fields = dirty,
    relationship_draft = relationDraft ? _relationsView?.CapturePendingDraft() : null,
   }, new JsonSerializerOptions(JsonDefaults.Options) { WriteIndented = true }));
   return path;
  }
  catch (Exception error) { App.Log(error); return null; }
 }

 private async Task InvalidateLoadedSessionAsync(string reason)
 {
  _liveReload.InvalidateView();
  ++_liveViewEpoch;
  _reloadContext ??= new(_detail?.Entity.Kind ?? _kind, _detail?.Entity.Id, _tab, SearchScreen.IsVisible, _searchView?.CaptureReloadState(), LeagueScreen.IsVisible, _returnToLeague, _leagueBrowser?.CaptureState());
  _reloadDraftPath = PreserveAbandonedDraft();
  _scalarReviewWindow?.Close(); _scalarReviewWindow = null;
  _searchTimer.Stop(); _scoreTimer.Stop(); ++_searchVersion;
  foreach (var workbench in _workbenches.ToArray()) workbench.InvalidateConnection();
  foreach (var tool in _toolWindows.ToArray()) tool.InvalidateConnection();
  _searchView?.InvalidateResults();
  await DisposeSearchView(); await StopDetailViewsAsync(); ClearDetail();
  _dataLoaded = _live = false; _ownClub = null;
  GameEditionText.Text = "FM"; GameEditionText.ToolTip = "새 게임 프로필을 확인하고 있습니다.";
  SearchScreenButton.IsEnabled = false;
  _routes.Clear(); _routeIndex = -1; _offset = _total = 0; _listKind = null;
  EntityGrid.ItemsSource = null; ResultCount.Text = "0명"; PageLabel.Text = "0 / 0";
  BackButton.IsEnabled = ForwardButton.IsEnabled = PrevPage.IsEnabled = NextPage.IsEnabled = false;
  OwnBadge.Source = null; OwnClubLabel.Text = "새 세이브 확인 중"; GameDateLabel.Text = "—  ·  LOADING"; Breadcrumb.Text = "세이브 변경 감지";
  DetailHost.Children.Clear();
  DetailHost.Children.Add(Text("세이브가 바뀌어 이전 자료와 변경 미리보기를 비웠습니다.", 20));
  DetailHost.Children.Add(Text("FM의 로딩이 끝나면 새 선수단과 구단을 자동으로 다시 불러옵니다. 이전 초안은 새 세이브에 적용하지 않습니다.", 13));
  if (_reloadDraftPath != null) DetailHost.Children.Add(Text("폐기된 초안 보관: " + _reloadDraftPath, 11));
  Status(reason);
 }

 private async Task ReconnectLoadedSessionAsync()
 {
  if (_bridge == null) return;
  DiagnosticsLog.Current?.Write("connection","reconnect_attempt");
  _liveReconnectBusy = true; SetBusy(true, "새 세이브를 자동으로 불러오는 중…");
  try
  {
   var observation = Decode<LiveConnectionObservation>(await _bridge.CallAsync("connection_reconnect")) ?? throw new InvalidDataException("자동 재연결 응답을 읽을 수 없습니다.");
   if (_closing) return;
   _liveReload.Observe(observation);
   if (observation.State != "reconnected" || observation.Summary.ValueKind != JsonValueKind.Object)
   {
    if(ConnectionPresentation.IsProblem(observation.State))ShowConnectionProblem(observation.State,observation.Reason);else ShowLiveWait(observation);
    _nextLiveReconnect = DateTime.UtcNow.AddSeconds(10); return;
   }
   await RestoreReloadedSummaryAsync(observation.Summary);
  }
  finally { _liveReconnectBusy = false; SetBusy(false); }
 }

 private async Task RestoreReloadedSummaryAsync(JsonElement summary)
 {
  try {await RestoreReloadedSummaryCoreAsync(summary);DiagnosticsLog.Current?.Write("connection","view_restored");}
  catch {
   // SetSummary restarts the watcher before rendering. If rendering fails,
   // retain a pending view restore even though the backend is still connected.
   _liveReload.InvalidateView();_dataLoaded=_live=false;SearchScreenButton.IsEnabled=false;
   throw;
  }
 }

 private async Task RestoreReloadedSummaryCoreAsync(JsonElement summary)
 {
  var context = _reloadContext;
  SetSummary(summary);
  _kind = context?.Kind ?? _kind; SetNavigation();
  if (context?.LeagueVisible == true || context?.LeagueOrigin == true)
  {
   await ShowLeagueBrowserAsync(context.League);
   if (!context.LeagueVisible && context.EntityId is uint leagueEntityId)
   {
    try { await OpenEntity(context.Kind, leagueEntityId, false); _tab = context.Tab; await RenderDetail(); }
    catch (BridgeException) { ClearDetail(); await ShowLeagueBrowserAsync(context.League); }
   }
  }
  else if (context?.SearchVisible == true)
  {
   await ShowSearchAsync(context.Kind);
   if (context.Search != null && _searchView != null) await _searchView.RestoreReloadStateAsync(context.Search);
  }
  else
  {
   await SearchAsync();
   if (context?.EntityId is uint id)
   {
    try { await OpenEntity(context.Kind, id, false); _tab = context.Tab; await RenderDetail(); }
    catch (BridgeException) { ClearDetail(); await ShowSearchAsync(context.Kind); }
   }
   else await ShowSearchAsync(_kind);
  }
  _reloadContext=null;
  Status("새 세이브를 자동으로 불러왔습니다 · 이전 초안과 미리보기는 재사용하지 않았습니다." + (_reloadDraftPath == null ? "" : " 폐기 초안: " + _reloadDraftPath));
 }

 private async Task RecoverExitedEngineAsync()
 {
  var previous = _bridge;
  if (previous == null || !previous.HasExited) return;
  DiagnosticsLog.Current?.Write("connection","engine_recovery_attempt");
  // Never kill or replace a live worker whose accepted write may be unfinished.
  await InvalidateLoadedSessionAsync("Rust 엔진이 종료되어 자료를 다시 불러옵니다.");
  _liveReconnectBusy = true; SetBusy(true, "엔진 다시 연결 중…");
  try
  {
   await EnsureEngineReadyAsync();
   EnableLiveWatch();
   // A failed connect arms the new backend's load monitor for a later retry.
   var summary = await _bridge!.CallAsync("connect");
   await RestoreReloadedSummaryAsync(summary);
  }
  finally { _liveReconnectBusy = false; SetBusy(false); }
 }
}
