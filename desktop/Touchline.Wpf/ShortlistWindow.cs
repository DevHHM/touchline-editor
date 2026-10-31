using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Media;
using Microsoft.Win32;

namespace Touchline.Wpf;

/// <summary>UID-based scouting lists and SI file interchange. Never writes game memory.</summary>
public sealed class ShortlistWindow : Window
{
 private readonly BridgeClient _bridge;
 private readonly EntityRecord[] _selection, _results;
 private WorkbenchLibrary _library = new();
 private readonly ComboBox _lists = new() { MinWidth = 180, DisplayMemberPath = "Name", MaxDropDownHeight = 300 };
 private readonly TextBox _name = new() { MinWidth = 170, MaxLength = 80 };
 private readonly ComboBox _exportKind = new() { MinWidth = 125, ItemsSource = new[] { "선수 명단", "스태프 명단" }, SelectedIndex = 0 };
 private readonly DataGrid _grid = new() { AutoGenerateColumns = false, IsReadOnly = true, CanUserAddRows = false, CanUserDeleteRows = false, SelectionMode = DataGridSelectionMode.Extended, SelectionUnit = DataGridSelectionUnit.FullRow, EnableRowVirtualization = true, RowHeight = 36 };
 private readonly TextBlock _status = Label("명단을 읽는 중…"), _count = Label("");
 private readonly DockPanel _body = new() { Margin = new Thickness(18) };
 private bool _busy, _refreshing, _closed;
 private int _resolveVersion;
 public EntityRecord? ProfileToOpen { get; private set; }

 public ShortlistWindow(BridgeClient bridge, EntityRecord[] selection, EntityRecord[] results)
 {
  _bridge = bridge; _selection = selection; _results = results;
  Title = "관심명단 · Touchline"; Width = 1060; Height = 740; MinWidth = 800; MinHeight = 600;
  WindowStartupLocation = WindowStartupLocation.CenterOwner;
  Background = Resource("BaseBrush"); Foreground = Resource("InkBrush"); Typography.Initialize(this);
  var top = new StackPanel();
  var title = Label("검색에서 관심명단으로"); title.FontSize = 22; title.FontWeight = FontWeights.Bold; top.Children.Add(title);
  top.Children.Add(Label("명단 변경은 Touchline에 즉시 저장됩니다. FM에는 파일을 내보낸 뒤 게임의 관심명단에서 불러오세요."));
  top.Children.Add(Bar(Field("관심명단", _lists), Field("명단 이름", _name), Button("새 명단", CreateAsync), Button("이름 변경", RenameAsync), Button("명단 삭제", DeleteAsync)));
  top.Children.Add(Bar(Button($"검색 선택 {_selection.Length:N0}명 추가", () => AddAsync(_selection)), Button($"검색 결과 전체 {_results.Length:N0}명 추가", () => AddAsync(_results)), Button("명단에서 선택 제거", RemoveAsync), Button("다시 읽기", ReloadAsync)));
  top.Children.Add(Bar(Button("FM / JSON 명단 가져오기", ImportAsync), Field("내보낼 종류", _exportKind), Button("FM용 파일 내보내기", ExportFmAsync), Button("JSON 백업", ExportAsync), Button("선택한 프로필 열기 ↗", OpenAsync)));
  var compatibility = Label("FM 파일 교환은 시험 지원입니다. 파일 구조를 검증했으며 FM27에서 실제 불러오기 확인은 아직 필요합니다. 서로 다른 세이브의 생성 선수 UID는 다를 수 있습니다.");
  compatibility.Foreground = Resource("MutedBrush"); compatibility.FontSize = 12; top.Children.Add(compatibility);
  top.Children.Add(_count); DockPanel.SetDock(top, Dock.Top); _body.Children.Add(top);
  _status.TextWrapping = TextWrapping.Wrap; _status.Margin = new Thickness(0, 10, 0, 0); DockPanel.SetDock(_status, Dock.Bottom); _body.Children.Add(_status);
  Column("이름", "Name", 190); Column("영문 이름", "EnglishName", 180); Column("종류", "Kind", 80); Column("국적", "Nationality", 100); Column("구단", "Club", 160); Column("포지션 / 직무", "Role", 110); Column("UID", "Reference.Id", 90);
  NameCopy.AttachGrid(_grid, item => (item as Member)?.Entity, message => _status.Text = message);
  _grid.MouseDoubleClick += async (_, _) => { if (_grid.SelectedItem is Member { Entity: not null }) await RunAsync(OpenAsync); };
  _body.Children.Add(_grid); Content = _body;
  _lists.SelectionChanged += async (_, _) => { if (_refreshing || _busy) return; if (_lists.SelectedItem is WorkbenchShortlist list) _name.Text = list.Name; await RunAsync(ResolveAsync); };
  Loaded += async (_, _) => await RunAsync(ReloadAsync);
  Closed += (_, _) => { _closed = true; ++_resolveVersion; };
 }
 private static Brush Resource(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Label(string text) => new() { Text = text, Margin = new Thickness(0, 3, 0, 5), TextWrapping = TextWrapping.Wrap };
 private static FrameworkElement Field(string text, FrameworkElement input) { var p = new StackPanel { Margin = new Thickness(0, 0, 8, 4) }; p.Children.Add(Label(text)); input.MinHeight = 32; p.Children.Add(input); return p; }
 private static WrapPanel Bar(params UIElement[] elements) { var bar = new WrapPanel { Margin = new Thickness(0, 8, 0, 0), VerticalAlignment = VerticalAlignment.Center }; foreach (var e in elements) bar.Children.Add(e); return bar; }
 private Button Button(string text, Func<Task> action) { var b = new Button { Content = text, Margin = new Thickness(0, 4, 8, 4), Padding = new Thickness(10, 7, 10, 7), VerticalAlignment = VerticalAlignment.Bottom }; b.Click += async (_, _) => await RunAsync(action); return b; }
 private void Column(string label, string path, double width) => _grid.Columns.Add(new DataGridTextColumn { Header = label, Binding = new Binding(path), Width = width, MinWidth = 65 });
 private static T Read<T>(JsonElement value) => value.Deserialize<T>(JsonDefaults.Options) ?? throw new InvalidOperationException("명단 응답이 비어 있습니다.");
 private async Task RunAsync(Func<Task> action)
 {
  if (_busy || _closed) return; _busy = true; _body.IsEnabled = false;
  try { await action(); }
  catch (Exception error) { App.Log(error); if (!_closed) _status.Text = error.Message; }
  finally { _busy = false; if (!_closed) _body.IsEnabled = true; }
 }
 private WorkbenchShortlist Selected() => _lists.SelectedItem as WorkbenchShortlist ?? throw new InvalidOperationException("관심명단을 선택하거나 새로 만드세요.");
 private static WorkbenchRef[] References(IEnumerable<EntityRecord> entities) => entities.Select(e => new WorkbenchRef(e.Kind, e.Id)).Distinct().ToArray();
 private void Refresh(string? name)
 {
  _refreshing = true;
  try { _lists.ItemsSource = _library.Lists; _lists.SelectedItem = _library.Lists.FirstOrDefault(l => l.Name == name) ?? _library.Lists.FirstOrDefault(); _name.Text = (_lists.SelectedItem as WorkbenchShortlist)?.Name ?? ""; }
  finally { _refreshing = false; }
 }
 private async Task ReloadAsync()
 {
  string? name = (_lists.SelectedItem as WorkbenchShortlist)?.Name;
  var library = Read<WorkbenchLibrary>(await _bridge.CallAsync("library_get")); if (_closed) return;
  _library = library; Refresh(name); await ResolveAsync();
  if (!_closed) _status.Text = "명단을 읽었습니다. Ctrl·Shift로 여러 대상을 선택할 수 있습니다.";
 }
 private async Task ResolveAsync()
 {
  int version = ++_resolveVersion;
  _grid.ItemsSource = null;
  if (_lists.SelectedItem is not WorkbenchShortlist list) { _count.Text = "새 명단을 만들어 검색 결과를 저장하세요."; return; }
  if (list.Members.Count > 0) _exportKind.SelectedIndex = list.Members.All(r => r.Kind == "Staff") ? 1 : 0;
  var found = new Dictionary<WorkbenchRef, EntityRecord>();
  string availability = "현재 자료에서 미확인";
  if (list.Members.Count > 0)
  {
   try
   {
    var response = await _bridge.CallAsync("resolve_targets", new { targets = list.Members });
    if (_closed || version != _resolveVersion) return;
    found = Read<List<EntityRecord>>(response.GetProperty("items")).ToDictionary(e => new WorkbenchRef(e.Kind, e.Id));
   }
   catch (Exception error)
   {
    App.Log(error);
    availability = "게임 연결/자료 확인 불가 · UID로 관리";
   }
  }
  if (_closed || version != _resolveVersion) return;
  _grid.ItemsSource = list.Members.Select(r => new Member(r, found.GetValueOrDefault(r))).ToArray();
  _count.Text = $"{list.Name} · {list.Members.Count:N0}명 · {availability} {list.Members.Count - found.Count:N0}명";
 }
 private async Task ChangeAsync(object change, string? selected, string message)
 {
  var library = Read<WorkbenchLibrary>(await _bridge.CallAsync("shortlist_update", new { expected = _library, change }));
  if (_closed) return; _library = library; Refresh(selected); await ResolveAsync(); if (!_closed) _status.Text = message;
 }
 private Task CreateAsync() => ChangeAsync(new { action = "create", name = _name.Text.Trim(), members = References(_selection) }, _name.Text.Trim(), $"새 명단에 검색에서 선택한 {_selection.Length:N0}명을 저장했습니다.");
 private Task RenameAsync() => ChangeAsync(new { action = "rename", name = Selected().Name, new_name = _name.Text.Trim() }, _name.Text.Trim(), "명단 이름을 바꿨습니다.");
 private Task DeleteAsync()
 {
  string name = Selected().Name;
  return AppDialog.Show(this, $"‘{name}’ 관심명단을 삭제할까요? 선수나 게임 데이터는 삭제되지 않습니다.", "명단 삭제", MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes
   ? ChangeAsync(new { action = "delete", name }, null, "관심명단을 삭제했습니다.") : Task.CompletedTask;
 }
 private Task AddAsync(EntityRecord[] people)
 {
  if (people.Length == 0) throw new InvalidOperationException("추가할 검색 결과가 없습니다. 전체 추가는 검색이 끝난 뒤 사용할 수 있습니다.");
  if (people.Length > 10000) throw new InvalidOperationException("한 명단에는 최대 10,000명까지 저장할 수 있습니다. 검색 조건을 좁혀주세요.");
  string name = Selected().Name; return ChangeAsync(new { action = "add", name, members = References(people) }, name, "검색 결과를 추가했습니다. 이미 있는 대상은 중복 저장하지 않습니다.");
 }
 private Task RemoveAsync()
 {
  var members = _grid.SelectedItems.Cast<Member>().Select(m => m.Reference).ToArray();
  if (members.Length == 0) throw new InvalidOperationException("명단에서 제거할 대상을 선택하세요.");
  string name = Selected().Name; return ChangeAsync(new { action = "remove", name, members }, name, $"관심명단에서 {members.Length:N0}명을 제거했습니다.");
 }
 private Task OpenAsync() { if (_grid.SelectedItem is not Member { Entity: { } entity }) throw new InvalidOperationException("현재 자료에서 확인된 대상을 선택하세요."); ProfileToOpen = entity; DialogResult = true; return Task.CompletedTask; }
 private async Task ExportAsync()
 {
  string name = Selected().Name;
  var dialog = new SaveFileDialog { Filter = "Touchline 관심명단 (*.json)|*.json", FileName = "touchline-shortlist.json" };
  if (dialog.ShowDialog(this) != true || _closed) return;
  await _bridge.CallAsync("library_export", new { path = dialog.FileName, name });
  _status.Text = "Touchline JSON 명단을 내보냈습니다. FM의 .fmf 영입명단 파일은 아닙니다.";
 }
 private async Task ExportFmAsync()
 {
  var list = Selected();
  string kind = _exportKind.SelectedIndex == 1 ? "Staff" : "Player";
  if (list.Members.Any(r => r.Kind != kind)) throw new InvalidOperationException("선수와 스태프는 별도 FM 명단으로 저장해야 합니다. 선택한 종류와 다른 대상이 있으므로 명단을 나누어 주세요.");
  string folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Sports Interactive", "Football Manager 27", kind == "Staff" ? "staff shortlists" : "shortlists");
  string safeName = string.Concat(list.Name.Select(c => Path.GetInvalidFileNameChars().Contains(c) ? '_' : c));
  var dialog = new SaveFileDialog { Filter = "FM 관심명단 (*.fmf)|*.fmf", FileName = safeName + ".fmf", DefaultExt = ".fmf", AddExtension = true };
  if (Directory.Exists(folder)) dialog.InitialDirectory = folder;
  if (dialog.ShowDialog(this) != true || _closed) return;
  var response = await _bridge.CallAsync("fm_shortlist_export", new { path = dialog.FileName, name = list.Name, kind, expected = _library });
  if (_closed) return;
  _status.Text = $"파일 저장 완료: {dialog.FileName}\nFM의 {(kind == "Staff" ? "스태프" : "선수")} 관심명단에서 이 파일을 불러오세요. 게임 반영은 아직 되지 않았습니다. FM27 불러오기는 확인이 필요합니다.";
 }
 private async Task ImportAsync()
 {
  var dialog = new OpenFileDialog { Filter = "모든 관심명단 (*.fmf;*.slf;*.ssf;*.json)|*.fmf;*.slf;*.ssf;*.json|FM 관심명단 (*.fmf;*.slf;*.ssf)|*.fmf;*.slf;*.ssf|Touchline 백업 (*.json)|*.json" };
  if (dialog.ShowDialog(this) != true || _closed) return;
  string command = Path.GetExtension(dialog.FileName).Equals(".json", StringComparison.OrdinalIgnoreCase) ? "library_import" : "fm_shortlist_import";
  var response = await _bridge.CallAsync(command, new { path = dialog.FileName }); if (_closed) return;
  var incoming = Read<WorkbenchLibrary>(response.GetProperty("library"));
  if (incoming.Lists.Count == 0) throw new InvalidOperationException("파일에 관심명단이 없습니다.");
  var summary = string.Join("\n", incoming.Lists.Take(8).Select(l => $"• {l.Name}: {l.Members.Count:N0}명"));
  if (incoming.Lists.Count > 8) summary += $"\n외 {incoming.Lists.Count - 8}개";
  if (AppDialog.Show(this, $"{summary}\n\nTouchline으로 가져올까요? 같은 이름의 명단은 대상을 합치며, 중복 UID는 한 번만 저장합니다. FM 내부 명단은 변경하지 않습니다.", "관심명단 가져오기", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
  // Preserve existing lists and presets; duplicates merge by stable entity identity.
  var merged = Read<WorkbenchLibrary>(JsonSerializer.SerializeToElement(_library, JsonDefaults.Options));
  foreach (var list in incoming.Lists)
  {
   var existing = merged.Lists.FirstOrDefault(l => l.Name.Equals(list.Name, StringComparison.OrdinalIgnoreCase));
   if (existing == null) merged.Lists.Add(list); else existing.Members = existing.Members.Concat(list.Members).Distinct().ToList();
  }
  var updated = Read<WorkbenchLibrary>(await _bridge.CallAsync("library_put", new { expected = _library, library = merged }));
  if (_closed) return; _library = updated; Refresh(incoming.Lists[0].Name); await ResolveAsync();
  _status.Text = "관심명단을 합쳐 저장했습니다. 기존 명단과 프리셋은 유지됩니다.";
 }
 public sealed record Member(WorkbenchRef Reference, EntityRecord? Entity)
 {
  public string Name => Entity?.DisplayName ?? "현재 자료에서 미확인";
  public string EnglishName => Entity?.EnglishName ?? "";
  public string Kind => Entity?.KindLabel ?? Reference.Kind;
  public string Nationality => Entity?.NationalityLabel ?? "—";
  public string Club => Entity?.Club ?? "—";
  public string Role => Entity?.Role ?? "—";
 }
}
