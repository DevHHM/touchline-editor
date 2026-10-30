using System.ComponentModel;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Media;

namespace Touchline.Wpf;

/// <summary>Reviewed display-name file installation. Never renames FM memory or closes the game.</summary>
public sealed class NameLocalizationWindow : Window
{
 private readonly BridgeClient _bridge;
 private readonly uint[] _ids;
 private readonly CancellationTokenSource _lifetime = new();
 private readonly DataGrid _grid = Table(), _history = Table();
 private readonly TextBlock _status = Text("선수 이름과 설치 경로를 확인하는 중…"), _coverage = Text(""), _installReason = Text(""), _reviewHint = Text("");
 private readonly TextBlock _originalNames = Text("선수를 선택하면 원래 이름·성·통용명을 확인할 수 있습니다.", 11);
 private readonly StackPanel _review = new();
 private readonly ScrollViewer _editScroll = new() { VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
 private readonly Border _reviewPanel;
 private readonly Button _fill = Button("추천 이름 채우기"), _selectAll = Button("전체 선택"), _clear = Button("선택 해제"), _refresh = Button("다시 읽기"), _previewButton = Button("선택한 변경 미리보기"), _apply = Button("검토한 한글화 파일 설치"), _historyRefresh = Button("설치 기록 새로 읽기"), _undo = Button("선택한 설치 되돌리기");
 private readonly CheckBox _confirmed = new() { Content = "변경 이름과 파일 경로를 확인했습니다", Margin = new Thickness(0, 10, 0, 10), Foreground = Brush("InkBrush") };
 private readonly TextBox _historyDetail = new() { IsReadOnly = true, TextWrapping = TextWrapping.Wrap, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, MinHeight = 80, MaxHeight = 180, Margin = new Thickness(0, 8, 0, 8) };
 private readonly TextBlock _targets = Text("");
 private NameLocalizationContext? _context;
 private NameLocalizationPreview? _preview;
 private TaskCompletionSource? _inflight;
 private bool _busy, _closed, _suppress, _loaded, _invalidated;
 private int _version;
 private string _baseline = "";
 public bool FilesChanged { get; private set; }
 public bool IsBusy => _busy;
 public bool HasPendingChanges => _preview != null || Fingerprint() != _baseline;

 public void InvalidateConnection()
 {
  if (_closed || _invalidated) return;
  if (HasPendingChanges && _context != null)
  {
   try
   {
    var directory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "DiscardedDrafts");
    Directory.CreateDirectory(directory);
    var path = Path.Combine(directory, "names-" + DateTime.UtcNow.ToString("yyyyMMdd-HHmmss-fffffff") + ".json");
    File.WriteAllText(path, JsonSerializer.Serialize(new
    {
     reason = "FM 세이브 변경으로 폐기된 이름 초안 · 자동 적용하지 않음",
     entries = _context.Rows.Select(row => new { row.Id, row.Name, row.EnglishName, row.Selected, row.KoreanName })
    }, new JsonSerializerOptions(JsonDefaults.Options) { WriteIndented = true }));
    System.Diagnostics.Trace.WriteLine("세이브 변경 · 이름 초안 보관: " + path);
   }
   catch (Exception error) { App.Log(error); }
  }
  _invalidated = true; ++_version; ClearPreview(); UpdateControls();
  _status.Text = "세이브가 바뀌어 이전 이름 목록과 미리보기를 폐기했습니다.";
  // A file transaction already accepted by the engine must finish; never cancel it.
  if (!_busy) Close();
 }

 public NameLocalizationWindow(BridgeClient bridge, IEnumerable<uint> playerIds, string scope)
 {
  _bridge = bridge; _ids = playerIds.Distinct().ToArray();
  if (_ids.Length is < 1 or > 200 || _ids.Contains(0u)) throw new ArgumentException("한 번에 1–200명의 선수를 선택하세요. 선수단 범위를 좁혀 다시 열 수 있습니다.");
  Title = "선수명 한글화 · " + scope; Width = 1120; Height = 800; MinWidth = 780; MinHeight = 580; WindowStartupLocation = WindowStartupLocation.CenterOwner; ShowInTaskbar = false;
  Background = Brush("BaseBrush"); Foreground = Brush("InkBrush"); Typography.Initialize(this);
  var shell = new DockPanel { Margin = new Thickness(24), Background = Background }; Content = shell;
  var header = new StackPanel(); header.Children.Add(Text("선수명 한글화", 24, true)); header.Children.Add(Text(scope + " · 선수의 원래 이름을 보존하고 FM 표시 이름만 한글로 지정합니다.", 13)); header.Children.Add(_coverage); DockPanel.SetDock(header, Dock.Top); shell.Children.Add(header);
  var bottom = new DockPanel { Margin = new Thickness(0, 12, 0, 0) }; var close = Button("닫기"); close.Click += (_, _) => Close(); DockPanel.SetDock(close, Dock.Right); bottom.Children.Add(close); bottom.Children.Add(_status); DockPanel.SetDock(bottom, Dock.Bottom); shell.Children.Add(bottom);
  var tabs = new TabControl(); shell.Children.Add(tabs);
  var edit = new StackPanel { Margin = new Thickness(0, 12, 6, 0) }; _editScroll.Content = edit; tabs.Items.Add(new TabItem { Header = "이름 한글화", Content = _editScroll });
  var top = new StackPanel(); top.Children.Add(Panel(Text("한글 표시명 파일을 설치합니다. FM을 재시작하고 세이브를 다시 불러온 뒤 확인하세요. 현재 실행 중인 게임의 이름은 즉시 바뀌지 않습니다.", 12)));
  var commands = new WrapPanel(); foreach (var button in new[] { _fill, _selectAll, _clear, _refresh, _previewButton }) commands.Children.Add(button); top.Children.Add(commands); top.Children.Add(_installReason);
  top.Children.Add(Details("설치 경로 확인", _targets)); edit.Children.Add(top);
  _reviewPanel = Panel(_review); _reviewPanel.Visibility = Visibility.Collapsed;
  AddEditColumns(); _grid.Height = Math.Min(280, 42 + 45 * _ids.Length); edit.Children.Add(_grid); _originalNames.Margin = new Thickness(0, 9, 0, 10); edit.Children.Add(_originalNames); edit.Children.Add(_reviewPanel);
  _grid.SelectionChanged += (_, _) => _originalNames.Text = _grid.SelectedItem is NameLocalizationRow row ? $"원본 유지 · 이름 {row.FirstName} / 성 {row.LastName} / 통용명 {(row.CommonName.Length == 0 ? "없음" : row.CommonName)}" + (string.IsNullOrWhiteSpace(row.InstalledName) ? "" : $"\n이미 설치한 표시명: {row.InstalledName} · 현재 FM 표기와 다르면 재시작 후 확인하세요.") : "선수를 선택하면 원래 이름·성·통용명을 확인할 수 있습니다.";
  var history = new DockPanel { Margin = new Thickness(0, 12, 0, 0) }; tabs.Items.Add(new TabItem { Header = "설치 기록", Content = history });
  var historyTop = new StackPanel(); historyTop.Children.Add(Text("한글화 파일의 설치와 되돌림 기록입니다. 되돌린 뒤에도 FM 재시작과 세이브 다시 불러오기가 필요합니다.")); var historyActions = new WrapPanel(); historyActions.Children.Add(_historyRefresh); historyActions.Children.Add(_undo); historyTop.Children.Add(historyActions); DockPanel.SetDock(historyTop, Dock.Top); history.Children.Add(historyTop); DockPanel.SetDock(_historyDetail, Dock.Bottom); history.Children.Add(_historyDetail); history.Children.Add(_history);
  Column(_history, "상태", nameof(NameLocalizationReceipt.StatusLabel), 140); Column(_history, "선수", nameof(NameLocalizationReceipt.Count), 60); Column(_history, "기록", nameof(NameLocalizationReceipt.Id), 2, true); Column(_history, "시각", nameof(NameLocalizationReceipt.TimeLabel), 170);
  _history.SelectionChanged += (_, _) => { _historyDetail.Text = (_history.SelectedItem as NameLocalizationReceipt)?.Detail ?? ""; UpdateControls(); };
  _fill.Click += (_, _) => FillSuggestions(); _selectAll.Click += (_, _) => SelectRows(true); _clear.Click += (_, _) => SelectRows(false);
  _refresh.Click += async (_, _) => { if (!ConfirmDiscard()) return; await RunAsync(LoadCoreAsync); };
  _previewButton.Click += async (_, _) => await RunAsync(PreviewAsync); _apply.Click += async (_, _) => await ApplyAsync(); _historyRefresh.Click += async (_, _) => await RunAsync(LoadHistoryAsync); _undo.Click += async (_, _) => await UndoAsync();
  _confirmed.Checked += (_, _) => UpdateControls(); _confirmed.Unchecked += (_, _) => UpdateControls();
  Loaded += async (_, _) => { if (!_loaded) await LoadAsync(); }; Closing += OnClosing; Closed += (_, _) => { _closed = true; _lifetime.Cancel(); UnsubscribeRows(); _lifetime.Dispose(); };
  UpdateControls();
 }
 private static Brush Brush(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Text(string text, double size = 12, bool bold = false) => new() { Text = text, FontSize = size, FontWeight = bold ? FontWeights.SemiBold : FontWeights.Medium, Foreground = Brush(bold ? "InkBrush" : "MutedBrush"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 0, 8) };
 private static Button Button(string text) => new() { Content = text, Margin = new Thickness(0, 0, 8, 6), Padding = new Thickness(11, 7, 11, 7) };
 private static Border Panel(UIElement child) => new() { Child = child, Padding = new Thickness(14), Background = Brush("PanelBrush"), BorderBrush = Brush("BorderBrush"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(8), Margin = new Thickness(0, 0, 0, 10) };
 private static FrameworkElement Details(string title, UIElement body, bool expanded = false)
 {
  var panel = new StackPanel(); var toggle = Button(title + (expanded ? " ▾" : " ▸")); toggle.HorizontalAlignment = HorizontalAlignment.Left; toggle.Background = Brushes.Transparent; toggle.BorderThickness = new Thickness(0); toggle.Padding = new Thickness(0, 4, 8, 4);
  var content = Panel(body); content.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed; toggle.Click += (_, _) => { bool show = content.Visibility != Visibility.Visible; content.Visibility = show ? Visibility.Visible : Visibility.Collapsed; toggle.Content = title + (show ? " ▾" : " ▸"); };
  panel.Children.Add(toggle); panel.Children.Add(content); return panel;
 }
 private static DataGrid Table() => new() { AutoGenerateColumns = false, IsReadOnly = true, CanUserAddRows = false, CanUserDeleteRows = false, RowHeight = 45, HeadersVisibility = DataGridHeadersVisibility.Column, SelectionMode = DataGridSelectionMode.Single, EnableRowVirtualization = true, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
 private static T Read<T>(JsonElement value) => value.Deserialize<T>(JsonDefaults.Options) ?? throw new InvalidDataException("선수명 한글화 응답을 읽지 못했습니다.");
 private static void Column(DataGrid grid, string label, string property, double width, bool star = false) => grid.Columns.Add(new DataGridTextColumn { Header = label, Binding = new Binding(property), Width = new DataGridLength(width, star ? DataGridLengthUnitType.Star : DataGridLengthUnitType.Pixel), MinWidth = star ? 100 : width });
 private void AddEditColumns()
 {
  var check = new FrameworkElementFactory(typeof(CheckBox)); check.SetBinding(CheckBox.IsCheckedProperty, new Binding(nameof(NameLocalizationRow.Selected)) { Mode = BindingMode.TwoWay, UpdateSourceTrigger = UpdateSourceTrigger.PropertyChanged }); check.SetBinding(IsEnabledProperty, new Binding(nameof(NameLocalizationRow.Editable))); check.SetValue(HorizontalAlignmentProperty, HorizontalAlignment.Center); check.SetValue(VerticalAlignmentProperty, VerticalAlignment.Center); check.SetValue(WidthProperty, 22d); check.SetValue(HeightProperty, 22d); check.SetValue(Control.PaddingProperty, new Thickness(0));
  _grid.Columns.Add(new DataGridTemplateColumn { Header = "적용", CellTemplate = new DataTemplate { VisualTree = check }, Width = 64, MinWidth = 64 });
  Column(_grid, "현재 FM 표시명", nameof(NameLocalizationRow.Name), 1.3, true); _grid.Columns[^1].MinWidth = 145; Column(_grid, "영문 이름", nameof(NameLocalizationRow.EnglishName), 1.4, true); _grid.Columns[^1].MinWidth = 150;
  var input = new FrameworkElementFactory(typeof(TextBox)); input.SetBinding(TextBox.TextProperty, new Binding(nameof(NameLocalizationRow.KoreanName)) { Mode = BindingMode.TwoWay, UpdateSourceTrigger = UpdateSourceTrigger.PropertyChanged }); input.SetBinding(IsEnabledProperty, new Binding(nameof(NameLocalizationRow.Editable))); input.SetBinding(ToolTipProperty, new Binding(nameof(NameLocalizationRow.OriginalParts))); input.SetValue(TextBox.MaxLengthProperty, 80); input.SetValue(MarginProperty, new Thickness(0, 3, 5, 3)); input.SetValue(VerticalContentAlignmentProperty, VerticalAlignment.Center);
  _grid.Columns.Add(new DataGridTemplateColumn { Header = "한글 표시명 · 직접 수정", CellTemplate = new DataTemplate { VisualTree = input }, Width = new DataGridLength(1.7, DataGridLengthUnitType.Star), MinWidth = 160 });
  Column(_grid, "추천 근거 / 확인 사항", nameof(NameLocalizationRow.SourceLabel), 1.5, true); _grid.Columns[^1].MinWidth = 160; Column(_grid, "UID", nameof(NameLocalizationRow.Id), 100);
 }
 public Task LoadAsync() => RunAsync(LoadCoreAsync);
 private void UnsubscribeRows() { if (_context != null) foreach (var row in _context.Rows) row.PropertyChanged -= RowChanged; }
 private async Task LoadCoreAsync()
 {
  ClearPreview(); UnsubscribeRows(); _context = null; _grid.ItemsSource = null; _loaded = false; _baseline = ""; UpdateControls();
  var context = Read<NameLocalizationContext>(await _bridge.CallAsync("name_localization_context", new { player_ids = _ids }, _lifetime.Token));
  if (_closed) return;
  if (!context.Rows.Select(row => row.Id).Order().SequenceEqual(_ids.Order())) throw new InvalidDataException("요청한 선수와 응답 명단이 다릅니다. 다시 읽어 주세요.");
  _context = context; _suppress = true;
  foreach (var row in context.Rows) { row.Selected = false; row.KoreanName = row.InstalledName ?? (ContainsHangul(row.Name) ? row.Name : ""); row.PropertyChanged += RowChanged; }
  _grid.ItemsSource = context.Rows; if (context.Rows.Count > 0) _grid.SelectedIndex = 0; _suppress = false; _loaded = true; _baseline = Fingerprint();
  _coverage.Text = $"{context.Rows.Count:N0}명 · 원래 이름·성 유지 · 변경할 선수만 선택"; _installReason.Text = context.CanInstall ? "추천 이름은 기존 한글 이름과 한글화 자료에서만 가져옵니다. 추천이 없는 선수는 직접 입력하세요." : context.Reason;
  _targets.Text = context.Targets.Count == 0 ? "설치 가능한 경로가 없습니다." : string.Join("\n", context.Targets.Select(target => target.Path));
  _status.Text = context.CanInstall ? "한글 표시명을 작성하고 변경 전후를 미리 확인하세요." : "파일 설치 불가 · " + context.Reason;
  await LoadHistoryAsync();
 }
 private static bool ContainsHangul(string text) => text.Any(character => character is >= '\uAC00' and <= '\uD7A3');
 private string Fingerprint() => _context == null ? "" : JsonSerializer.Serialize(_context.Rows.Select(row => new { row.Id, row.Selected, row.KoreanName }));
 private void RowChanged(object? sender, PropertyChangedEventArgs args) { if (_suppress) return; if (args.PropertyName == nameof(NameLocalizationRow.KoreanName) && sender is NameLocalizationRow row && row.Editable && !string.IsNullOrWhiteSpace(row.KoreanName)) row.Selected = true; ++_version; ClearPreview(); UpdateControls(); }
 private void ClearPreview() { _preview = null; _confirmed.IsChecked = false; foreach (var element in new FrameworkElement[] { _confirmed, _apply }) if (element.Parent is System.Windows.Controls.Panel parent) parent.Children.Remove(element); _review.Children.Clear(); _reviewPanel.Visibility = Visibility.Collapsed; _apply.IsEnabled = false; }
 private void FillSuggestions()
 {
  if (_busy || _context == null) return; int count = 0;
  foreach (var row in _context.Rows.Where(row => row.Editable && !string.IsNullOrWhiteSpace(row.SuggestedName))) { row.KoreanName = row.SuggestedName!; row.Selected = true; count++; }
  _status.Text = count == 0 ? "확인된 한글 이름이 없습니다. 표시명을 직접 입력하세요." : $"확인된 추천 이름 {count}개를 채웠습니다. 철자를 확인한 뒤 미리보기 하세요.";
 }
 private void SelectRows(bool selected) { if (_busy || _context == null) return; foreach (var row in _context.Rows.Where(row => row.Editable)) row.Selected = selected; }
 private object[] Entries()
 {
  if (_context == null) throw new InvalidOperationException("선수 이름을 먼저 불러오세요.");
  var changed = _context.Rows.Where(row => row.Selected && row.Editable && row.KoreanName.Trim() != row.BaselineName).ToArray();
  if (changed.Length == 0) throw new InvalidOperationException("바꿀 선수의 한글 이름을 입력하고 적용 칸을 선택하세요.");
  foreach (var row in changed) { string value = row.KoreanName.Trim(); if (value.EnumerateRunes().Count() is < 1 or > 40 || Encoding.UTF8.GetByteCount(value) > 120 || !ContainsHangul(value) || value.Any(char.IsControl) || value.Contains('"') || value.Contains('\\')) throw new InvalidOperationException($"{row.Name}: 한글을 포함한 1–40자, 120바이트 이내의 이름을 입력하세요. 줄바꿈, 따옴표, 역슬래시는 사용할 수 없습니다."); }
  return changed.Select(row => (object)new { id = row.Id, korean_name = row.KoreanName.Trim() }).ToArray();
 }
 private async Task PreviewAsync()
 {
  ClearPreview(); var entries = Entries(); int version = _version;
  var preview = Read<NameLocalizationPreview>(await _bridge.CallAsync("name_localization_preview", new { entries }, _lifetime.Token));
  if (_closed || version != _version) return;
  if (preview.Token.Length == 0 || preview.Entries.Count != entries.Length) throw new InvalidDataException("한글화 미리보기의 검토 토큰 또는 선수 수가 올바르지 않습니다.");
  _preview = preview; _review.Children.Add(Text($"적용 전 검토 · {preview.Entries.Count}명", 16, true));
  var changes = Table(); changes.Height = Math.Min(180, 36 + 39 * preview.Entries.Count); changes.RowHeight = 35; changes.ItemsSource = preview.Entries; Column(changes, "현재", nameof(NameLocalizationChange.Before), 1, true); Column(changes, "변경 후", nameof(NameLocalizationChange.After), 1, true); _review.Children.Add(changes);
  _reviewHint.Text = "이름·성 등 원본 데이터는 보존됩니다. 설치된 FM 데이터베이스에서 같은 UID를 사용하는 다른 세이브에도 표시 이름이 적용될 수 있습니다. 적용 후 FM 재시작 → 세이브 다시 불러오기가 필요합니다."; _reviewHint.Margin = new Thickness(0, 9, 0, 6); _review.Children.Add(_reviewHint);
  var files = new TextBox { Text = string.Join("\n\n", preview.Files.Select(file => file.Description).Concat(preview.Warnings)), IsReadOnly = true, TextWrapping = TextWrapping.Wrap, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, MaxHeight = 120, MinHeight = 35 };
  _review.Children.Add(Details($"파일 {preview.Files.Count}개 · 충돌 {preview.Files.Sum(file => file.Conflicts.Count)}건 · 경로와 변경 내용", files, preview.Warnings.Count > 0 || preview.Files.Any(file => file.Conflicts.Count > 0)));
  var footer = new WrapPanel(); footer.Children.Add(_confirmed); footer.Children.Add(_apply); _review.Children.Add(footer); _reviewPanel.Visibility = Visibility.Visible;
  _ = Dispatcher.BeginInvoke(() => _reviewPanel.BringIntoView(), System.Windows.Threading.DispatcherPriority.Loaded);
  _status.Text = preview.CanApply && _context?.CanInstall == true ? "이름과 설치 경로를 확인한 다음 설치하세요." : "미리보기 전용 · 현재 상태에서는 설치할 수 없습니다."; UpdateControls();
 }
 private async Task ApplyAsync()
 {
  if (_preview is not { CanApply: true } reviewed || _context?.CanInstall != true || _confirmed.IsChecked != true || _busy) return;
  if (AppDialog.Show(this, $"검토한 {reviewed.Entries.Count}명의 한글화 파일을 설치할까요?\n\nFM은 자동으로 종료하지 않습니다. 재시작 후 세이브를 다시 불러오세요.", "한글화 파일 설치", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
  await ApplyReviewedAsync(reviewed);
 }
 private async Task ApplyReviewedAsync(NameLocalizationPreview reviewed)
 {
  if (_preview != reviewed || !reviewed.CanApply || _context?.CanInstall != true || _confirmed.IsChecked != true || _busy || _closed || _invalidated) return;
  await RunAsync(async () => {
   string token = reviewed.Token; ClearPreview(); var receipt = Read<NameLocalizationReceipt>(await _bridge.CallAsync("name_localization_apply", new { token, confirmed = true }));
   FilesChanged = true; foreach (var entry in reviewed.Entries) { var row = _context.Rows.Single(item => item.Id == entry.Id); row.InstalledName = entry.After; } _baseline = Fingerprint();
   string message = "파일 설치 완료 · FM 재시작 후 세이브 다시 불러오기";
   try { await LoadCoreAsync(); _status.Text = message + (receipt.Message.Length == 0 ? "" : "\n" + receipt.Message); }
   catch (Exception error) { ClearPreview(); _status.Text = message + "\n다시 읽기 실패 · " + error.Message; App.Log(error); }
  });
 }
 private async Task LoadHistoryAsync()
 {
  _history.ItemsSource = null; _historyDetail.Text = "";
  var data = await _bridge.CallAsync("name_localization_history", cancellationToken: _lifetime.Token);
  if (_closed) return;
  if (data.ValueKind == JsonValueKind.Object) data = data.TryGetProperty("items", out var items) ? items : data.GetProperty("receipts");
  _history.ItemsSource = Read<List<NameLocalizationReceipt>>(data); UpdateControls();
 }
 private async Task UndoAsync()
 {
  if (_busy || _history.SelectedItem is not NameLocalizationReceipt { CanUndo: true } receipt || !ConfirmDiscard()) return;
  if (AppDialog.Show(this, $"이 설치의 한글화 파일을 변경 전으로 되돌릴까요?\n{receipt.Id}\n\nFM 재시작 후 세이브를 다시 불러와야 합니다.", "한글화 설치 되돌리기", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
  await UndoReviewedAsync(receipt);
 }
 private async Task UndoReviewedAsync(NameLocalizationReceipt receipt)
 {
  if (_busy || _closed || _invalidated || !receipt.CanUndo) return;
  await RunAsync(async () => { ClearPreview(); await _bridge.CallAsync("name_localization_undo", new { id = receipt.Id, confirmed = true }); FilesChanged = true; try { await LoadCoreAsync(); _status.Text = "파일 되돌리기 완료 · FM 재시작 후 세이브 다시 불러오기"; } catch (Exception error) { ClearPreview(); _status.Text = "파일은 되돌렸습니다. 다시 읽기 실패 · " + error.Message; App.Log(error); } });
 }
 private void UpdateControls()
 {
  bool enabled = !_busy && !_closed && !_invalidated; _grid.IsEnabled = enabled && _context != null;
  foreach (var button in new[] { _fill, _selectAll, _clear, _previewButton }) button.IsEnabled = enabled && _loaded;
  _refresh.IsEnabled = _historyRefresh.IsEnabled = enabled; _history.IsEnabled = enabled; _confirmed.IsEnabled = enabled && _context?.CanInstall == true;
  _apply.IsEnabled = enabled && _context?.CanInstall == true && _preview is { CanApply: true } && _confirmed.IsChecked == true;
  _undo.IsEnabled = enabled && _history.SelectedItem is NameLocalizationReceipt { CanUndo: true };
 }
 private async Task RunAsync(Func<Task> operation)
 {
  if (_busy || _closed || _invalidated) return; _busy = true; var completion = _inflight = new(TaskCreationOptions.RunContinuationsAsynchronously); UpdateControls();
  try { await operation(); } catch (OperationCanceledException) { if (!_closed) _status.Text = "작업을 취소했습니다."; ClearPreview(); } catch (Exception error) { App.Log(error); ClearPreview(); _status.Text = error.Message; }
  finally { _busy = false; completion.TrySetResult(); UpdateControls(); if (_invalidated && !_closed) Close(); }
 }
 private bool ConfirmDiscard() => !HasPendingChanges || AppDialog.Show(this, "아직 설치하지 않은 이름 변경이 있습니다. 작성 내용을 버릴까요?", "이름 변경 확인", MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes;
 private bool CanClose(Func<bool> confirmDiscard)
 {
  if (_busy) { _status.Text = "진행 중인 작업이 끝난 뒤 닫아 주세요."; return false; }
  return _invalidated || !HasPendingChanges || confirmDiscard();
 }
 private void OnClosing(object? sender, CancelEventArgs args) => args.Cancel = !CanClose(ConfirmDiscard);
}
