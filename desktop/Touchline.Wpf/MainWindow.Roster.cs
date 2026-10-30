using System.IO;
using System.Text.Json;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private ClubRosterView? _rosterView;
 private ClubGrowthAvailability _growthAvailability = ClubGrowthAvailability.Unavailable;
 private readonly Dictionary<uint, string> _rosterSelections = [];

 private async Task StopRosterAsync()
 {
  var view = _rosterView; _rosterView = null;
  if (view != null) await view.StopAsync();
 }

 private async Task RenderClubRosterAsync(DetailRecord detail, int renderVersion)
 {
  if (_bridge == null) return;
  _rosterSelections.TryGetValue(detail.Entity.Id, out string? selected);
  var view = new ClubRosterView(_bridge, detail.Entity, _live, async target =>
  {
   if (!_busy && CanLeave()) await Guard(() => OpenEntity(target.Kind, target.Id), true);
  }, selected, _growthAvailability);
  view.GroupChanged += key => _rosterSelections[detail.Entity.Id] = key;
  _rosterView = view; DetailHost.Children.Add(view); await view.LoadAsync();
  if (renderVersion != _renderVersion || _detail != detail) view.Invalidate();
 }

 private async Task RunClubRosterQaAsync(string directory)
 {
  if (_live) throw new InvalidOperationException("선수단 화면 QA는 스냅샷에서 실행합니다.");
  if (_detail?.Entity.Kind != "Club") throw new InvalidOperationException("구단 상세가 필요합니다.");
  _tab = "선수단"; await RenderDetail();
  if (_rosterView == null) throw new InvalidOperationException("구단 선수단 화면이 연결되지 않았습니다.");
  var report = await _rosterView.QaAsync(directory);
  await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  Capture(Path.Combine(directory, "main-club-roster.png"));
  File.WriteAllText(Path.Combine(directory, "club-roster-ui.json"), JsonSerializer.Serialize(new { success = true, live = false, game_writes = false, report }, new JsonSerializerOptions { WriteIndented = true }));
  _qaChecks.Add("club_roster_actual_squad_filters");
 }
}
