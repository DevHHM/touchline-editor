using System.Collections;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Globalization;
using System.IO;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Microsoft.Win32;

namespace Touchline.Wpf;

/// <summary>Address-free scouting, local libraries and explicitly reviewed batch operations.</summary>
public sealed class WorkbenchWindow : Window
{
 private readonly BridgeClient _bridge;
 private bool _live;
 private EntityRecord? _current;
 private readonly EntityRecord? _ownClub;
 private readonly Action<EntityRecord> _open;
 private readonly ObservableCollection<EntityRecord> _targets = [], _results = [];
 private readonly ObservableCollection<WorkbenchDraft> _ranges = [], _rules = [];
 private readonly List<EditableField> _catalog = [];
 private List<DetailRecord> _comparisonDetails = [];
 private readonly List<(TextBlock Label, EntityRecord Entity)> _comparisonHeadings = [];
 private readonly Dictionary<string, string[]> _historyFields = [];
 private readonly TabControl _tabs = new();
 private TabItem? _freezeTab;
 private readonly DataGrid _targetGrid = EntityTable(), _resultGrid = EntityTable(), _rangeGrid = EditTable(), _ruleGrid = EditTable(), _listGrid = EntityTable(), _batchChanges = ChangeTable(), _freezeChanges = ChangeTable(), _historyGrid = HistoryTable();
 private readonly ComboBox _kind = new() { Width = 110 }, _lists = new() { MinWidth = 200, DisplayMemberPath = "Name" }, _presets = new() { MinWidth = 200, DisplayMemberPath = "Name" };
 private readonly ComboBox _footballCategory = new() { Width = 110, DisplayMemberPath = "Label", SelectedValuePath = "Key", ToolTip = FootballSearchCategories.Hint };
 private FrameworkElement _footballCategoryControl = null!;
 private readonly ComboBox _nationality = new() { Width = 185, DisplayMemberPath = nameof(SearchNationChoice.Label), IsEditable = true, IsTextSearchEnabled = true, IsTextSearchCaseSensitive = false, MaxDropDownHeight = 320, ToolTip = SearchNationalityChoices.Hint };
 private FrameworkElement _nationalityControl = null!;
 private List<SearchNationChoice> _nations = SearchNationalityChoices.Build([]);
 private string? _nationalityKind;
 private readonly TextBox _query = new() { MinWidth = 160 }, _role = new() { Width = 100 }, _listName = new() { MinWidth = 180 }, _presetName = new() { MinWidth = 180 };
 private readonly CheckBox _ownOnly = Check("우리 구단"), _unemployed = Check("무소속"), _batchConfirm = Check("대상별 이전 값과 변경 값을 확인했습니다"), _freezeConfirm = Check("표시한 값을 3초마다 확인하고 필요하면 게임에 다시 적용합니다");
 private readonly Dictionary<string, TextBox> _bounds = [];
 private readonly TextBlock _status = Text("준비 중…"), _targetCount = Text("작업 대상 0 / 200", 16, true), _searchProgress = Text("검색 조건을 선택하세요"), _libraryStatus = Text("앱 관심명단입니다. FM 내부 영입명단과 별도로 저장됩니다."), _ruleSummary = Text("변경 규칙이 없습니다"), _freezeState = Text("수치 고정 중지됨"), _batchSummary = Text("대상과 규칙을 선택한 뒤 미리 보세요."), _freezeSummary = Text("고정할 대상은 최대 50명입니다.");
 private readonly StackPanel _comparison = new(), _searchFilters = new();
 private readonly DispatcherTimer _freezeTimer = new() { Interval = TimeSpan.FromSeconds(3) };
 private readonly TaskCompletionSource _initialized = new(TaskCreationOptions.RunContinuationsAsynchronously);
 private readonly List<Button> _buttons = [];
 private readonly HashSet<Button> _offlineButtons = [];
 private readonly TextBlock _connectionLabel = Text("");
 private CancellationTokenSource _resultLifetime = new();
 private bool _connectionInvalidated;
 private int _resultVersion;
 private Button _searchButton = null!, _cancelButton = null!, _batchApply = null!, _freezeEnable = null!, _freezeStop = null!, _undo = null!;
 private WorkbenchLibrary _library = new();
 private JsonElement _libraryBaseline;
 private CancellationTokenSource? _queryCancellation;
 private ulong? _activeQueryId;
 private TaskCompletionSource? _actionFinished, _queryFinished;
 private string? _batchToken, _freezeToken;
 private bool _busy, _queryRunning, _freezeActive, _closing, _closeRequested, _libraryDirty, _stopping;
 public bool IsBusy => _busy;
 public bool IsConnectionInvalidated => _connectionInvalidated;
 private int _catalogVersion, _draftVersion;

 public WorkbenchWindow(BridgeClient bridge, bool live, EntityRecord? current, EntityRecord? ownClub, Action<EntityRecord> open)
 {
  _bridge = bridge; _live = live; _current = current; _ownClub = ownClub; _open = open;
  Title = "스카우팅 · 대량 편집 — Touchline"; Width = 1350; Height = 900; MinWidth = 1120; MinHeight = 740; WindowStartupLocation = WindowStartupLocation.CenterOwner;
  Background = Resource("BaseBrush"); Foreground = Resource("InkBrush"); Typography.Initialize(this);
  var root = new Grid { Margin = new Thickness(20) };
  root.RowDefinitions.Add(new() { Height = GridLength.Auto }); root.RowDefinitions.Add(new()); root.RowDefinitions.Add(new() { Height = GridLength.Auto });
  var heading = new StackPanel { Margin = new Thickness(0, 0, 0, 14) };
  heading.Children.Add(Text("스카우팅 · 대량 편집", 25, true)); _connectionLabel.Text = live ? "현재 게임 · 모든 변경은 개별 검토와 확인 후 적용됩니다" : "스냅샷 · 검색, 비교, 미리보기와 로컬 저장 가능 · 게임 적용 불가"; heading.Children.Add(_connectionLabel); root.Children.Add(heading);
  var body = new Grid(); body.ColumnDefinitions.Add(new()); body.ColumnDefinitions.Add(new() { Width = new GridLength(280) }); Grid.SetRow(body, 1); root.Children.Add(body);
  _tabs.Margin = new Thickness(0, 0, 14, 0); body.Children.Add(_tabs); BuildSearch(); BuildComparison(); BuildLibrary(); BuildPresets(); BuildBatch(); BuildFreeze();
  var basket = new DockPanel { Margin = new Thickness(12) }; basket.Children.Add(Docked(_targetCount, Dock.Top));
  var basketHelp = Text("탭을 바꿔도 작업 대상을 유지합니다. 검색 결과에서 선택한 대상을 추가하세요."); basketHelp.Margin = new Thickness(0, 4, 0, 12); basket.Children.Add(Docked(basketHelp, Dock.Top));
  var basketActions = Bar(Button("현재 대상 추가", () => { if (_current != null) AddTargets([_current]); return Task.CompletedTask; }), Button("선택 제거", () => { foreach (var item in _targetGrid.SelectedItems.Cast<EntityRecord>().ToArray()) _targets.Remove(item); return Task.CompletedTask; }), Button("비우기", () => { _targets.Clear(); return Task.CompletedTask; })); basket.Children.Add(Docked(basketActions, Dock.Bottom));
  _targetGrid.ItemsSource = _targets; _targetGrid.Columns.Clear(); Column(_targetGrid, "대상 / 이름", "DisplayName", 1); Column(_targetGrid, "종류", "KindLabel", 60, false); _targetGrid.MouseDoubleClick += (_, _) => OpenFrom(_targetGrid); basket.Children.Add(_targetGrid);
  foreach (var entityGrid in new[] { _targetGrid, _resultGrid, _listGrid })
  {
   AbilitySafeGridSort.Attach(entityGrid);
   NameCopy.AttachGrid(entityGrid, item => item as EntityRecord, message => _status.Text = message);
   entityGrid.ToolTip = "우클릭: 이름 복사 · Ctrl+C / Ctrl+Shift+C";
  }
  var basketCard = Card(basket); Grid.SetColumn(basketCard, 1); body.Children.Add(basketCard);
  _status.Margin = new Thickness(0, 12, 0, 0); _status.MaxHeight = 56; Grid.SetRow(_status, 2); root.Children.Add(_status); Content = root;
  _targets.CollectionChanged += (_, _) => { _targetCount.Text = $"작업 대상 {_targets.Count} / 200"; InvalidateReview(); };
  _rules.CollectionChanged += (_, _) => { foreach (var draft in _rules) { draft.PropertyChanged -= RuleChanged; draft.PropertyChanged += RuleChanged; } InvalidateReview(); };
  _batchConfirm.Checked += (_, _) => UpdateActions(); _batchConfirm.Unchecked += (_, _) => UpdateActions(); _freezeConfirm.Checked += (_, _) => UpdateActions(); _freezeConfirm.Unchecked += (_, _) => UpdateActions();
  _freezeTimer.Tick += async (_, _) => { if (!_busy && _freezeActive) await RunAsync(TickFreezeAsync); };
  if (_current != null) AddTargets([_current]);
  Loaded += async (_, _) => { try { await RunAsync(InitializeAsync); } finally { _initialized.TrySetResult(); } };
  _rangeGrid.Items.Filter = item => item is WorkbenchDraft draft && !AbilityVisibility.Current.IsHidden(draft.Key);
  _ruleGrid.Items.Filter = item => item is WorkbenchDraft draft && !AbilityVisibility.Current.IsHidden(draft.Key);
  AbilityVisibility.Current.Changed += AbilityVisibilityChanged;
  Closed += (_, _) => AbilityVisibility.Current.Changed -= AbilityVisibilityChanged;
  Closing += OnClosing; AbilityVisibilityChanged(null, EventArgs.Empty); UpdateActions();
 }

 private static Brush Resource(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Text(string text, double size = 12, bool bold = false) => new() { Text = text, FontSize = size, FontWeight = bold ? FontWeights.Bold : FontWeights.Medium, Foreground = Resource(bold ? "InkBrush" : "MutedBrush"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 0, 7) };
 private static CheckBox Check(string text) => new() { Content = text, Foreground = Resource("InkBrush"), VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(4, 5, 12, 5) };
 private static Border Card(UIElement child) => new() { Child = child, Background = Resource("PanelBrush"), BorderBrush = Resource("BorderBrush"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(8) };
 private static UIElement Docked(UIElement child, Dock dock) { DockPanel.SetDock(child, dock); return child; }
 private static WrapPanel Bar(params UIElement[] items) { var panel = new WrapPanel { Margin = new Thickness(0, 4, 0, 8) }; foreach (var item in items) panel.Children.Add(item); return panel; }
 private Button Button(string title, Func<Task> callback, bool allowDisconnected = false) { var button = new Button { Content = title, Padding = new Thickness(10, 7, 10, 7), Margin = new Thickness(0, 2, 7, 2) }; button.Click += async (_, _) => await RunAsync(callback, allowDisconnected); _buttons.Add(button); if (allowDisconnected) _offlineButtons.Add(button); return button; }
 private static StackPanel Labeled(string name, UIElement input) { var panel = new StackPanel { Margin = new Thickness(0, 0, 12, 8) }; panel.Children.Add(Text(name, 11)); panel.Children.Add(input); return panel; }
 private TabItem Tab(string title, UIElement content) { var tab = new TabItem { Header = title, Padding = new Thickness(12, 9, 12, 9), Content = new Border { Padding = new Thickness(12), Child = content } }; _tabs.Items.Add(tab); return tab; }
 private static DataGrid EditTable() => new() { AutoGenerateColumns = false, CanUserAddRows = false, CanUserDeleteRows = false, IsReadOnly = false, SelectionMode = DataGridSelectionMode.Single, RowHeight = 36, MinHeight = 70, HeadersVisibility = DataGridHeadersVisibility.Column };
 private static DataGrid EntityTable() { var grid = new DataGrid { IsReadOnly = true, SelectionMode = DataGridSelectionMode.Extended, SelectionUnit = DataGridSelectionUnit.FullRow, AutoGenerateColumns = false, CanUserAddRows = false, CanUserDeleteRows = false, EnableRowVirtualization = true, RowHeight = 36 }; Column(grid, "이름", "DisplayName", 1.6); Column(grid, "국적", nameof(EntityRecord.NationalityLabel), 98, false); Column(grid, "구단", "Club", 1.5); Column(grid, "축구 구분", nameof(EntityRecord.FootballCategoryLabel), 84, false); Column(grid, "나이", "Age", 58, false); Column(grid, "CA", "Ca", 52, false); Column(grid, "PA", "Pa", 52, false); Column(grid, "역할", "Role", 1); EntityRoleSorting.Attach(grid); return grid; }
 private static void Column(DataGrid grid, string name, string binding, double width, bool star = true) { var style = new Style(typeof(TextBlock)); Binding Display() => new(binding) { Converter = binding is "Ca" or "Pa" ? new WorkbenchAbilityConverter(binding.ToLowerInvariant()) : null }; style.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis)); style.Setters.Add(new Setter(ToolTipService.ToolTipProperty, Display())); grid.Columns.Add(new DataGridTextColumn { Header = name, Binding = Display(), SortMemberPath = binding, CanUserSort = !AbilityVisibility.Current.IsHidden(binding.ToLowerInvariant()), ElementStyle = style, MinWidth = star ? 72 : width, Width = new DataGridLength(width, star ? DataGridLengthUnitType.Star : DataGridLengthUnitType.Pixel) }); }
 private sealed class WorkbenchAbilityConverter(string key) : IValueConverter
 {
  public object Convert(object value, Type targetType, object parameter, CultureInfo culture) => AbilityVisibility.Current.Format(key, value);
  public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => Binding.DoNothing;
 }
 private void AbilityVisibilityChanged(object? sender, EventArgs args)
 {
  if (!Dispatcher.CheckAccess()) { Dispatcher.Invoke(() => AbilityVisibilityChanged(sender, args)); return; }
  object? selectedRange = _rangeGrid.SelectedItem, selectedRule = _ruleGrid.SelectedItem;
  foreach (var grid in new[] { _rangeGrid, _ruleGrid }) { grid.CommitEdit(DataGridEditingUnit.Cell, true); grid.CommitEdit(DataGridEditingUnit.Row, true); grid.ItemsSource = null; }
  RefreshFieldChoices();
  _rangeGrid.ItemsSource = _ranges; _ruleGrid.ItemsSource = _rules;
  foreach (var grid in new[] { _rangeGrid, _ruleGrid }) grid.Items.Filter = item => item is WorkbenchDraft draft && !AbilityVisibility.Current.IsHidden(draft.Key);
  if (selectedRange is WorkbenchDraft range && !AbilityVisibility.Current.IsHidden(range.Key)) _rangeGrid.SelectedItem = range;
  if (selectedRule is WorkbenchDraft rule && !AbilityVisibility.Current.IsHidden(rule.Key)) _ruleGrid.SelectedItem = rule;
  foreach (var grid in new[] { _targetGrid, _resultGrid, _listGrid, _batchChanges, _freezeChanges })
  {
   grid.Items.Refresh();
   foreach (var column in grid.Columns.Where(column => column.SortMemberPath is "Ca" or "Pa")) column.CanUserSort = !AbilityVisibility.Current.IsHidden(column.SortMemberPath.ToLowerInvariant());
   if (grid == _resultGrid || grid == _listGrid) AbilitySafeGridSort.Refresh(grid, nameof(EntityRecord.DisplayName), path => AbilityVisibility.Current.IsHidden(path), AbilityVisibility.Current.HidePa);
  }
  foreach (var (key, box) in _bounds)
   if (box.Parent is FrameworkElement control) control.Visibility = AbilityVisibility.Current.IsHidden(key.StartsWith("ca_", StringComparison.Ordinal) ? "ca" : key.StartsWith("pa_", StringComparison.Ordinal) ? "pa" : "") ? Visibility.Collapsed : Visibility.Visible;
  RefreshComparison(); UpdateActions();
 }
 private static DataGrid ChangeTable() { var grid = EntityTable(); grid.Columns.Clear(); Column(grid, "대상", "Name", 1.4); Column(grid, "항목", "Field", 1.2); Column(grid, "이전", "Before", 1); Column(grid, "변경 후", "After", 1); return grid; }
 private static DataGrid HistoryTable() { var grid = EntityTable(); grid.SelectionMode = DataGridSelectionMode.Single; grid.Columns.Clear(); Column(grid, "시각", "Time", 1.2); Column(grid, "상태", "Status", 1.5); Column(grid, "대상", "Targets", 60, false); Column(grid, "변경", "Changes", 60, false); return grid; }
 private static T Read<T>(JsonElement value) => value.Deserialize<T>(JsonDefaults.Options) ?? throw new InvalidOperationException("응답 자료가 비어 있습니다");
 private static string Str(JsonElement value, string key) => value.TryGetProperty(key, out var field) ? field.ToString() : "";
 private static double? Number(string text, string label) { if (string.IsNullOrWhiteSpace(text)) return null; if (!double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var value) || !double.IsFinite(value)) throw new InvalidOperationException($"{label}: 숫자를 입력하세요"); return value; }
 private static uint? Whole(string text, string label, uint max) { double? value = Number(text, label); if (value == null) return null; if (value < 0 || value > max || value != Math.Truncate(value.Value)) throw new InvalidOperationException($"{label}: 0–{max} 정수를 입력하세요"); return (uint)value; }
 private string Kind => (_kind.SelectedItem as ComboBoxItem)?.Tag?.ToString() ?? "Player";
 private WorkbenchRef[] Targets(int limit = 200) { if (_targets.Count == 0 || _targets.Count > limit) throw new InvalidOperationException($"작업 대상은 1–{limit}명을 선택하세요"); return _targets.Select(entity => new WorkbenchRef(entity.Kind, entity.Id)).ToArray(); }
 private void AddTargets(IEnumerable<EntityRecord> entities) { var additions = entities.Where(entity => !_targets.Any(existing => existing.Identity == entity.Identity)).DistinctBy(entity => entity.Identity).ToArray(); if (_targets.Count + additions.Length > 200) throw new InvalidOperationException("작업 대상은 최대 200명입니다"); foreach (var entity in additions) _targets.Add(entity); }
 private void OpenFrom(DataGrid grid) { if (!_connectionInvalidated && grid.SelectedItem is EntityRecord entity) _open(entity); }
 public void InvalidateResults()
 {
  ++_resultVersion; ++_catalogVersion; _resultLifetime.Cancel(); _resultLifetime.Dispose(); _resultLifetime = new();
  _queryCancellation?.Cancel(); _activeQueryId = null; _freezeTimer.Stop(); _freezeActive = false;
  _current = null; _results.Clear(); _targets.Clear(); _listGrid.ItemsSource = null; _comparisonDetails.Clear(); _comparisonHeadings.Clear(); _comparison.Children.Clear(); _historyGrid.ItemsSource = null; _historyFields.Clear();
  InvalidateReview(); _freezeChanges.ItemsSource = null; _freezeSummary.Text = "자료가 변경되었습니다. 대상을 다시 확인하고 미리 보세요.";
  _freezeState.Text = "수치 고정 중지됨"; _searchProgress.Text = "이전 결과를 비웠습니다. 검색 조건을 유지한 채 다시 검색하세요."; _searchProgress.ToolTip = null;
  _status.Text = "자료가 변경되어 결과와 미리보기를 비웠습니다. 검색 조건과 라이브러리 초안은 유지됩니다."; UpdateActions();
 }
 public void InvalidateConnection()
 {
  _connectionInvalidated = true; _live = false; InvalidateResults();
  _kind.IsEnabled = _lists.IsEnabled = _presets.IsEnabled = _searchFilters.IsEnabled = false;
  _rangeGrid.IsReadOnly = _ruleGrid.IsReadOnly = true; _batchConfirm.IsEnabled = _freezeConfirm.IsEnabled = false;
  _connectionLabel.Text = "연결 해제 · 이 창에서는 작업할 수 없습니다";
  _status.Text = "연결이 해제되었습니다. ‘라이브러리 저장’으로 보존한 초안을 저장할 수 있습니다. 다시 연결한 뒤 작업 창을 새로 여세요."; UpdateActions();
 }
 private async Task<JsonElement> CallAsync(string command, object? args = null, CancellationToken cancellationToken = default)
 {
  bool localSave = command == "library_put";
  if (_connectionInvalidated && !localSave) throw new OperationCanceledException("연결이 해제되었습니다.");
  int version = _resultVersion;
  using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _resultLifetime.Token);
  var result = await _bridge.CallAsync(command, args, linked.Token);
  if ((_connectionInvalidated && !localSave) || version != _resultVersion)
  {
   if (command == "query_start" && result.TryGetProperty("query_id", out var cursor))
    try { await _bridge.CallAsync("query_cancel", new { query_id = cursor.GetUInt64() }); } catch (Exception error) { App.Log(error); }
   throw new OperationCanceledException("자료가 변경되어 이전 작업 결과를 폐기했습니다.");
  }
  return result;
 }
 private async Task RunAsync(Func<Task> action, bool allowDisconnected = false) { if (_busy || _closing || _stopping || (_connectionInvalidated && !allowDisconnected)) return; int version = _resultVersion; _busy = true; var completed = _actionFinished = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously); _kind.IsEnabled = _lists.IsEnabled = _presets.IsEnabled = false; UpdateActions(); try { await action(); } catch (OperationCanceledException) { if (!_connectionInvalidated && version == _resultVersion) _status.Text = "검색 중지됨 · 완료된 결과는 유지됩니다"; } catch (Exception error) { App.Log(error); _status.Text = error.Message; } finally { _busy = false; _kind.IsEnabled = _lists.IsEnabled = _presets.IsEnabled = !_stopping && !_connectionInvalidated; UpdateActions(); completed.TrySetResult(); } }
 private async Task InitializeAsync() { _libraryBaseline = await CallAsync("library_get"); _library = Read<WorkbenchLibrary>(_libraryBaseline); RefreshLibrary(); await ResolveListAsync(); await LoadCatalogAsync(); await LoadHistoryAsync(); var freeze = await CallAsync("freeze_status"); ShowFreezeStatus(freeze); _status.Text = _live ? "준비됨 · 게임 적용은 변경 검토 후 가능합니다" : "준비됨 · 스냅샷은 게임에 적용할 수 없습니다"; }

 private void BuildSearch()
 {
  foreach (var (label, kind) in new[] { ("선수", "Player"), ("스태프", "Staff"), ("구단", "Club"), ("기타 인물", "Person"), ("도시", "City") }) _kind.Items.Add(new ComboBoxItem { Content = label, Tag = kind }); _kind.SelectedIndex = Math.Max(0, _kind.Items.Cast<ComboBoxItem>().ToList().FindIndex(item => item.Tag?.ToString() == _current?.Kind));
  _kind.SelectionChanged += async (_, _) => { if (IsLoaded && !_busy) await RunAsync(async () => { _ranges.Clear(); _rules.Clear(); await LoadCatalogAsync(); }); };
  _ownOnly.IsEnabled = _ownClub != null;
  _footballCategory.ItemsSource = FootballSearchCategories.Choices; _footballCategory.SelectedValue = "";
  _footballCategoryControl = Labeled("축구 구분", _footballCategory); _footballCategoryControl.Visibility = FootballSearchCategories.Supports(Kind) ? Visibility.Visible : Visibility.Collapsed;
  TextSearch.SetTextPath(_nationality, nameof(SearchNationChoice.Name)); System.Windows.Automation.AutomationProperties.SetName(_nationality, "주 국적");
  _nationality.ItemsSource = _nations; _nationality.SelectedIndex = 0; _nationality.Text = SearchNationalityChoices.AllLabel;
  _nationalityControl = Labeled("주 국적 · 입력 / 선택", _nationality); _nationalityControl.Visibility = SearchNationalityChoices.Supports(Kind) ? Visibility.Visible : Visibility.Collapsed;
  var identity = Bar(Labeled("종류", _kind), _footballCategoryControl, Labeled("이름 · 영문 이름 · UID", _query), Labeled("역할 / 포지션", _role), _nationalityControl, _ownOnly, _unemployed); _searchFilters.Children.Add(identity);
  var numeric = new WrapPanel(); foreach (string key in new[] { "age_min", "age_max", "ca_min", "ca_max", "pa_min", "pa_max" }) { var box = new TextBox { Width = 76 }; _bounds[key] = box; numeric.Children.Add(Labeled(key switch { "age_min" => "최소 나이", "age_max" => "최대 나이", "ca_min" => "최소 CA", "ca_max" => "최대 CA", "pa_min" => "최소 PA", _ => "최대 PA" }, box)); } _searchFilters.Children.Add(numeric);
  _rangeGrid.ItemsSource = _ranges; _rangeGrid.MaxHeight = 170; _searchFilters.Children.Add(_rangeGrid);
  _searchFilters.Children.Add(Bar(Button("능력치 조건 추가", () => { if (_ranges.Count >= 16) throw new InvalidOperationException("능력치 조건은 최대 16개입니다"); _ranges.Add(new WorkbenchDraft { Key = _catalog.FirstOrDefault(field => !AbilityVisibility.Current.IsHidden(field.Key))?.Key ?? "" }); return Task.CompletedTask; }), Button("조건 제거", () => { if (_rangeGrid.SelectedItem is WorkbenchDraft draft) _ranges.Remove(draft); return Task.CompletedTask; })));
  var dock = new DockPanel(); dock.Children.Add(Docked(_searchFilters, Dock.Top));
  _searchButton = Button("검색 시작", SearchAsync); _cancelButton = new Button { Content = "검색 중지", IsEnabled = false, Margin = new Thickness(0, 2, 7, 2) }; _cancelButton.Click += (_, _) => _queryCancellation?.Cancel();
  dock.Children.Add(Docked(Bar(_searchButton, _cancelButton, Button("선택을 작업 대상에 추가", () => { AddTargets(_resultGrid.SelectedItems.Cast<EntityRecord>()); return Task.CompletedTask; }), Button("선택 CSV 내보내기", ExportRowsAsync)), Dock.Top)); dock.Children.Add(Docked(_searchProgress, Dock.Top));
  _resultGrid.ItemsSource = _results; _resultGrid.MouseDoubleClick += (_, _) => OpenFrom(_resultGrid); dock.Children.Add(_resultGrid); Tab("고급 검색", dock);
 }
 private async Task LoadCatalogAsync()
 {
  string kind = Kind; bool categorySupported = FootballSearchCategories.Supports(kind); _footballCategoryControl.Visibility = categorySupported ? Visibility.Visible : Visibility.Collapsed; if (!categorySupported) _footballCategory.SelectedValue = "";
  bool nationalitySupported = SearchNationalityChoices.Supports(kind); _nationalityControl.Visibility = nationalitySupported ? Visibility.Visible : Visibility.Collapsed;
  var selectedNationality = _nationality.SelectedItem as SearchNationChoice; string nationalityText = _nationality.Text;
  try { selectedNationality = SearchNationalityChoices.Resolve(_nations, selectedNationality, nationalityText); nationalityText = selectedNationality.Name; } catch (InvalidOperationException) { /* Preserve invalid input for an explicit search error instead of broadening it. */ }
  _nationalityKind = null; _nationality.IsEnabled = false;
  int version = ++_catalogVersion; var args = new Dictionary<string, object?> { ["kind"] = kind }; if (_current?.Kind == kind) args["id"] = _current.Id;
  var response = await CallAsync("field_catalog", args); if (version != _catalogVersion || kind != Kind) return;
  var facets = await CallAsync("search_facets", new { kind }); if (version != _catalogVersion || kind != Kind) return;
  _nations = SearchNationalityChoices.Build(SearchNationalityChoices.FromFacets(facets), nationalitySupported ? selectedNationality?.Id : null, selectedNationality?.Name);
  _nationality.ItemsSource = _nations; _nationality.SelectedItem = nationalitySupported ? _nations.FirstOrDefault(choice => choice.Id == selectedNationality?.Id) : _nations[0];
  _nationality.Text = nationalitySupported && !string.IsNullOrWhiteSpace(nationalityText) ? nationalityText : SearchNationalityChoices.AllLabel;
  _nationalityKind = kind; _nationality.IsEnabled = nationalitySupported;
  var nationalityColumn = _resultGrid.Columns.OfType<DataGridTextColumn>().Single(column => (column.Binding as Binding)?.Path.Path == nameof(EntityRecord.NationalityLabel)); nationalityColumn.Visibility = nationalitySupported ? Visibility.Visible : Visibility.Collapsed;
  _catalog.Clear(); _catalog.AddRange(Read<List<EditableField>>(response.GetProperty("fields")));
  _rangeGrid.Columns.Clear(); _ruleGrid.Columns.Clear(); AddFieldColumn(_rangeGrid, _catalog); AddFieldColumn(_ruleGrid, _catalog.Where(field => field.Editable).ToArray());
  foreach (var (header, property) in new[] { ("최소", "Minimum"), ("최대", "Maximum") }) _rangeGrid.Columns.Add(new DataGridTextColumn { Header = header, Binding = DraftBinding(property), Width = 100 });
  _ruleGrid.Columns.Add(new DataGridComboBoxColumn { Header = "연산", ItemsSource = new[] { new Option("set", "지정 값"), new Option("add", "더하기 / 빼기"), new Option("percent", "비율 % 증감"), new Option("at_least", "최소 보장"), new Option("at_most", "최대 제한") }, DisplayMemberPath = "Label", SelectedValuePath = "Key", SelectedValueBinding = DraftBinding("Operation"), Width = 150 }); _ruleGrid.Columns.Add(new DataGridTextColumn { Header = "값", Binding = DraftBinding("Value"), Width = 110 });
 }
 private sealed record Option(string Key, string Label);
 private sealed class DraftKeyConverter : IValueConverter
 {
  public object Convert(object value, Type targetType, object parameter, CultureInfo culture) => value;
  public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => value is string { Length: > 0 } ? value : Binding.DoNothing;
 }
 private static Binding DraftBinding(string property) => new(property) { Mode = BindingMode.TwoWay, UpdateSourceTrigger = UpdateSourceTrigger.PropertyChanged, Converter = property == "Key" ? new DraftKeyConverter() : null };
 private static Option[] FieldChoices(IEnumerable<EditableField> fields) => fields.Where(field => !AbilityVisibility.Current.IsHidden(field.Key)).Select(field => new Option(field.Key, $"{field.Section} · {field.Label} ({field.Min:G}–{field.Max:G})")).ToArray();
 private static void AddFieldColumn(DataGrid grid, IEnumerable<EditableField> fields) => grid.Columns.Add(new DataGridComboBoxColumn { Header = "항목", ItemsSource = FieldChoices(fields), DisplayMemberPath = "Label", SelectedValuePath = "Key", SelectedValueBinding = DraftBinding("Key"), Width = new DataGridLength(1, DataGridLengthUnitType.Star) });
 private void RefreshFieldChoices()
 {
  if (_rangeGrid.Columns.FirstOrDefault() is DataGridComboBoxColumn range) range.ItemsSource = FieldChoices(_catalog);
  if (_ruleGrid.Columns.FirstOrDefault() is DataGridComboBoxColumn rule) rule.ItemsSource = FieldChoices(_catalog.Where(field => field.Editable));
 }
 private async Task SearchAsync()
 {
  _rangeGrid.CommitEdit(DataGridEditingUnit.Cell, true); _rangeGrid.CommitEdit(DataGridEditingUnit.Row, true);
  string? category = FootballSearchCategories.Supports(Kind) && _footballCategory.SelectedValue is string { Length: > 0 } chosen ? chosen : null;
  if (_nationalityKind != Kind) throw new InvalidOperationException("현재 종류의 검색 조건을 불러오지 못했습니다. 종류를 다시 선택해 주세요.");
  var nationality = SearchNationalityChoices.Supports(Kind) ? SearchNationalityChoices.Resolve(_nations, _nationality.SelectedItem as SearchNationChoice, _nationality.Text) : new SearchNationChoice(null, SearchNationalityChoices.AllLabel);
  SearchNationalityChoices.Validate(Kind, nationality.Id, nationality.Name);
  _nationality.SelectedItem = _nations.FirstOrDefault(choice => choice.Id == nationality.Id); _nationality.Text = nationality.Name;
  var filter = new { nationality_id = nationality.Id, football_category = category, age_min = Whole(_bounds["age_min"].Text, "최소 나이", 150), age_max = Whole(_bounds["age_max"].Text, "최대 나이", 150), ca_min = AbilityVisibility.Current.HideCa ? null : Whole(_bounds["ca_min"].Text, "최소 CA", 200), ca_max = AbilityVisibility.Current.HideCa ? null : Whole(_bounds["ca_max"].Text, "최대 CA", 200), pa_min = AbilityVisibility.Current.HidePa ? null : Whole(_bounds["pa_min"].Text, "최소 PA", 200), pa_max = AbilityVisibility.Current.HidePa ? null : Whole(_bounds["pa_max"].Text, "최대 PA", 200), role = _role.Text.Trim(), unemployed = _unemployed.IsChecked == true, fields = _ranges.Where(draft => !AbilityVisibility.Current.IsHidden(draft.Key)).Select(draft => new { key = draft.Key, min = Number(draft.Minimum, "능력치 최소"), max = Number(draft.Maximum, "능력치 최대") }).ToArray() };
  _queryCancellation?.Dispose(); _queryCancellation = new CancellationTokenSource(); var cancellation = _queryCancellation.Token; var finished = _queryFinished = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously); _queryRunning = true; _searchFilters.IsEnabled = false; _cancelButton.IsEnabled = true; _results.Clear();
  ulong? cursor = null;
  try
  {
   var start = await CallAsync("query_start", new { kind = Kind, query = _query.Text.Trim(), club_id = _ownOnly.IsChecked == true ? _ownClub?.Id : null, filter, sort = AbilityVisibility.Current.HidePa ? "name" : "pa" }); ulong id = start.GetProperty("query_id").GetUInt64(); cursor = _activeQueryId = id;
   while (true)
   {
    cancellation.ThrowIfCancellationRequested(); var step = await CallAsync("query_step", new { query_id = id }, cancellation); foreach (var entity in Read<List<EntityRecord>>(step.GetProperty("items"))) _results.Add(entity);
    int failed = step.GetProperty("failed").GetInt32(); _searchProgress.Text = $"{FootballSearchCategories.Label(category)} · {nationality.Name} · 조회 {step.GetProperty("scanned")} / {step.GetProperty("total")} · 일치 {step.GetProperty("matched")} · 상세 조회 실패 {failed}";
    if (step.TryGetProperty("errors", out var errors) && errors.GetArrayLength() > 0) _searchProgress.ToolTip = string.Join(Environment.NewLine, errors.EnumerateArray().Select(value => value.GetString()));
    if (step.GetProperty("complete").GetBoolean()) { _status.Text = failed == 0 ? $"검색 완료 · {_results.Count:N0}개 결과" : $"검색 완료 · {_results.Count:N0}개 결과 / {failed:N0}개 상세 조회 실패. 실패한 대상은 일치하지 않는 것으로 간주하지 않습니다."; break; }
    await Dispatcher.Yield(DispatcherPriority.Background);
   }
  }
  finally { try { if(cursor is ulong active)await _bridge.CallAsync("query_cancel",new{query_id=active}); } finally { _activeQueryId=null;_queryRunning = false; _searchFilters.IsEnabled = !_stopping && !_connectionInvalidated; _cancelButton.IsEnabled = false; finished.TrySetResult(); } }
 }
 private async Task ExportRowsAsync() { var items = _resultGrid.SelectedItems.Cast<EntityRecord>().ToArray(); if (items.Length is < 1 or > 200) throw new InvalidOperationException("CSV로 내보낼 결과를 1–200개 선택하세요"); if (AbilityVisibility.Current.AnyHidden && AppDialog.Show(this, "CSV 파일에는 화면에서 숨긴 CA·PA 원본 값이 포함됩니다. 내보낼까요?", "원본 수치 내보내기", MessageBoxButton.YesNo, MessageBoxImage.Information) != MessageBoxResult.Yes) return; var dialog = new SaveFileDialog { Filter = "CSV 파일|*.csv", FileName = "touchline-scouting.csv" }; if (dialog.ShowDialog(this) != true) return; await CallAsync("export_rows", new { targets = items.Select(entity => new WorkbenchRef(entity.Kind, entity.Id)), path = dialog.FileName }); _status.Text = "선택 결과를 CSV로 내보냈습니다"; }

 private void BuildComparison() { var dock = new DockPanel(); dock.Children.Add(Docked(Bar(Button("작업 대상 비교 (최대 4명)", CompareAsync)), Dock.Top)); dock.Children.Add(Docked(Text("첫 번째 대상을 기준으로 수치 차이를 함께 표시합니다. 역할이 다른 대상은 같은 항목만 비교하세요."), Dock.Top)); dock.Children.Add(new ScrollViewer { Content = _comparison, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled }); Tab("비교", dock); }
 private async Task CompareAsync()
 {
  var response = await CallAsync("compare", new { targets = Targets(4) }); _comparisonDetails = Read<List<DetailRecord>>(response.GetProperty("details")); RenderComparison();
  _status.Text = $"{_comparisonDetails.Count}명 비교 완료";
 }
 private void RenderComparison()
 {
  var details = _comparisonDetails; _comparisonHeadings.Clear(); _comparison.Children.Clear(); if (details.Count == 0) return;
  var heroes = new Grid(); foreach (var detail in details) heroes.ColumnDefinitions.Add(new());
  for (int i = 0; i < details.Count; i++) { var detail = details[i]; var card = new StackPanel { Margin = new Thickness(6) }; if (LoadImage(detail.ImagePath) is { } image) card.Children.Add(new Image { Source = image, Width = 86, Height = 86, Stretch = Stretch.Uniform, HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 0, 0, 8) }); card.Children.Add(Text(detail.Entity.DisplayName, 16, true)); var summary = Text(ComparisonHeading(detail.Entity)); _comparisonHeadings.Add((summary, detail.Entity)); card.Children.Add(summary); Grid.SetColumn(card, i); heroes.Children.Add(card); } _comparison.Children.Add(heroes);
  foreach (var section in details.SelectMany(detail => detail.Fields).DistinctBy(field => field.Key).GroupBy(field => field.Section))
  {
   _comparison.Children.Add(Text(section.Key, 15, true)); var table = EntityTable(); table.Columns.Clear(); table.MinHeight = 50; table.MaxHeight = 620; Column(table, "항목", "Label", 150, false); for (int i = 0; i < details.Count; i++) Column(table, details[i].Entity.DisplayName, $"Values[{i}]", 1);
   table.ItemsSource = section.Select(field => new ComparisonRow(field.Key, field.Label, details.Select(detail => (double?)detail.Fields.FirstOrDefault(item => item.Key == field.Key)?.Value).ToArray())).ToArray(); _comparison.Children.Add(table);
  }
 }
 private static string ComparisonHeading(EntityRecord entity) => $"{entity.Club}\n{entity.Role} · {entity.AgeText}\nCA / PA {entity.AbilityText}";
 private void RefreshComparison()
 {
  foreach (var (label, entity) in _comparisonHeadings) label.Text = ComparisonHeading(entity);
  foreach (var table in _comparison.Children.OfType<DataGrid>()) table.Items.Refresh();
 }
 private sealed record ComparisonRow(string Key, string Label, double?[] RawValues)
 {
  public override string ToString() => Label + ": " + string.Join(" / ", Values);
  public string[] Values => RawValues.Select((value, index) =>
  {
   if (AbilityVisibility.Current.IsHidden(Key)) return "숨김";
   if (value == null) return "—";
   string text = value.Value.ToString("0.########", CultureInfo.InvariantCulture);
   return index > 0 && RawValues[0] is double baseline ? $"{text} ({value - baseline:+0.########;-0.########;0})" : text;
  }).ToArray();
 }
 private static BitmapImage? LoadImage(string? path) { if (string.IsNullOrEmpty(path) || !File.Exists(path)) return null; try { using var stream = File.OpenRead(path); if (stream.Length > 32 * 1024 * 1024) return null; var image = new BitmapImage(); image.BeginInit(); image.CacheOption = BitmapCacheOption.OnLoad; image.DecodePixelWidth = 200; image.StreamSource = stream; image.EndInit(); image.Freeze(); return image; } catch { return null; } }

 private void BuildLibrary()
 {
  var dock = new DockPanel(); dock.Children.Add(Docked(_libraryStatus, Dock.Top));
  dock.Children.Add(Docked(Bar(Labeled("관심명단", _lists), Labeled("명단 이름", _listName)), Dock.Top));
  dock.Children.Add(Docked(Bar(Button("새 명단", () => { string name = RequiredName(_listName.Text); if (_library.Lists.Any(list => list.Name.Equals(name, StringComparison.OrdinalIgnoreCase))) throw new InvalidOperationException("같은 이름의 명단이 있습니다"); var list = new WorkbenchShortlist { Name = name }; _library.Lists.Add(list); MarkLibraryChanged(); RefreshLibrary(list); return Task.CompletedTask; }), Button("이름 변경", () => { SelectedList().Name = RequiredName(_listName.Text); MarkLibraryChanged(); RefreshLibrary(); return Task.CompletedTask; }), Button("명단 삭제", () => { _library.Lists.Remove(SelectedList()); MarkLibraryChanged(); RefreshLibrary(); return Task.CompletedTask; }), Button("작업 대상 추가", () => { var list = SelectedList(); list.Members = list.Members.Concat(Targets()).Distinct().ToList(); MarkLibraryChanged(); return ResolveListAsync(); }), Button("선택 제거", () => { var list = SelectedList(); var identities = _listGrid.SelectedItems.Cast<EntityRecord>().Select(entity => new WorkbenchRef(entity.Kind, entity.Id)).ToHashSet(); list.Members.RemoveAll(identities.Contains); MarkLibraryChanged(); return ResolveListAsync(); })), Dock.Top));
  dock.Children.Add(Docked(Bar(Button("명단 다시 확인", ResolveListAsync), Button("미확인 대상 제거", RemoveMissingAsync), Button("선택을 작업 대상에 추가", () => { AddTargets(_listGrid.SelectedItems.Cast<EntityRecord>()); return Task.CompletedTask; }), Button("라이브러리 저장", SaveLibraryAsync, true), Button("저장본 다시 읽기", ReloadLibraryAsync), Button("가져오기", ImportLibraryAsync), Button("저장된 라이브러리 내보내기", ExportLibraryAsync)), Dock.Top));
  dock.Children.Add(Docked(Bar(Button("FM 명단 관리 · 가져오기/내보내기", OpenFmShortlistAsync, true)), Dock.Top));
  _lists.SelectionChanged += async (_, _) => { if (_lists.SelectedItem is WorkbenchShortlist list) _listName.Text = list.Name; if (IsLoaded && !_busy) await RunAsync(ResolveListAsync); }; _listGrid.MouseDoubleClick += (_, _) => OpenFrom(_listGrid); dock.Children.Add(_listGrid); Tab("관심명단", dock);
 }
 private async Task OpenFmShortlistAsync()
 {
  if (_libraryDirty) throw new InvalidOperationException("작성 중인 명단·프리셋을 먼저 ‘라이브러리 저장’으로 저장하세요.");
  int version = _resultVersion;
  var dialog = new ShortlistWindow(_bridge, _connectionInvalidated ? [] : _targets.ToArray(), []) { Owner = this };
  dialog.ShowDialog();
  if (_closing) return;
  var saved = await _bridge.CallAsync("library_get"); _libraryBaseline = saved; _library = Read<WorkbenchLibrary>(saved);
  RefreshLibrary(); if (!_connectionInvalidated) await ResolveListAsync();
  _libraryStatus.Text = "FM 파일을 내보낸 뒤 게임의 관심명단 불러오기로 반영하세요. 앱 명단은 별도로 보관됩니다.";
  if (!_connectionInvalidated && version == _resultVersion && dialog.ProfileToOpen is { } entity) _open(entity);
 }
 private static string RequiredName(string name) { name = name.Trim(); if (name.Length is < 1 or > 80 || name.Any(char.IsControl)) throw new InvalidOperationException("이름은 1–80자로 입력하세요"); return name; }
 private WorkbenchShortlist SelectedList() => _lists.SelectedItem as WorkbenchShortlist ?? throw new InvalidOperationException("관심명단을 선택하세요");
 private void RefreshLibrary(WorkbenchShortlist? selected = null) { string? selectedName = (selected ?? _lists.SelectedItem as WorkbenchShortlist)?.Name; _lists.ItemsSource = null; _lists.ItemsSource = _library.Lists; _lists.SelectedItem = _library.Lists.FirstOrDefault(list => list.Name == selectedName) ?? _library.Lists.FirstOrDefault(); _listGrid.ItemsSource = null; _presets.ItemsSource = null; _presets.ItemsSource = _library.Presets; _presets.SelectedIndex = _library.Presets.Count > 0 ? 0 : -1; }
 private void MarkLibraryChanged() { _libraryDirty = true; _libraryStatus.Text = "저장되지 않은 변경이 있습니다. ‘라이브러리 저장’으로 앱에 저장하세요. FM 내부 영입명단은 변경되지 않습니다."; }
 private async Task ResolveListAsync() { if (_lists.SelectedItem is not WorkbenchShortlist list) { _listGrid.ItemsSource = null; return; } var response = await CallAsync("resolve_targets", new { targets = list.Members }); _listGrid.ItemsSource = Read<List<EntityRecord>>(response.GetProperty("items")); AbilitySafeGridSort.Refresh(_listGrid, nameof(EntityRecord.DisplayName), path => AbilityVisibility.Current.IsHidden(path), AbilityVisibility.Current.HidePa); _status.Text = $"{list.Name}: {response.GetProperty("items").GetArrayLength()}명 확인 · 현재 자료에 없는 대상 {response.GetProperty("missing").GetArrayLength()}명"; }
 private async Task SaveLibraryAsync() { var saved = await CallAsync("library_put", new { library = _library, expected = _libraryBaseline }); _libraryBaseline = saved; _library = Read<WorkbenchLibrary>(saved); _libraryDirty = false; RefreshLibrary(); if (!_connectionInvalidated) await ResolveListAsync(); _libraryStatus.Text = "앱 관심명단과 프리셋을 저장했습니다. FM 내부 영입명단과 별도로 보관됩니다."; }
 private async Task ReloadLibraryAsync()
 {
  if (_libraryDirty && AppDialog.Show(this, "저장하지 않은 명단·프리셋 변경을 버리고 최신 저장본을 읽을까요?", "저장본 다시 읽기", MessageBoxButton.YesNo) != MessageBoxResult.Yes) return;
  var saved = await CallAsync("library_get");
  _libraryBaseline = saved; _library = Read<WorkbenchLibrary>(saved); _libraryDirty = false;
  RefreshLibrary(); await ResolveListAsync(); _libraryStatus.Text = "최신 관심명단과 프리셋을 읽었습니다.";
 }
 private async Task RemoveMissingAsync() { var list = SelectedList(); var response = await CallAsync("resolve_targets", new { targets = list.Members }); var missing = Read<List<WorkbenchRef>>(response.GetProperty("missing")).ToHashSet(); list.Members.RemoveAll(missing.Contains); MarkLibraryChanged(); await ResolveListAsync(); _status.Text = $"현재 자료에 없는 {missing.Count}개 대상을 명단 초안에서 제거했습니다. 저장 전까지 파일은 변경되지 않습니다."; }
 private async Task ImportLibraryAsync() { var dialog = new OpenFileDialog { Filter = "Touchline 라이브러리|*.json" }; if (dialog.ShowDialog(this) != true) return; if (_libraryDirty && AppDialog.Show(this, "저장되지 않은 라이브러리 초안을 가져온 자료로 교체할까요?", "라이브러리 가져오기", MessageBoxButton.YesNo) != MessageBoxResult.Yes) return; var response = await CallAsync("library_import", new { path = dialog.FileName }); _library = Read<WorkbenchLibrary>(response.GetProperty("library")); MarkLibraryChanged(); RefreshLibrary(); await ResolveListAsync(); _status.Text = "가져온 라이브러리를 검토하세요. 아직 저장하거나 게임에 적용하지 않았습니다."; }
 private async Task ExportLibraryAsync() { if (_libraryDirty) throw new InvalidOperationException("현재 변경을 먼저 라이브러리에 저장하세요"); var dialog = new SaveFileDialog { Filter = "Touchline 라이브러리|*.json", FileName = "touchline-library.json" }; if (dialog.ShowDialog(this) != true) return; await CallAsync("library_export", new { path = dialog.FileName }); _status.Text = "저장된 라이브러리를 내보냈습니다"; }

 private void BuildPresets()
 {
  var dock = new DockPanel(); dock.Children.Add(Docked(Text("여기서 작성한 규칙을 대량 편집과 수치 고정에서 사용합니다. 범위를 벗어나는 결과는 엔진이 거절합니다."), Dock.Top));
  dock.Children.Add(Docked(Bar(Labeled("저장된 프리셋", _presets), Labeled("프리셋 이름", _presetName), Button("불러오기", LoadPresetAsync), Button("프리셋 저장", SavePresetAsync), Button("삭제", async () => { if (_presets.SelectedItem is not WorkbenchPreset preset) return; _library.Presets.Remove(preset); MarkLibraryChanged(); await SaveLibraryAsync(); })), Dock.Top));
  dock.Children.Add(Docked(Bar(Button("규칙 추가", () => { if (_rules.Count >= 256) throw new InvalidOperationException("규칙은 최대 256개입니다"); var field = _catalog.FirstOrDefault(item => item.Editable && !AbilityVisibility.Current.IsHidden(item.Key) && !_rules.Any(rule => rule.Key == item.Key)) ?? throw new InvalidOperationException("추가할 편집 항목이 없습니다"); _rules.Add(new WorkbenchDraft { Key = field.Key, Value = field.Value.ToString("G17", CultureInfo.InvariantCulture) }); return Task.CompletedTask; }), Button("규칙 제거", () => { if (_ruleGrid.SelectedItem is WorkbenchDraft draft) _rules.Remove(draft); return Task.CompletedTask; }), Button("규칙 비우기", () => { foreach (var draft in _rules.Where(draft => !AbilityVisibility.Current.IsHidden(draft.Key)).ToArray()) _rules.Remove(draft); return Task.CompletedTask; })), Dock.Top));
  dock.Children.Add(Docked(_ruleSummary, Dock.Bottom)); _ruleGrid.ItemsSource = _rules; dock.Children.Add(_ruleGrid); Tab("프리셋 · 규칙", dock);
 }
 private void RuleChanged(object? sender, PropertyChangedEventArgs args) => InvalidateReview();
 private bool HasHiddenRules => _rules.Any(draft => AbilityVisibility.Current.IsHidden(draft.Key));
 private WorkbenchRule[] Rules() { _ruleGrid.CommitEdit(DataGridEditingUnit.Cell, true); _ruleGrid.CommitEdit(DataGridEditingUnit.Row, true); if (HasHiddenRules) throw new InvalidOperationException("숨긴 CA·PA 규칙은 보존되어 있습니다. 해당 수치를 다시 표시한 뒤 검토하거나 편집하세요."); if (_rules.Count == 0) throw new InvalidOperationException("프리셋 · 규칙 탭에서 변경 규칙을 추가하세요"); var rules = _rules.Select(draft => new WorkbenchRule(draft.Key, draft.Operation, Number(draft.Value, "규칙 값") ?? throw new InvalidOperationException("규칙 값을 입력하세요"))).ToArray(); if (rules.Any(rule => string.IsNullOrWhiteSpace(rule.Key)) || rules.Select(rule => rule.Key).Distinct().Count() != rules.Length) throw new InvalidOperationException("필드가 비어 있거나 중복된 규칙이 있습니다"); return rules; }
 private async Task LoadPresetAsync() { if (_presets.SelectedItem is not WorkbenchPreset preset) throw new InvalidOperationException("프리셋을 선택하세요"); int index = _kind.Items.Cast<ComboBoxItem>().ToList().FindIndex(item => item.Tag?.ToString() == preset.Kind); _kind.SelectedIndex = index; _ranges.Clear(); await LoadCatalogAsync(); _rules.Clear(); foreach (var rule in preset.Rules) _rules.Add(new WorkbenchDraft { Key = rule.Key, Operation = rule.Operation, Value = rule.Value.ToString("G17", CultureInfo.InvariantCulture) }); _presetName.Text = preset.Name; _status.Text = "프리셋 규칙을 불러왔습니다. 아직 게임에 적용되지 않았습니다."; }
 private async Task SavePresetAsync() { string name = RequiredName(_presetName.Text); var preset = new WorkbenchPreset { Name = name, Kind = Kind, Rules = Rules().ToList() }; int index = _library.Presets.FindIndex(item => item.Name.Equals(name, StringComparison.OrdinalIgnoreCase)); if (index >= 0) _library.Presets[index] = preset; else _library.Presets.Add(preset); MarkLibraryChanged(); await SaveLibraryAsync(); _status.Text = "프리셋을 앱 라이브러리에 저장했습니다"; }

 private void BuildBatch()
 {
  var grid = new Grid(); grid.RowDefinitions.Add(new() { Height = GridLength.Auto }); grid.RowDefinitions.Add(new()); grid.RowDefinitions.Add(new() { Height = GridLength.Auto }); grid.RowDefinitions.Add(new() { Height = new GridLength(170) });
  var top = new StackPanel(); top.Children.Add(Text("1–200개 대상 · 규칙 변경 후에는 미리보기를 다시 받아야 합니다. 쓰기 전에 전체 원본을 백업합니다.")); _batchApply = Button("백업 후 전체 적용", ApplyBatchAsync); top.Children.Add(Bar(Button("규칙 편집", () => { _tabs.SelectedIndex = 3; return Task.CompletedTask; }), Button("변경 미리보기", () => PreviewAsync(false)), _batchApply)); top.Children.Add(_batchConfirm); top.Children.Add(_batchSummary); grid.Children.Add(top);
  Grid.SetRow(_batchChanges, 1); grid.Children.Add(_batchChanges); _undo = Button("선택 기록 되돌리기", UndoBatchAsync); var historyBar = Bar(Text("대량 변경 이력", 14, true), Button("이력 새로고침", LoadHistoryAsync), _undo); Grid.SetRow(historyBar, 2); grid.Children.Add(historyBar); Grid.SetRow(_historyGrid, 3); grid.Children.Add(_historyGrid); _historyGrid.SelectionChanged += (_, _) => UpdateActions(); Tab("대량 편집", grid);
 }
 private void BuildFreeze()
 {
  var dock = new DockPanel(); var top = new StackPanel(); top.Children.Add(Text("능력치 고정 · 현재 빌드에서 편집 가능한 수치만 지원합니다. 최대 50명의 검토한 목표 값을 3초마다 확인해, 달라진 값만 백업 후 다시 적용합니다. 이 창을 연 동안만 동작하며 창을 닫거나 연결·세이브가 바뀌면 중지됩니다."));
  top.Children.Add(Text("고급 검색에서 작업 대상을 추가한 뒤 ‘규칙 편집’에서 유지할 수치와 목표 값을 정하고, 미리보기와 확인을 거쳐 시작하세요. 스냅샷에서는 미리보기만 가능합니다.")); top.Children.Add(_freezeState); _freezeEnable = Button("검토한 값 고정 시작", EnableFreezeAsync); _freezeStop = new Button { Content = "고정 중지", Margin = new Thickness(0, 2, 7, 2) }; _freezeStop.Click += async (_, _) => { try { await StopFreezeAsync(); } catch (Exception error) { _status.Text = error.Message; } };
  top.Children.Add(Bar(Button("규칙 편집", () => { _tabs.SelectedIndex = 3; return Task.CompletedTask; }), Button("고정할 값 미리보기", () => PreviewAsync(true)), _freezeEnable, _freezeStop)); top.Children.Add(_freezeConfirm); top.Children.Add(_freezeSummary); dock.Children.Add(Docked(top, Dock.Top)); dock.Children.Add(_freezeChanges); _freezeTab = Tab("수치 고정", dock);
 }
 public void OpenFreeze()
 {
  _tabs.SelectedItem = _freezeTab;
  if (_targets.Count == 0) _freezeSummary.Text = "먼저 고급 검색 탭에서 작업 대상을 추가하세요. 현재 보고 있는 프로필이 있으면 오른쪽 ‘현재 대상 추가’를 사용할 수 있습니다.";
 }
 private void InvalidateReview() { _draftVersion++; _batchToken = _freezeToken = null; _batchConfirm.IsChecked = _freezeConfirm.IsChecked = false; _batchChanges.ItemsSource = null; if (!_freezeActive) _freezeChanges.ItemsSource = null; _batchSummary.Text = "대상 또는 규칙이 변경되었습니다. 미리보기를 다시 실행하세요."; _ruleSummary.Text = $"현재 {Kind} 규칙 {_rules.Count}개 · 작업 대상 {_targets.Count}개"; UpdateActions(); }
 private void UpdateActions() { if (_batchApply == null) return; bool ready = !_busy && !_stopping && !_connectionInvalidated; foreach (var button in _buttons) button.IsEnabled = ready || (_connectionInvalidated && !_busy && !_stopping && _offlineButtons.Contains(button)); _searchButton.IsEnabled = ready; _cancelButton.IsEnabled = _queryRunning && !_connectionInvalidated; _batchApply.IsEnabled = _live && ready && !_freezeActive && !HasHiddenRules && _batchToken != null && _batchConfirm.IsChecked == true; _freezeEnable.IsEnabled = _live && ready && !_freezeActive && !HasHiddenRules && _freezeToken != null && _freezeConfirm.IsChecked == true; _freezeStop.IsEnabled = _freezeActive && !_connectionInvalidated; _undo.IsEnabled = _live && ready && !_freezeActive && _historyGrid.SelectedItem is WorkbenchHistory history && !HistoryHasHiddenFields(history) && history.Status.StartsWith("applied", StringComparison.Ordinal); _ruleSummary.Text = $"현재 {Kind} 규칙 {_rules.Count}개 · 작업 대상 {_targets.Count}개" + (HasHiddenRules ? " · 숨긴 CA·PA 규칙 보존됨 · 다시 표시한 뒤 편집/적용 가능" : ""); }
 private bool HistoryHasHiddenFields(WorkbenchHistory history) => _historyFields.TryGetValue(history.Id, out var keys) && keys.Any(AbilityVisibility.Current.IsHidden);
 private sealed record VisibleWorkbenchChange(string Name, string Kind, uint Id, string Field, string Key, string RawBefore, string RawAfter)
 {
  public string Before => AbilityVisibility.Current.Format(Key, RawBefore);
  public string After => AbilityVisibility.Current.Format(Key, RawAfter);
  public override string ToString() => $"{Name} · {Field}: {Before} → {After}";
 }
 private async Task PreviewAsync(bool freeze)
 {
  if (_freezeActive) throw new InvalidOperationException("활성 수치 고정을 먼저 중지하세요"); var targets = Targets(freeze ? 50 : 200); var rules = Rules(); int version = _draftVersion; _batchToken = _freezeToken = null; _batchConfirm.IsChecked = _freezeConfirm.IsChecked = false;
  var response = await CallAsync(freeze ? "freeze_preview" : "batch_preview", new { targets, rules }); if (version != _draftVersion) { _status.Text = "미리보기 중 대상 또는 규칙이 변경되었습니다. 다시 미리 보세요."; return; } var rows = new List<VisibleWorkbenchChange>(); foreach (var item in response.GetProperty("items").EnumerateArray()) { var entity = Read<EntityRecord>(item.GetProperty("entity")); foreach (var change in item.GetProperty(freeze ? "values" : "changes").EnumerateArray()) rows.Add(new(entity.DisplayName, entity.Kind, entity.Id, Str(change, "label"), Str(change, "key"), Str(change, "before"), Str(change, "after"))); }
  string summary = $"{response.GetProperty("items").GetArrayLength()}개 대상 · {rows.Count}개 {(freeze ? "고정 값" : "변경")} · 현재 변경 없음 {response.GetProperty("unchanged").GetArrayLength()}개"; string token = response.GetProperty("token").GetString()!;
  if (freeze) { _freezeChanges.ItemsSource = rows; _freezeSummary.Text = summary; _freezeToken = token; } else { _batchChanges.ItemsSource = rows; _batchSummary.Text = summary; _batchToken = token; }
  if (!_live || !response.GetProperty("can_apply").GetBoolean()) _status.Text = summary + " · 스냅샷에서는 적용할 수 없습니다"; else _status.Text = summary + " · 검토 후 확인란을 선택하세요"; UpdateActions();
 }
 private async Task ApplyBatchAsync() { if (!_live || HasHiddenRules || _batchToken == null || _batchConfirm.IsChecked != true) throw new InvalidOperationException("현재 게임에서 표시된 변경을 먼저 검토하세요"); if (AppDialog.Show(this, _batchSummary.Text + "\n표시한 전체 변경을 현재 게임에 적용할까요?", "대량 변경 최종 확인", MessageBoxButton.YesNo, MessageBoxImage.Warning) != MessageBoxResult.Yes) return; string token = _batchToken; _batchToken = null; var receipt = await CallAsync("batch_apply", new { token }); InvalidateReview(); await LoadHistoryAsync(); _status.Text = "대량 변경 결과: " + Str(receipt, "status"); }
 private async Task LoadHistoryAsync() { var response = await CallAsync("batch_history"); _historyFields.Clear(); foreach (var receipt in response.EnumerateArray()) _historyFields[Str(receipt, "id")] = receipt.GetProperty("items").EnumerateArray().SelectMany(item => item.GetProperty("changes").EnumerateArray().Select(change => Str(change, "key"))).Distinct().ToArray(); _historyGrid.ItemsSource = response.EnumerateArray().Select(receipt => new WorkbenchHistory(Str(receipt, "id"), Str(receipt, "time"), Str(receipt, "status"), receipt.GetProperty("items").GetArrayLength(), receipt.GetProperty("items").EnumerateArray().Sum(item => item.GetProperty("changes").GetArrayLength()))).ToArray(); }
 private async Task UndoBatchAsync() { if (!_live || _historyGrid.SelectedItem is not WorkbenchHistory receipt || HistoryHasHiddenFields(receipt)) throw new InvalidOperationException("현재 게임에서 표시된 수치의 기록을 선택하세요"); if (AppDialog.Show(this, $"{receipt.Targets}개 대상, {receipt.Changes}개 변경의 이전 값으로 되돌릴까요?\n현재 값과 연결 정보가 기록과 일치할 때만 복원합니다.", "대량 변경 되돌리기", MessageBoxButton.YesNo, MessageBoxImage.Warning) != MessageBoxResult.Yes) return; var result = await CallAsync("batch_undo", new { receipt_id = receipt.Id }); InvalidateReview(); await LoadHistoryAsync(); _status.Text = "되돌리기 결과: " + Str(result, "status"); }
 private async Task EnableFreezeAsync() { if (!_live || HasHiddenRules || _freezeToken == null || _freezeConfirm.IsChecked != true) throw new InvalidOperationException("현재 게임에서 표시된 고정 값을 먼저 검토하세요"); if (AppDialog.Show(this, _freezeSummary.Text + "\n활성화한 동안 표시한 목표 값을 유지하도록 게임에 다시 적용할까요?", "수치 고정 시작", MessageBoxButton.YesNo, MessageBoxImage.Warning) != MessageBoxResult.Yes) return; string token = _freezeToken; _freezeToken = _batchToken = null; ShowFreezeStatus(await CallAsync("freeze_enable", new { token })); _freezeTimer.Start(); _status.Text = "수치 고정 활성화 · 첫 확인은 3초 뒤 실행됩니다"; }
 private async Task TickFreezeAsync() { try { ShowFreezeStatus(await CallAsync("freeze_tick")); } catch { _freezeTimer.Stop(); _freezeActive = false; _freezeState.Text = "오류로 고정 중지됨 · 상세 오류는 아래 상태를 확인하세요"; UpdateActions(); throw; } }
 private void ShowFreezeStatus(JsonElement status) { _freezeActive = status.GetProperty("active").GetBoolean(); _freezeState.Text = _freezeActive ? $"활성 · {status.GetProperty("targets").GetArrayLength()}개 대상 · 재적용 {status.GetProperty("write_cycles")}회" : "수치 고정 중지됨"; if (!_freezeActive) _freezeTimer.Stop(); UpdateActions(); }
 private async Task StopFreezeAsync() { _freezeTimer.Stop(); var response = await CallAsync("freeze_stop"); ShowFreezeStatus(response); _freezeToken = null; _freezeConfirm.IsChecked = false; _status.Text = "수치 고정을 중지했습니다"; }
 public async Task StopAsync() { _stopping = true; _queryCancellation?.Cancel(); _freezeTimer.Stop(); UpdateActions(); if (_queryFinished is { } query) await query.Task; if (_actionFinished is { } action) await action.Task; if (!_connectionInvalidated) await StopFreezeAsync(); }
 public async Task<bool> CloseAsync()
 {
  if (_closing) return true;
  if (_libraryDirty && AppDialog.Show(this, "저장되지 않은 라이브러리 변경을 버리고 닫을까요?", "라이브러리 초안", MessageBoxButton.YesNo) != MessageBoxResult.Yes) return false;
  await StopAsync(); _closing = true; Close(); return true;
 }
 private async void OnClosing(object? sender, CancelEventArgs args)
 {
  if (_closing) return; args.Cancel = true; if (_closeRequested) return;
  _closeRequested = true; try { if (!await CloseAsync()) _closeRequested = false; } catch (Exception error) { _status.Text = "고정 중지 확인 실패: " + error.Message; _closeRequested = false; }
 }

 public async Task<object> QaAbilityVisibilityAsync(string? directory = null)
 {
  await _initialized.Task;
  if (_live || _busy || _queryRunning || _current == null) throw new InvalidOperationException("능력치 표시 QA는 대기 중인 스냅샷 대상에서 실행합니다.");
  bool originalCa = AbilityVisibility.Current.HideCa, originalPa = AbilityVisibility.Current.HidePa;
  _busy = true;
  try
  {
   AbilityVisibility.Current.Set(false, false);
   _targets.Clear(); AddTargets([_current]); _query.Text = _current.Id.ToString(CultureInfo.InvariantCulture);
   _ranges.Clear(); _rules.Clear(); foreach (var bound in _bounds.Values) bound.Clear();
   await SearchAsync(); await CompareAsync();
   var entity = _results.Single(person => person.Identity == _current.Identity);
   var ca = _catalog.First(field => AbilityVisibility.AbilityKey(field.Key) == "ca");
   var pa = _catalog.First(field => AbilityVisibility.AbilityKey(field.Key) == "pa");
   // Offline snapshots can omit edit bindings. Exercise the presentation of
   // drafts and previews locally, without creating a writable bridge token.
   ca.Editable = pa.Editable = true; RefreshFieldChoices();
   _rules.Add(new() { Key = ca.Key, Value = ca.Value.ToString("G17", CultureInfo.InvariantCulture) });
   _rules.Add(new() { Key = pa.Key, Value = pa.Value.ToString("G17", CultureInfo.InvariantCulture) });
   _ranges.Add(new() { Key = ca.Key, Minimum = "11", Maximum = "189" }); _bounds["ca_min"].Text = "20";
   var changes = new[] { ca, pa }.Select(field => new VisibleWorkbenchChange(entity.DisplayName, entity.Kind, entity.Id, field.Label, field.Key, field.Value.ToString("G17", CultureInfo.InvariantCulture), field.Value.ToString("G17", CultureInfo.InvariantCulture))).ToArray();
   _freezeChanges.ItemsSource = changes.ToList();
   var raw = (entity.Ca, entity.Pa, ca.Value, pa.Value);
   var results = new List<object>();
   foreach (var (hideCa, hidePa) in new[] { (true, false), (false, true), (true, true), (false, false) })
   {
    AbilityVisibility.Current.Set(hideCa, hidePa);
    _tabs.SelectedIndex = 0;
    foreach (var (key, property, value) in new[] { ("ca", "Ca", entity.Ca), ("pa", "Pa", entity.Pa) })
    {
     var column = _resultGrid.Columns.OfType<DataGridTextColumn>().Single(column => (column.Binding as Binding)?.Path.Path == property);
     string actual = await ReadAbilityQaCellAsync(_resultGrid, entity, column);
     if (actual != AbilityVisibility.Current.Format(key, value)) throw new InvalidOperationException("검색 표 숨김 불일치: " + key);
     if (AbilityVisibility.Current.IsHidden(key) && column.CanUserSort) throw new InvalidOperationException("숨긴 수치 정렬이 활성화되었습니다.");
    }
    if (directory != null) { Directory.CreateDirectory(directory); await CaptureQaTabAsync(directory, 0, $"ability-{hideCa}-{hidePa}-search"); }
    _tabs.SelectedIndex = 1;
    foreach (var table in _comparison.Children.OfType<DataGrid>())
     foreach (var row in table.Items.OfType<ComparisonRow>().Where(row => AbilityVisibility.AbilityKey(row.Key) != null))
     {
      string actual = await ReadAbilityQaCellAsync(table, row, (DataGridTextColumn)table.Columns[1]);
      string expected = AbilityVisibility.Current.IsHidden(row.Key) ? "숨김" : row.RawValues[0]?.ToString("0.########", CultureInfo.InvariantCulture) ?? "—";
      if (actual != expected) throw new InvalidOperationException("비교 표 숨김 불일치: " + row.Key);
     }
    _tabs.SelectedIndex = 5;
    foreach (var change in changes)
     foreach (string property in new[] { "Before", "After" })
     {
      var column = _freezeChanges.Columns.OfType<DataGridTextColumn>().Single(column => (column.Binding as Binding)?.Path.Path == property);
      string actual = await ReadAbilityQaCellAsync(_freezeChanges, change, column);
      if (actual != AbilityVisibility.Current.Format(change.Key, property == "Before" ? change.RawBefore : change.RawAfter)) throw new InvalidOperationException("미리보기 숨김 불일치: " + change.Key);
     }
    if (_rules.Count != 2 || _rules[0].Key != ca.Key || _rules[1].Key != pa.Key || _rules[0].Value != ca.Value.ToString("G17", CultureInfo.InvariantCulture) || _rules[1].Value != pa.Value.ToString("G17", CultureInfo.InvariantCulture) || _ranges.Single().Minimum != "11" || _bounds["ca_min"].Text != "20" || raw != (entity.Ca, entity.Pa, ca.Value, pa.Value)) throw new InvalidOperationException("숨김 변경이 원본 또는 초안을 변경했습니다.");
    if (_ruleGrid.Items.OfType<WorkbenchDraft>().Any(draft => AbilityVisibility.Current.IsHidden(draft.Key)) || _rangeGrid.Items.OfType<WorkbenchDraft>().Any(draft => AbilityVisibility.Current.IsHidden(draft.Key))) throw new InvalidOperationException("숨긴 수치 편집 행이 노출되었습니다.");
    if (_freezeEnable.IsEnabled || _batchApply.IsEnabled) throw new InvalidOperationException("스냅샷에서 쓰기가 활성화되었습니다.");
    results.Add(new { hideCa, hidePa, visibleRules = _ruleGrid.Items.Count, rawAndDraftsPreserved = true, renderedCellsMasked = true });
   }
   return new { success = true, gameWrites = false, results };
  }
  finally { AbilityVisibility.Current.Set(originalCa, originalPa); _busy = false; UpdateActions(); }
 }
 internal static async Task<string> ReadAbilityQaCellAsync(DataGrid grid, object item, DataGridTextColumn column)
 {
  grid.ScrollIntoView(item, column); await grid.Dispatcher.InvokeAsync(grid.UpdateLayout, DispatcherPriority.Loaded); await Task.Delay(30); grid.UpdateLayout();
  if (column.GetCellContent(item) is not TextBlock text) throw new InvalidOperationException("능력치 QA 셀을 렌더링하지 못했습니다: " + column.Header);
  return text.Text;
 }

 public async Task<object> QaPositionOrderAsync(string directory, uint[] ascending, uint[] descending)
 {
  await _initialized.Task;
  if (_live || _busy || _queryRunning) throw new InvalidOperationException("포지션 정렬 QA는 대기 중인 스냅샷에서 실행합니다.");
  _busy = true;
  try
  {
   _kind.SelectedItem = _kind.Items.Cast<ComboBoxItem>().Single(item => item.Tag?.ToString() == "Player");
   await LoadCatalogAsync();
   _footballCategory.SelectedValue = ""; _query.Clear(); _role.Clear(); _ownOnly.IsChecked = _unemployed.IsChecked = false;
   _ranges.Clear(); foreach (var bound in _bounds.Values) bound.Clear();
   await SearchAsync();
   var report = EntityRoleSorting.VerifyGrid(_resultGrid, ascending, descending);
   _listGrid.ItemsSource = _results.ToList();
   var listReport = EntityRoleSorting.VerifyGrid(_listGrid, ascending, descending);
   await CaptureQaTabAsync(directory, 0, "workbench-position-order");
   return new { success = true, report, listReport };
  }
  finally { _busy = false; UpdateActions(); }
 }

 public async Task<object> QaFootballCategoryAsync(string directory)
 {
  await _initialized.Task; if (_live) throw new InvalidOperationException("축구 구분 QA는 스냅샷에서만 실행합니다.");
  if (_busy || _queryRunning) throw new InvalidOperationException("다른 작업이 끝난 뒤 축구 구분 QA를 실행하세요.");
  if (_footballCategory.SelectedValue?.ToString() != "") throw new InvalidOperationException("고급 검색의 기본 축구 구분이 전체가 아닙니다.");
  var reports = new List<object>(); _busy = true;
  try
  {
   _query.Clear(); _role.Clear(); _ownOnly.IsChecked = _unemployed.IsChecked = false; _ranges.Clear(); foreach (var bound in _bounds.Values) bound.Clear();
   foreach (string kind in new[] { "Player", "Staff", "Club" })
   {
    _kind.SelectedItem = _kind.Items.Cast<ComboBoxItem>().Single(item => item.Tag?.ToString() == kind); await LoadCatalogAsync();
    var baseline = new List<EntityRecord>(); int offset = 0;
    while (true)
    {
     var page = await CallAsync("search", new { kind, offset, limit = 500 }); var items = Read<List<EntityRecord>>(page.GetProperty("items")); baseline.AddRange(items); offset += items.Count;
     if (offset >= page.GetProperty("total").GetInt32()) break; if (items.Count == 0) throw new InvalidOperationException("축구 구분 기준 명단을 모두 읽지 못했습니다.");
    }
    foreach (string category in new[] { "men", "women", "" })
    {
     _footballCategory.SelectedValue = category; await SearchAsync();
     uint[] expected = baseline.Where(person => category.Length == 0 || person.FootballCategory == category).Select(person => person.Id).Order().ToArray();
     uint[] actual = _results.Select(person => person.Id).Order().ToArray();
     if (!actual.SequenceEqual(expected) || _results.Any(person => person.Kind != kind || category.Length > 0 && person.FootballCategory != category)) throw new InvalidOperationException("고급 검색 축구 구분 누락/혼입 · " + kind + " / " + category);
     reports.Add(new { kind, category = category.Length == 0 ? "all" : category, ids = actual, count = actual.Length });
     if (kind == "Staff" && category != "men") await CaptureQaTabAsync(directory, 0, "workbench-football-" + (category.Length == 0 ? "all" : category));
    }
   }
   if (!_resultGrid.Columns.OfType<DataGridTextColumn>().Any(column => (column.Binding as Binding)?.Path.Path == nameof(EntityRecord.FootballCategoryLabel))) throw new InvalidOperationException("고급 검색 결과에 축구 구분 열이 없습니다.");
   return new { success = true, live = false, gameWrites = false, categoryDefaultAll = true, allRestoresUnknown = true, categoryColumn = true, results = reports };
  }
  finally { _busy = false; UpdateActions(); }
 }

 public async Task<object> QaNationalityAsync(string directory)
 {
  await _initialized.Task;
  if (_live || _busy || _queryRunning) throw new InvalidOperationException("주 국적 QA는 대기 중인 스냅샷에서만 실행합니다.");
  var reports = new List<object>(); _busy = true;
  try
  {
   _query.Clear(); _role.Clear(); _ownOnly.IsChecked = _unemployed.IsChecked = false; _ranges.Clear(); foreach (var bound in _bounds.Values) bound.Clear(); _footballCategory.SelectedValue = "";
   foreach (string kind in new[] { "Player", "Staff" })
   {
    _nationality.SelectedItem = _nations.First(choice => choice.Id == null); _nationality.Text = SearchNationalityChoices.AllLabel;
    _kind.SelectedItem = _kind.Items.Cast<ComboBoxItem>().Single(item => item.Tag?.ToString() == kind); await LoadCatalogAsync();
    if (_nationalityControl.Visibility != Visibility.Visible || _nationalityKind != kind) throw new InvalidOperationException("고급 검색의 주 국적 선택기가 없습니다.");
    var baseline = new List<EntityRecord>(); int offset = 0;
    while (true)
    {
     var page = await CallAsync("search", new { kind, offset, limit = 500 }); var items = Read<List<EntityRecord>>(page.GetProperty("items")); baseline.AddRange(items); offset += items.Count;
     if (offset >= page.GetProperty("total").GetInt32()) break; if (items.Count == 0) throw new InvalidOperationException("주 국적 기준 명단을 모두 읽지 못했습니다.");
    }
    var choices = _nations.Where(choice => choice.Id.HasValue).Take(2).ToArray();
    if (choices.Length == 0) throw new InvalidOperationException("주 국적 QA fixture에 국적이 확인된 인물이 필요합니다: " + kind);
    foreach (var choice in choices)
    {
     _nationality.SelectedItem = choice; _nationality.Text = choice.Name; await SearchAsync();
     uint[] expected = baseline.Where(person => person.NationalityId == choice.Id).Select(person => person.Id).Order().ToArray();
     uint[] actual = _results.Select(person => person.Id).Order().ToArray();
     if (!actual.SequenceEqual(expected) || _results.Any(person => person.Kind != kind || person.NationalityId != choice.Id)) throw new InvalidOperationException("고급 검색 국적 필터 누락/혼입: " + kind);
     reports.Add(new { kind, nationality = choice.Id, count = actual.Length });
     var sample = _results.FirstOrDefault() ?? throw new InvalidOperationException("주 국적 목록에 결과 없는 국가가 있습니다.");
     _query.Text = sample.Id.ToString(CultureInfo.InvariantCulture); await SearchAsync();
     if (_results.Count != 1 || _results[0].Identity != sample.Identity) throw new InvalidOperationException("주 국적과 UID 복합 검색 실패.");
     _query.Clear(); await SearchAsync();
    }
    var prior = _results.Select(person => person.Identity).ToArray(); _nationality.SelectedItem = null; _nationality.Text = "__unknown_nationality__";
    bool rejected = false; try { await SearchAsync(); } catch (InvalidOperationException) { rejected = true; }
    if (!rejected || _queryRunning || !prior.SequenceEqual(_results.Select(person => person.Identity))) throw new InvalidOperationException("잘못 입력한 국적이 전체 검색으로 바뀌었습니다.");
    _nationality.SelectedItem = _nations.First(choice => choice.Id == null); _nationality.Text = SearchNationalityChoices.AllLabel; await SearchAsync();
    if (!_results.Select(person => person.Id).Order().SequenceEqual(baseline.Select(person => person.Id).Order())) throw new InvalidOperationException("모든 국적 선택이 미확인 국적을 복원하지 못했습니다.");
    _nationality.SelectedItem = choices[0]; _nationality.Text = choices[0].Name; await SearchAsync();
    await CaptureQaTabAsync(directory, 0, "workbench-nationality-" + kind.ToLowerInvariant());
   }
   _kind.SelectedItem = _kind.Items.Cast<ComboBoxItem>().Single(item => item.Tag?.ToString() == "Club"); await LoadCatalogAsync();
   if (_nationalityControl.Visibility != Visibility.Collapsed || (_nationality.SelectedItem as SearchNationChoice)?.Id != null) throw new InvalidOperationException("구단 검색에 인물 국적 조건이 남았습니다.");
   _kind.SelectedItem = _kind.Items.Cast<ComboBoxItem>().Single(item => item.Tag?.ToString() == "Player"); await LoadCatalogAsync();
   if (_nationalityControl.Visibility != Visibility.Visible || _nationalityKind != "Player" || (_nationality.SelectedItem as SearchNationChoice)?.Id != null) throw new InvalidOperationException("인물 검색 복귀 후 국적 선택기 복원 실패.");
   if (!_resultGrid.Columns.OfType<DataGridTextColumn>().Any(column => column.Visibility == Visibility.Visible && (column.Binding as Binding)?.Path.Path == nameof(EntityRecord.NationalityLabel)) || !_listGrid.Columns.OfType<DataGridTextColumn>().Any(column => (column.Binding as Binding)?.Path.Path == nameof(EntityRecord.NationalityLabel))) throw new InvalidOperationException("고급 검색/관심명단 국적 열이 없습니다.");
   return new { success = true, live = false, gameWrites = false, exactPrimaryNationality = true, unknownTextRejected = true, allRestoresUnknown = true, combinedUid = true, unsupportedKindClearsFilter = true, nationalityColumns = true, results = reports };
  }
  finally { _busy = false; UpdateActions(); }
 }

 public async Task<object> QaAsync()
 {
  await _initialized.Task; if (_live) throw new InvalidOperationException("Workbench QA는 스냅샷으로만 실행합니다"); if (_current == null) throw new InvalidOperationException("QA 대상을 선택하세요");
  string? output = App.Arg("--ui-check"); string? directory = output == null ? null : Path.Combine(Path.GetDirectoryName(Path.GetFullPath(output))!, "workbench-tabs"); if (directory != null) Directory.CreateDirectory(directory);
  _targets.Clear(); AddTargets([_current]); _query.Text = _current.Id.ToString(CultureInfo.InvariantCulture); bool cancelled = false; var cancelledSearch = SearchAsync(); _queryCancellation?.Cancel(); try { await cancelledSearch; } catch (OperationCanceledException) { cancelled = true; } if (!cancelled) throw new InvalidOperationException("검색 취소가 적용되지 않았습니다");
  await SearchAsync(); if (!_results.Any(entity => entity.Identity == _current.Identity)) throw new InvalidOperationException("검색 결과에 QA 대상이 없습니다"); await CaptureQaTabAsync(directory, 0, "search");
  var field = _catalog.FirstOrDefault(item => item.Editable && item.Step > 0 && (item.Value + item.Step <= item.Max || item.Value - item.Step >= item.Min)) ?? throw new InvalidOperationException("QA 편집 항목이 없습니다");
  _rules.Clear(); _rules.Add(new WorkbenchDraft { Key = field.Key, Operation = "set", Value = (field.Value + field.Step <= field.Max ? field.Value + field.Step : field.Value - field.Step).ToString("G17", CultureInfo.InvariantCulture) }); await PreviewAsync(false); _batchConfirm.IsChecked = true; UpdateActions(); if (_batchApply.IsEnabled) throw new InvalidOperationException("스냅샷에서 대량 적용이 활성화되었습니다"); bool reviewed = _batchToken != null; await CaptureQaTabAsync(directory, 4, "batch-review");
  _rules[0].Value = field.Value.ToString("G17", CultureInfo.InvariantCulture); if (_batchToken != null) throw new InvalidOperationException("규칙 변경이 검토를 무효화하지 않았습니다"); await PreviewAsync(true); _freezeConfirm.IsChecked = true; UpdateActions(); if (_freezeToken == null || _freezeEnable.IsEnabled || (_freezeChanges.ItemsSource as List<VisibleWorkbenchChange>)?.Count != 1) throw new InvalidOperationException("현재 값 고정 검토 또는 스냅샷 보호가 올바르지 않습니다"); await CaptureQaTabAsync(directory, 5, "freeze-review");
  await CaptureQaTabAsync(directory, 2, "shortlists"); await CaptureQaTabAsync(directory, 3, "presets"); await CompareAsync(); await LoadHistoryAsync(); await CaptureQaTabAsync(directory, 1, "comparison"); if (_targets.Count != 1 || _targets[0].Identity != _current.Identity) throw new InvalidOperationException("탭 변경 중 작업 대상이 변경되었습니다");
  return new { success = true, gameWrites = false, libraryWrites = false, checks = new[] { "incremental_search", "search_cancellation_drained", "persistent_target_selection", "comparison", "batch_preview", "rule_change_invalidates_preview", "unchanged_value_freeze_preview", "offline_apply_and_freeze_disabled_after_confirmation", "batch_history_read" }, reviewed, results = _results.Count, embeddedFont = FontFamily.Source, rendering = TextOptions.GetTextRenderingMode(this).ToString(), captures = directory };
 }

 private async Task CaptureQaTabAsync(string? directory, int index, string name)
 {
  _tabs.SelectedIndex = index; await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded); await Task.Delay(80); UpdateLayout(); if (directory == null) return;
  var grids = VisualChildren<DataGrid>(this).Where(grid => grid.IsVisible && grid.ActualWidth > 100).Select(grid => new { width = grid.ActualWidth, columns = grid.Columns.Where(column => column.Visibility == Visibility.Visible).Select(column => new { header = column.Header?.ToString(), width = column.ActualWidth }).ToArray() }).ToArray();
  File.WriteAllText(Path.Combine(directory, name + ".json"), JsonSerializer.Serialize(grids, new JsonSerializerOptions { WriteIndented = true }));
  if (grids.Any(grid => grid.columns.Any(column => column.width < 35))) throw new InvalidOperationException(name + ": 표 열 너비가 충분하지 않습니다");
  var bitmap = new RenderTargetBitmap((int)Math.Ceiling(ActualWidth), (int)Math.Ceiling(ActualHeight), 96, 96, PixelFormats.Pbgra32); bitmap.Render(this);
  var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(Path.Combine(directory, name + ".png")); encoder.Save(stream);
 }
 private static IEnumerable<T> VisualChildren<T>(DependencyObject parent) where T : DependencyObject { for (int i = 0; i < VisualTreeHelper.GetChildrenCount(parent); i++) { var child = VisualTreeHelper.GetChild(parent, i); if (child is T typed) yield return typed; foreach (var descendant in VisualChildren<T>(child)) yield return descendant; } }
}

/// <summary>Temporarily replaces a hidden-value ranking while retaining the user's visible sort.</summary>
internal static class AbilitySafeGridSort
{
 private sealed record SavedSort(ListCollectionView View, SortDescription[] Sorts, IComparer? Comparer, Dictionary<DataGridColumn, ListSortDirection?> Directions, string[] Keys, bool DefaultOrder);
 private static readonly ConditionalWeakTable<DataGrid, SavedSort> Saved = new();
 public static void Attach(DataGrid grid) => grid.Sorting += UserSorted;
 private static void UserSorted(object sender, DataGridSortingEventArgs args) { if (sender is DataGrid grid) Saved.Remove(grid); }
 public static void Refresh(DataGrid grid, string fallback, Func<string, bool> hidden, bool defaultHidden = false)
 {
  if (grid.ItemsSource == null || CollectionViewSource.GetDefaultView(grid.ItemsSource) is not ListCollectionView view) return;
  if (Saved.TryGetValue(grid, out var saved))
  {
   if (!ReferenceEquals(saved.View, view)) Saved.Remove(grid);
   else
   {
    if (saved.Keys.Any(hidden) || saved.DefaultOrder && defaultHidden) return;
    using (view.DeferRefresh())
    {
     view.SortDescriptions.Clear(); view.CustomSort = saved.Comparer;
     if (saved.Comparer == null) foreach (var sort in saved.Sorts) view.SortDescriptions.Add(sort);
     foreach (var column in grid.Columns) column.SortDirection = saved.Directions.GetValueOrDefault(column);
    }
    Saved.Remove(grid); return;
   }
  }
  string[] keys = view.SortDescriptions.Select(sort => sort.PropertyName).Concat(grid.Columns.Where(column => column.SortDirection.HasValue).Select(column => column.SortMemberPath)).Distinct().ToArray();
  bool defaultOrder = view.SortDescriptions.Count == 0 && view.CustomSort == null && keys.Length == 0;
  if (!keys.Any(hidden) && !(defaultOrder && defaultHidden)) return;
  Saved.Add(grid, new(view, view.SortDescriptions.ToArray(), view.CustomSort, grid.Columns.ToDictionary(column => column, column => column.SortDirection), keys, defaultOrder));
  using (view.DeferRefresh())
  {
   view.CustomSort = null; view.SortDescriptions.Clear(); view.SortDescriptions.Add(new(fallback, ListSortDirection.Ascending));
   foreach (var column in grid.Columns) column.SortDirection = column.SortMemberPath == fallback ? ListSortDirection.Ascending : null;
  }
 }
}
