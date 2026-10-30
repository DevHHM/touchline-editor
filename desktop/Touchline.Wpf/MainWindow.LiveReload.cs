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
 private sealed record LiveReloadContext(string Kind, uint? EntityId, string Tab, bool SearchVisible, SearchReloadState? Search);

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
   if (_closing) return;
   var decision = _liveReload.Observe(observation);
   if (decision.Stop) { _liveWatchTimer.Stop(); return; }
   if (decision.Invalidate) await InvalidateLoadedSessionAsync(observation.Reason);
   if (decision.RestoreConnected)
   {
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
    catch (Exception recoveryError) { App.Log(recoveryError); Status("엔진 재연결을 다시 시도합니다 · " + recoveryError.Message); }
   }
   else if (!_closing) Status("자동 연결 확인을 다시 시도합니다 · " + error.Message);
   _nextLiveReconnect = DateTime.UtcNow.AddSeconds(10);
  }
  finally { _livePollBusy = false; }
 }

 private void ShowLiveWait(LiveConnectionObservation observation)
 {
  ConnectionLabel.Text = observation.State == "waiting" ? "○  FM 실행 대기 · 자동 연결" : "○  세이브 로드 감지 · 다시 연결 대기";
  ConnectionLabel.Foreground = ResourceBrush("GoldBrush");
  Status(string.IsNullOrWhiteSpace(observation.Reason) ? "새 게임 데이터가 준비되면 자동으로 다시 연결합니다." : observation.Reason);
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
  ++_liveViewEpoch;
  _reloadContext ??= new(_detail?.Entity.Kind ?? _kind, _detail?.Entity.Id, _tab, SearchScreen.IsVisible, _searchView?.CaptureReloadState());
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
  _liveReconnectBusy = true; SetBusy(true, "새 세이브를 자동으로 불러오는 중…");
  try
  {
   var observation = Decode<LiveConnectionObservation>(await _bridge.CallAsync("connection_reconnect")) ?? throw new InvalidDataException("자동 재연결 응답을 읽을 수 없습니다.");
   if (_closing) return;
   _liveReload.Observe(observation);
   if (observation.State != "reconnected" || observation.Summary.ValueKind != JsonValueKind.Object)
   {
    ShowLiveWait(observation); _nextLiveReconnect = DateTime.UtcNow.AddSeconds(10); return;
   }
   await RestoreReloadedSummaryAsync(observation.Summary);
  }
  finally { _liveReconnectBusy = false; SetBusy(false); }
 }

 private async Task RestoreReloadedSummaryAsync(JsonElement summary)
 {
  var context = _reloadContext; _reloadContext = null;
  SetSummary(summary);
  _kind = context?.Kind ?? _kind; SetNavigation();
  if (context?.SearchVisible == true)
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
  Status("새 세이브를 자동으로 불러왔습니다 · 이전 초안과 미리보기는 재사용하지 않았습니다." + (_reloadDraftPath == null ? "" : " 폐기 초안: " + _reloadDraftPath));
 }

 private async Task RecoverExitedEngineAsync()
 {
  var previous = _bridge;
  if (previous == null || !previous.HasExited) return;
  // Never kill or replace a live worker whose accepted write may be unfinished.
  await InvalidateLoadedSessionAsync("Rust 엔진이 종료되어 자료를 다시 불러옵니다.");
  _liveReconnectBusy = true; SetBusy(true, "엔진 다시 연결 중…");
  try
  {
   await previous.DisposeAsync();
   _bridge = new BridgeClient(EnginePath());
   _bridge.Diagnostic += message => Dispatcher.BeginInvoke(() => Status(message));
   await _bridge.CallAsync("status");
   EnableLiveWatch();
   // A failed connect arms the new backend's load monitor for a later retry.
   var summary = await _bridge.CallAsync("connect");
   await RestoreReloadedSummaryAsync(summary);
  }
  finally { _liveReconnectBusy = false; SetBusy(false); }
 }
}
