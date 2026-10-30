using System.Collections;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

/// <summary>Main search workspace. Each search owns its cursor and never edits game data.</summary>
public sealed partial class SearchView : UserControl
{
 private readonly BridgeClient _bridge;
 private readonly bool _live;
 private readonly EntityRecord? _ownClub;
 private readonly Func<EntityRecord, Task> _open;
 private readonly ObservableCollection<SearchRow> _rows = [];
 private readonly List<EditableField> _catalog = [], _columnCatalog = [];
 private readonly List<SearchChoice> _roles = [], _groups = [];
 private readonly List<string> _errors = [];
 private readonly HashSet<string> _loadedFields = [];
 private readonly ListCollectionView _resultView;
 private readonly TextBox _name = Input();
 private readonly ComboBox _scope = Select(), _sort = Select(), _view = Select();
 private readonly WrapPanel _chips = new();
 private readonly WrapPanel _footballBar = new() { Margin = new Thickness(12, 0, 0, 0), VerticalAlignment = VerticalAlignment.Center };
 private readonly Dictionary<string, Button> _footballButtons = [];
 private readonly TextBlock _title = Text("검색", 24, true), _subtitle = Text("새 검색에서 조건을 설정하세요."), _count = Text("검색 준비", 17, true), _status = Text("새 검색에서 조건을 설정하거나 이름으로 검색하세요."), _empty = Text("새 검색을 눌러 검색 조건을 설정하세요.", 15, true), _errorText = Text("", 12);
 private readonly ProgressBar _progress = new() { Height = 3, Minimum = 0, Maximum = 1, Visibility = Visibility.Collapsed };
 private readonly Border _emptyPanel, _errorPanel;
 private readonly DataGrid _grid;
 private readonly Grid _toolbar;
 private readonly Border _tableCard;
 private readonly Button _search, _cancel, _reset, _newSearch, _editSearch, _save, _load, _direction, _openButton;
 private SearchCriteria _criteria = new();
 private string _kind = "", _sortKey = "ca";
 private EntityRecord? _sample;
 private bool _initialized, _loading, _searching, _suppress, _descending = true, _lastCancelled, _complete, _resultsInvalidated;
 private int _epoch, _scanned, _total, _matched, _failed, _navigationRequests;
 private ulong? _queryId;
 private CancellationTokenSource? _cancellation;
 private TaskCompletionSource? _searchFinished, _loadFinished;
 private Action? _qaAfterStart;
 private Window? _activeDialog;

 public SearchView(BridgeClient bridge, bool live, EntityRecord? ownClub, Func<EntityRecord, Task> open)
 {
  _bridge = bridge; _live = live; _ownClub = ownClub; _open = open;
  _columnSettings = new(SearchColumnsPath());
  FontFamily = Typography.AppFont; FontWeight = FontWeights.Medium; FontSize = 13;
  Foreground = Brush("InkBrush"); UseLayoutRounding = true; SnapsToDevicePixels = true;
  TextOptions.SetTextFormattingMode(this, TextFormattingMode.Display);
  SetResourceReference(TextOptions.TextRenderingModeProperty, "AppTextRenderingMode");
  _resultView = new ListCollectionView(_rows);
  var root = new Grid { Margin = new Thickness(20, 16, 20, 16) };
  for (int i = 0; i < 5; i++) root.RowDefinitions.Add(new() { Height = GridLength.Auto });
  root.RowDefinitions.Add(new()); root.RowDefinitions.Add(new() { Height = GridLength.Auto });
  var heading = new DockPanel { Margin = new Thickness(0, 0, 0, 12) };
  _count.HorizontalAlignment = HorizontalAlignment.Right; _count.VerticalAlignment = VerticalAlignment.Center; DockPanel.SetDock(_count, Dock.Right); heading.Children.Add(_count);
  var titles = new StackPanel(); titles.Children.Add(_title); _subtitle.Margin = new Thickness(0, 3, 0, 0); titles.Children.Add(_subtitle); heading.Children.Add(titles); root.Children.Add(heading);

  var commands = new WrapPanel { Margin = new Thickness(0, 0, 0, 7) };
  _newSearch = Button("새 검색", () => SafeAsync(() => EditCriteriaAsync(true))); _newSearch.Style = TryFindResource("PrimaryButton") as Style; _newSearch.FontWeight = FontWeights.Bold;
  _editSearch = Button("검색 편집 (0)", () => SafeAsync(() => EditCriteriaAsync(false)));
  _reset = Button("모두 지우기", () => SafeAsync(ResetAsync));
  _save = Button("검색 저장", () => SafeAsync(() => PresetDialogAsync(true)));
  _load = Button("검색 불러오기", () => SafeAsync(() => PresetDialogAsync(false)));
  foreach (var button in new[] { _newSearch, _editSearch, _reset, _save, _load }) { button.Margin = new Thickness(0, 0, 6, 5); commands.Children.Add(button); }
  var footballCaption = Text("축구 구분", 12); footballCaption.Margin = new Thickness(0, 0, 8, 5); footballCaption.VerticalAlignment = VerticalAlignment.Center; footballCaption.ToolTip = FootballSearchCategories.Hint; _footballBar.Children.Add(footballCaption);
  foreach (var choice in FootballSearchCategories.Choices)
  {
   var button = Button(choice.Label, () => SafeAsync(() => ApplyFootballCategoryAsync(choice.Key.Length == 0 ? null : choice.Key)));
   button.Name = "SearchFootball" + (choice.Key.Length == 0 ? "All" : choice.Key); button.Margin = new Thickness(0, 0, 5, 5); button.Padding = new Thickness(10, 7, 10, 7); button.ToolTip = FootballSearchCategories.Hint;
   _footballButtons.Add(choice.Key, button); _footballBar.Children.Add(button);
  }
  commands.Children.Add(_footballBar);
  Grid.SetRow(commands, 1); root.Children.Add(commands);

  var quick = new Grid { Margin = new Thickness(0, 0, 0, 8) };
  quick.ColumnDefinitions.Add(new()); quick.ColumnDefinitions.Add(new() { Width = new GridLength(190) }); quick.ColumnDefinitions.Add(new() { Width = GridLength.Auto }); quick.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
  _name.ToolTip = "한글 이름, 영문 이름 또는 정확한 UID · Enter로 검색"; _name.Padding = new Thickness(10, 6, 10, 6); var nameEntry = new DockPanel(); var nameCaption = Text("이름 · 영문 이름 · UID", 12); nameCaption.VerticalAlignment = VerticalAlignment.Center; nameCaption.Margin = new Thickness(0, 0, 10, 0); DockPanel.SetDock(nameCaption, Dock.Left); nameEntry.Children.Add(nameCaption); nameEntry.Children.Add(_name); quick.Children.Add(nameEntry);
  _scope.Items.Add(new SearchChoice("all", "전체 데이터베이스")); if (ownClub != null) _scope.Items.Add(new SearchChoice("own", "우리 구단")); _scope.SelectedIndex = 0; _scope.Margin = new Thickness(8, 0, 0, 0); Grid.SetColumn(_scope, 1); quick.Children.Add(_scope);
  _search = Button("검색", () => SafeAsync(ApplyQuickAsync)); _search.Margin = new Thickness(8, 0, 0, 0); Grid.SetColumn(_search, 2); quick.Children.Add(_search);
  _cancel = Button("중지", () => SafeAsync(StopAsync)); _cancel.Margin = new Thickness(6, 0, 0, 0); _cancel.IsEnabled = false; Grid.SetColumn(_cancel, 3); quick.Children.Add(_cancel);
  Grid.SetRow(quick, 2); root.Children.Add(quick);

  var summary = new StackPanel { Margin = new Thickness(0, 0, 0, 8) };
  summary.Children.Add(new ScrollViewer { Content = _chips, MaxHeight = 74, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled });
  _progress.Margin = new Thickness(0, 5, 0, 0); summary.Children.Add(_progress); Grid.SetRow(summary, 3); root.Children.Add(summary);
  var toolbar = _toolbar = new Grid { Margin = new Thickness(0, 0, 0, 8) }; toolbar.ColumnDefinitions.Add(new()); toolbar.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
  var views = new StackPanel { Orientation = Orientation.Horizontal }; var viewLabel = Text("보기", 12); viewLabel.VerticalAlignment = VerticalAlignment.Center; viewLabel.Margin = new Thickness(0, 0, 8, 0); views.Children.Add(viewLabel); _view.Width = 150; views.Children.Add(_view); _columnsButton = Button("열 설정", () => SafeAsync(EditColumnsAsync)); _columnsButton.Margin = new Thickness(8,0,0,0); views.Children.Add(_columnsButton); toolbar.Children.Add(views);
  var sorting = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right };
  var sortLabel = Text("정렬", 12); sortLabel.VerticalAlignment = VerticalAlignment.Center; sortLabel.Margin = new Thickness(0, 0, 8, 0); sorting.Children.Add(sortLabel); _sort.Width = 185; sorting.Children.Add(_sort);
  _direction = Button("내림차순 ↓", () => { _descending = !_descending; UpdateDirection(); ApplySort(); return Task.CompletedTask; }); _direction.Margin = new Thickness(6, 0, 0, 0); _direction.MinWidth = 92; sorting.Children.Add(_direction); Grid.SetColumn(sorting, 1); toolbar.Children.Add(sorting); Grid.SetRow(toolbar, 4); root.Children.Add(toolbar);

  _grid = new DataGrid { ItemsSource = _resultView, AutoGenerateColumns = false, CanUserAddRows = false, CanUserDeleteRows = false, IsReadOnly = true, SelectionMode = DataGridSelectionMode.Single, SelectionUnit = DataGridSelectionUnit.FullRow, EnableRowVirtualization = true, EnableColumnVirtualization = true, RowHeight = 43, ColumnHeaderHeight = 35, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, BorderThickness = new Thickness(0) };
  VirtualizingPanel.SetVirtualizationMode(_grid, VirtualizationMode.Recycling); ScrollViewer.SetCanContentScroll(_grid, true);
  NameCopy.AttachGrid(_grid, item => (item as SearchRow)?.Entity, message => _status.Text = message);
  _grid.ToolTip = "우클릭: 이름 복사 · Ctrl+C / Ctrl+Shift+C";
  _grid.MouseDoubleClick += async (_, args) => { if (FindRow(args.OriginalSource as DependencyObject) != null) await SafeAsync(OpenSelectedAsync); };
  _grid.PreviewKeyDown += async (_, args) => { if (args.Key == Key.Enter) { args.Handled = true; await SafeAsync(OpenSelectedAsync); } };
  _grid.Sorting += GridSorting;
  _grid.FrozenColumnCount = 1; _grid.ColumnReordered += (_, _) => SaveReorderedColumns();
  var table = new Grid(); table.Children.Add(_grid); _emptyPanel = new Border { Background = Brush("PanelBrush"), Child = _empty, Padding = new Thickness(24), IsHitTestVisible = false }; _empty.HorizontalAlignment = HorizontalAlignment.Center; _empty.VerticalAlignment = VerticalAlignment.Center; _empty.TextAlignment = TextAlignment.Center; table.Children.Add(_emptyPanel);
  _errorPanel = new Border { Background = Solid("#302923"), BorderBrush = Solid("#705A3A"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(6), Padding = new Thickness(10), Margin = new Thickness(10), VerticalAlignment = VerticalAlignment.Bottom, Visibility = Visibility.Collapsed, Child = new ScrollViewer { Content = _errorText, MaxHeight = 85, VerticalScrollBarVisibility = ScrollBarVisibility.Auto } }; table.Children.Add(_errorPanel);
  _tableCard = Card(table); Grid.SetRow(_tableCard, 5); root.Children.Add(_tableCard);
  var footer = new DockPanel { Margin = new Thickness(0, 9, 0, 0) }; _openButton = Button("선택한 프로필 열기 ↗", () => SafeAsync(OpenSelectedAsync)); _openButton.IsEnabled = false; DockPanel.SetDock(_openButton, Dock.Right); footer.Children.Add(_openButton); _status.Margin = new Thickness(0, 0, 10, 0); _status.MaxHeight = 45; var footerText = new StackPanel(); footerText.Children.Add(_status); footerText.Children.Add(Text("우클릭: 이름 복사 · Ctrl+C / Ctrl+Shift+C", 11)); footer.Children.Add(footerText); Grid.SetRow(footer, 6); root.Children.Add(footer);
  _grid.SelectionChanged += (_, _) => _openButton.IsEnabled = _grid.SelectedItem is SearchRow;
  _name.TextChanged += (_, _) => QuickChanged(); _scope.SelectionChanged += (_, _) => QuickChanged();
  _name.KeyDown += async (_, args) => { if (args.Key == Key.Enter && !_searching) { args.Handled = true; await SafeAsync(ApplyQuickAsync); } };
  _sort.SelectionChanged += async (_, _) => { if (_suppress) return; string next = Selected(_sort, "ca"); if (next == "role" && _sortKey != next) { _descending = false; UpdateDirection(); } _sortKey = next; await SafeAsync(RefreshColumnsAsync); };
  _view.SelectionChanged += async (_, _) => { if (!_suppress) await SafeAsync(async () => { SaveColumnPreferences(); await RefreshColumnsAsync(); }); };
  Unloaded += (_, _) => { try { SaveColumnPreferences(); } catch (Exception ex) { App.Log(ex); } };
  Content = root; UpdateButtons();
 }

 private sealed class SearchRow
 {
  public EntityRecord Entity { get; init; } = new();
  public Dictionary<string, double> Values { get; init; } = [];
  public string Name => Entity.DisplayName;
  public string EnglishName => Entity.EnglishName == Name ? "" : Entity.EnglishName;
  public uint? Age => Entity.Age;
  public uint Id => Entity.Id;
  public string OriginalName => Entity.EnglishName;
  public string Nationality => Entity.NationalityLabel;
  public string Group => Entity.Group;
  public string Club => string.IsNullOrWhiteSpace(Entity.Club) ? "무소속" : Entity.Club;
  public string Role => Entity.Role;
  public ushort? Ca => Entity.Ca;
  public ushort? Pa => Entity.Pa;
  public string FootballCategoryLabel => Entity.FootballCategoryLabel;
  public object? SortValue(string key) => key switch { "name" => Name, "english_name" => OriginalName, "id" => Id, "nationality" => string.IsNullOrWhiteSpace(Entity.Nationality) ? null : Entity.Nationality, "group" => Group, "football_category" => FootballCategoryLabel, "age" => Age, "club" => Club, "role" => Role, "ca" => Ca, "pa" => Pa, _ => Values.TryGetValue(key, out var value) ? value : null };
 }
 private sealed class RowComparer(string key, bool descending) : IComparer
 {
  public int Compare(object? x, object? y)
  {
   if (x is not SearchRow a || y is not SearchRow b) return 0;
   if (key == "role") return PositionOrder.Compare(a.Entity, b.Entity, descending);
   object? av = a.SortValue(key), bv = b.SortValue(key);
   if (av == null || bv == null) return av == null ? bv == null ? a.Entity.Id.CompareTo(b.Entity.Id) : 1 : -1;
   int result = av is string text ? StringComparer.CurrentCultureIgnoreCase.Compare(text, (string)bv) : Convert.ToDouble(av, CultureInfo.InvariantCulture).CompareTo(Convert.ToDouble(bv, CultureInfo.InvariantCulture));
   return result == 0 ? a.Entity.Id.CompareTo(b.Entity.Id) : descending ? -result : result;
  }
 }
 private static TextBox Input() => new() { MinHeight = 32, Padding = new Thickness(8, 5, 8, 5), FontWeight = FontWeights.Medium };
 private static ComboBox Select() => new() { MinHeight = 32, DisplayMemberPath = "Label", SelectedValuePath = "Key", IsTextSearchEnabled = true, MaxDropDownHeight = 360, FontWeight = FontWeights.Medium };
 private static SolidColorBrush Solid(string value) => new((Color)ColorConverter.ConvertFromString(value));
 private static Brush Brush(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Text(string value, double size = 12, bool bold = false) => new() { Text = value, FontSize = size, FontWeight = bold ? FontWeights.Bold : FontWeights.Medium, TextWrapping = TextWrapping.Wrap, Foreground = Brush(bold ? "InkBrush" : "MutedBrush") };
 private static Button Button(string label, Func<Task> action)
 {
  var button = new Button { Content = label, Padding = new Thickness(10, 7, 10, 7), FontWeight = FontWeights.Medium };
  button.Click += async (_, _) => await action(); return button;
 }
 private static Border Card(UIElement child) => new() { Child = child, Background = Brush("PanelBrush"), BorderBrush = Brush("BorderBrush"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(8) };
 private static FrameworkElement Labeled(string label, FrameworkElement input)
 {
  var panel = new StackPanel { Margin = new Thickness(0, 0, 0, 11) }; var caption = Text(label, 12); caption.Margin = new Thickness(0, 0, 0, 5); panel.Children.Add(caption); panel.Children.Add(input); return panel;
 }
 private static string Selected(ComboBox selector, string fallback = "") => selector.SelectedValue as string ?? fallback;
 private static string Label(ComboBox selector) => (selector.SelectedItem as SearchChoice)?.Label ?? "";
 private static T Read<T>(JsonElement value) => value.Deserialize<T>(JsonDefaults.Options) ?? throw new InvalidOperationException("검색 응답이 비어 있습니다.");
 private static DataGridRow? FindRow(DependencyObject? value) { while (value != null) { if (value is DataGridRow row) return row; value = value is Visual ? VisualTreeHelper.GetParent(value) : null; } return null; }
 private string KindLabel => _kind switch { "Staff" => "스태프", "Player" => "선수", "Club" => "구단", "City" => "도시", _ => "인물" };

 public SearchReloadState CaptureReloadState() => new(_kind, _criteria.Clone(), _sortKey, _descending);
 public async Task RestoreReloadStateAsync(SearchReloadState state)
 {
  await SetKindAsync(state.Kind);
  _sortKey = state.SortKey; _descending = state.Descending;
  if (!_columnCatalog.Any(field => field.Key == _sortKey) && !IsMetadata(_sortKey)) _sortKey = "name";
  _suppress = true; _sort.SelectedValue = _sortKey; _suppress = false;
  UpdateDirection(); await ApplyCriteriaAsync(state.Criteria);
 }
 public async Task SetKindAsync(string kind)
 {
  if (_initialized && _kind == kind) { if (_resultsInvalidated) { await StopAsync(); await SearchAsync(); } return; }
  await StopAsync();
  SaveColumnPreferences(); _buildingColumns = true; _grid.Columns.Clear(); _buildingColumns = false;
  int epoch = ++_epoch; var finished = _loadFinished = new(TaskCreationOptions.RunContinuationsAsynchronously);
  _loading = true; _initialized = false; _resultsInvalidated = false; _kind = kind; _catalog.Clear(); _columnCatalog.Clear(); _sample = null; _rows.Clear(); _criteria = new(); SyncQuick(); UpdateButtons();
  _title.Text = $"{KindLabel} 검색"; _subtitle.Text = _live ? "현재 게임 데이터 · 새 검색에서 조건을 설정하세요." : "저장된 스냅샷 · 새 검색에서 조건을 설정하세요.";
  _status.Text = "직무와 검색 가능한 능력치를 불러오는 중…"; _count.Text = "검색 준비";
  try
  {
   var facets = await _bridge.CallAsync("search_facets", new { kind }); if (epoch != _epoch) return;
   FillFacet(_roles, facets, "roles", "모든 직무 / 포지션"); FillFacet(_groups, facets, "groups", "모든 선수단");
   _count.Text = $"검색 가능 {facets.GetProperty("total").GetInt32():N0}명";
   try
   {
    var catalog = await _bridge.CallAsync("field_catalog", new { kind }); if (epoch != _epoch) return;
    _catalog.AddRange(Read<List<EditableField>>(catalog.GetProperty("fields")));
    _columnCatalog.AddRange(Read<List<EditableField>>(catalog.TryGetProperty("column_fields", out var columns) ? columns : catalog.GetProperty("fields"))); _sample = Read<EntityRecord>(catalog.GetProperty("sample"));
   }
   catch (BridgeException error) { App.Log(error); }
   if (epoch != _epoch) return;
   _availableColumns = SearchColumns.Available(_kind, _columnCatalog); _columnLayout = _columnSettings.Get(_kind);
   _initialized = true; BuildSortOptions(); BuildViewOptions(); BuildColumns(); RefreshSummary();
   _empty.Text = "새 검색을 눌러 검색 조건을 설정하세요.\n이름 또는 UID를 입력해 바로 검색할 수도 있습니다."; _emptyPanel.Visibility = Visibility.Visible;
   _status.Text = _columnSettings.LoadError == null ? "전체 데이터베이스에서 검색합니다. 조건을 설정하면 결과 표에 적용됩니다." : "저장된 열 설정을 읽지 못해 기본 보기로 열었습니다.";
  }
  finally { _suppress = false; if (epoch == _epoch) { _loading = false; UpdateButtons(); } finished.TrySetResult(); }
 }
 private static void FillFacet(List<SearchChoice> choices, JsonElement facets, string name, string all)
 {
  choices.Clear(); choices.Add(new("", all));
  if (facets.TryGetProperty(name, out var items)) foreach (var item in items.EnumerateArray()) { string value = item.GetProperty("value").GetString() ?? ""; if (value.Length != 0) choices.Add(new(value, $"{value} ({item.GetProperty("count").GetInt32():N0})")); }
 }
 private void BuildSortOptions()
 {
  _suppress = true;
  var choices = new List<SearchChoice> { new("ca", "현재 능력 (CA)"), new("pa", "잠재 능력 (PA)"), new("name", "이름"), new("age", "나이"), new("nationality", "국적"), new("club", "소속 구단"), new("group", "선수단 / 소속"), new("id", "고유 번호"), new("english_name", "원어 이름"), new("football_category", "축구 구분"), new("role", _kind == "Player" ? "포지션 (GK → ST)" : "현재 직무") };
  choices.AddRange(_columnCatalog.Where(item => item.Key.StartsWith("score_")).Select(item => new SearchChoice(item.Key, "직무 점수 · " + item.Label)));
  choices.AddRange(_columnCatalog.Where(item => !item.Key.StartsWith("score_") && !choices.Any(choice => choice.Key == item.Key)).OrderBy(item => item.Section).ThenBy(item => item.Label).Select(item => new SearchChoice(item.Key, item.Label)));
  _sort.ItemsSource = choices; _sortKey = _kind is "Player" or "Staff" ? "ca" : "name"; _sort.SelectedValue = _sortKey; _descending = _sortKey == "ca"; UpdateDirection(); _suppress = false;
 }
 private async Task RefreshColumnsAsync()
 {
  if (!_initialized || _loading) return;
  bool missing = _rows.Count > 0 && ViewFields().Any(key => !_loadedFields.Contains(key));
  BuildColumns(); ApplySort();
  if (missing) { await StopAsync(); await SearchAsync(); }
 }
 private SearchCriteriaWindow CriteriaWindow(SearchCriteria initial)
 {
  var dialog = new SearchCriteriaWindow(_kind, _catalog, _roles, _groups, _ownClub, initial.Clone());
  var owner = Window.GetWindow(this); if (owner != null) dialog.Owner = owner; return dialog;
 }
 private async Task EditCriteriaAsync(bool blank)
 {
  if (!_initialized || _loading) return;
  var dialog = CriteriaWindow(blank ? new SearchCriteria() : _criteria);
  int epoch = _epoch; _activeDialog = dialog;
  try { if (dialog.ShowDialog() == true && dialog.Result != null && epoch == _epoch) await ApplyCriteriaAsync(dialog.Result); }
  finally { if (_activeDialog == dialog) _activeDialog = null; }
 }
 private async Task ApplyQuickAsync()
 {
  var next = _criteria.Clone(); next.Name = _name.Text.Trim(); next.Scope = Selected(_scope, "all"); await ApplyCriteriaAsync(next);
 }
 private async Task ApplyFootballCategoryAsync(string? category)
 {
  if (!_initialized || _loading || _searching || !FootballSearchCategories.Supports(_kind)) return;
  var next = _criteria.Clone(); next.Name = _name.Text.Trim(); next.Scope = Selected(_scope, "all"); next.FootballCategory = category; await ApplyCriteriaAsync(next);
 }
 private async Task ApplyCriteriaAsync(SearchCriteria next)
 {
  ValidateCriteria(next); await StopAsync(); _criteria = next.Clone(); SyncQuick(); RefreshSummary(); await SearchAsync();
 }
 private void SyncQuick() { _suppress = true; _name.Text = _criteria.Name; _scope.SelectedValue = _criteria.Scope; UpdateFootballButtons(); _suppress = false; }
 private void UpdateFootballButtons()
 {
  _footballBar.Visibility = FootballSearchCategories.Supports(_kind) ? Visibility.Visible : Visibility.Collapsed;
  foreach (var (key, button) in _footballButtons)
  {
   bool selected = key == (_criteria.FootballCategory ?? ""); button.Background = Brush(selected ? "AccentBrush" : "RaisedBrush"); button.Foreground = selected ? Solid("#112A25") : Brush("InkBrush"); button.BorderBrush = Brush(selected ? "AccentBrush" : "BorderBrush"); button.FontWeight = selected ? FontWeights.SemiBold : FontWeights.Medium;
   System.Windows.Automation.AutomationProperties.SetHelpText(button, selected ? "선택됨" : "선택되지 않음");
  }
 }
 private async Task ResetAsync() { await ApplyCriteriaAsync(new()); }
 private void QuickChanged() { if (!_suppress && _initialized && !_loading && !_searching) _status.Text = "이름 / 검색 범위를 적용하려면 Enter 또는 검색을 누르세요."; }
 private static string RangeText(double? low, double? high) => low == high ? $"= {low:0.##}" : low.HasValue && high.HasValue ? $"{low:0.##}–{high:0.##}" : low.HasValue ? $"≥ {low:0.##}" : $"≤ {high:0.##}";
 private List<string> CriteriaLabels()
 {
  var values = new List<string>();
  if (_criteria.FootballCategory != null) values.Add("축구 구분: " + FootballSearchCategories.Label(_criteria.FootballCategory));
  if (_criteria.Name.Length > 0) values.Add("이름 / UID: " + _criteria.Name); if (_criteria.Club.Length > 0) values.Add("구단: " + _criteria.Club);
  if (_criteria.Role.Length > 0) values.Add("직무: " + _criteria.Role); if (_criteria.Group.Length > 0 && _kind == "Player") values.Add("선수단: " + _criteria.Group);
  if (_criteria.Employment != "all") values.Add(_criteria.Employment == "unemployed" ? "무소속" : "소속 있음");
  foreach (var pair in new[] { ("나이", _criteria.AgeMin, _criteria.AgeMax), ("CA", _criteria.CaMin, _criteria.CaMax), ("PA", _criteria.PaMin, _criteria.PaMax) }) if (pair.Item2.Length > 0 || pair.Item3.Length > 0) values.Add($"{pair.Item1} {pair.Item2}–{pair.Item3}");
  foreach (var condition in _criteria.Conditions) { var item = _catalog.FirstOrDefault(item => item.Key == condition.Key); if (item != null) values.Add($"{item.Label} {RangeText(condition.Min, condition.Max)}"); }
  return values;
 }
 private void RefreshSummary()
 {
  var labels = CriteriaLabels(); _editSearch.Content = $"검색 편집 ({labels.Count})"; _chips.Children.Clear();
  var scopeChip = Text(_criteria.Scope == "own" ? "우리 구단" : "전체 데이터베이스", 12, true); scopeChip.Margin = new Thickness(0, 4, 12, 4); _chips.Children.Add(scopeChip);
  if (labels.Count == 0) { var none = Text("조건 없음", 12); none.Margin = new Thickness(0, 4, 0, 4); _chips.Children.Add(none); }
  foreach (string label in labels) { var chip = Button(label, () => SafeAsync(() => EditCriteriaAsync(false))); chip.Padding = new Thickness(8, 3, 8, 3); chip.Margin = new Thickness(0, 0, 5, 4); chip.ToolTip = "클릭하여 검색 조건 편집"; _chips.Children.Add(chip); }
 }
 private void UpdateButtons()
 {
  bool ready = _initialized && !_loading && !_searching;
  foreach (var button in new[] { _search, _newSearch, _editSearch, _reset, _save, _load }) button.IsEnabled = ready;
  _cancel.IsEnabled = _searching; _scope.IsEnabled = ready && (_kind is "Player" or "Staff" or "Person"); _name.IsEnabled = ready; _sort.IsEnabled = ready; _view.IsEnabled = ready; _columnsButton.IsEnabled = ready; _direction.IsEnabled = ready; _chips.IsEnabled = ready;
  _footballBar.IsEnabled = ready;
 }
 private void UpdateDirection() => _direction.Content = _descending ? "내림차순 ↓" : "오름차순 ↑";
 private static double? Number(string text, string label, double minimum, double maximum, bool whole = false)
 {
  if (string.IsNullOrWhiteSpace(text)) return null;
  if (!double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var value) || !double.IsFinite(value) || value < minimum || value > maximum || (whole && value != Math.Truncate(value))) throw new InvalidOperationException($"{label}: {minimum:G}–{maximum:G} 범위의 {(whole ? "정수" : "숫자")}를 입력하세요.");
  return value;
 }
 private void ValidateCriteria(SearchCriteria value)
 {
  if (value == null || value.Conditions == null || value.Name == null || value.Club == null || value.Role == null || value.Group == null) throw new InvalidOperationException("저장된 검색 조건이 올바르지 않습니다.");
  if (value.Name.Length > 200 || value.Club.Length > 200) throw new InvalidOperationException("이름과 구단 검색어는 각각 200자 이내로 입력하세요.");
  if (value.Scope is not ("all" or "own") || value.Scope == "own" && _ownClub == null) throw new InvalidOperationException("이 검색 범위를 현재 자료에서 사용할 수 없습니다.");
  if (value.Employment is not ("all" or "unemployed" or "employed")) throw new InvalidOperationException("고용 상태가 올바르지 않습니다.");
  if (!FootballSearchCategories.IsValid(value.FootballCategory) || value.FootballCategory != null && !FootballSearchCategories.Supports(_kind)) throw new InvalidOperationException("현재 검색에서 사용할 수 없는 축구 구분입니다.");
  if (value.Role.Length > 0 && !_roles.Any(item => item.Key == value.Role)) throw new InvalidOperationException("현재 자료에 없는 직무 / 포지션입니다.");
  if (value.Group.Length > 0 && (_kind != "Player" || !_groups.Any(item => item.Key == value.Group))) throw new InvalidOperationException("현재 자료에 없는 선수단입니다.");
  foreach (var pair in new[] { ("나이", value.AgeMin, value.AgeMax, 129), ("CA", value.CaMin, value.CaMax, 200), ("PA", value.PaMin, value.PaMax, 200) }) { var low = Number(pair.Item2, pair.Item1 + " 최소", 0, pair.Item4, true); var high = Number(pair.Item3, pair.Item1 + " 최대", 0, pair.Item4, true); if (low > high) throw new InvalidOperationException(pair.Item1 + ": 최소가 최대보다 큽니다."); }
  ReadConditions(value);
 }
 private List<RangeFilter> ReadConditions(SearchCriteria criteria)
 {
  if (criteria.Conditions.Count > 64) throw new InvalidOperationException("능력치 조건은 최대 64개입니다.");
  var result = new List<RangeFilter>();
  foreach (var condition in criteria.Conditions)
  {
   var item = _catalog.SingleOrDefault(item => item.Key == condition.Key) ?? throw new InvalidOperationException("현재 자료에서 사용할 수 없는 능력치 조건입니다.");
   double? low = condition.Min, high = condition.Max;
   if (low == null && high == null || low > high || low.HasValue && (!double.IsFinite(low.Value) || low < item.Min || low > item.Max) || high.HasValue && (!double.IsFinite(high.Value) || high < item.Min || high > item.Max)) throw new InvalidOperationException(item.Label + ": 능력치 범위를 확인하세요.");
   if (result.Any(existing => existing.key == item.Key)) throw new InvalidOperationException(item.Label + " 조건이 중복되었습니다.");
   result.Add(new(item.Key, low, high));
  }
  return result;
 }
 private sealed record RangeFilter(string key, double? min, double? max);
 private sealed class SavedSearch { public SavedSearch() { } public string Name { get; set; } = ""; public string Kind { get; set; } = ""; public SearchCriteria Criteria { get; set; } = new(); }
 private static string PresetPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "search-presets.json");
 private static List<SavedSearch> ReadPresets() => File.Exists(PresetPath) ? JsonSerializer.Deserialize<List<SavedSearch>>(File.ReadAllText(PresetPath), JsonDefaults.Options) ?? [] : [];
 private async Task PresetDialogAsync(bool save)
 {
  var presets = ReadPresets(); var matching = presets.Where(item => item.Kind == _kind).OrderBy(item => item.Name).ToList();
  var window = new Window { Title = save ? "검색 조건 저장" : "검색 조건 불러오기", Width = 480, Height = 410, MinWidth = 420, MinHeight = 330, WindowStartupLocation = WindowStartupLocation.CenterOwner, ResizeMode = ResizeMode.CanResize, Background = Brush("BaseBrush"), Foreground = Brush("InkBrush"), Owner = Window.GetWindow(this) };
  Typography.Initialize(window); var panel = new DockPanel { Margin = new Thickness(20) }; window.Content = panel;
  var intro = Text(save ? "현재 적용된 검색 조건을 이 컴퓨터에 저장합니다." : "같은 종류의 검색 조건을 불러옵니다.", 13); intro.Margin = new Thickness(0, 0, 0, 12); DockPanel.SetDock(intro, Dock.Top); panel.Children.Add(intro);
  var name = Input(); name.ToolTip = "검색 이름"; var label = Labeled("검색 이름", name); DockPanel.SetDock(label, Dock.Top); panel.Children.Add(label);
  var footer = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 12, 0, 0) }; SearchCriteria? selected = null;
  var list = new ListBox { ItemsSource = matching, DisplayMemberPath = "Name" }; list.SelectionChanged += (_, _) => { if (list.SelectedItem is SavedSearch item) name.Text = item.Name; };
  var accept = Button(save ? "저장" : "불러오기", () => {
   try {
    if (save) { string title = name.Text.Trim(); if (title.Length == 0 || title.Length > 80) throw new InvalidOperationException("검색 이름을 1–80자로 입력하세요."); ValidateCriteria(_criteria); var existing = presets.FirstOrDefault(item => item.Kind == _kind && item.Name.Equals(title, StringComparison.OrdinalIgnoreCase)); if (existing != null && AppDialog.Show(window, "같은 이름의 저장된 검색을 바꾸시겠습니까?", "검색 조건 덮어쓰기", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return Task.CompletedTask; if (existing != null) presets.Remove(existing); presets.Add(new() { Name = title, Kind = _kind, Criteria = _criteria.Clone() }); Directory.CreateDirectory(Path.GetDirectoryName(PresetPath)!); string temporary = PresetPath + ".tmp"; File.WriteAllText(temporary, JsonSerializer.Serialize(presets, new JsonSerializerOptions(JsonDefaults.Options) { WriteIndented = true })); File.Move(temporary, PresetPath, true); _status.Text = $"검색 조건 ‘{title}’을 이 컴퓨터에 저장했습니다."; }
    else { if (list.SelectedItem is not SavedSearch item) throw new InvalidOperationException("불러올 검색을 선택하세요."); ValidateCriteria(item.Criteria); selected = item.Criteria.Clone(); }
    window.DialogResult = true;
   } catch (Exception error) { AppDialog.Show(window, Friendly(error.Message), "검색 조건", MessageBoxButton.OK, MessageBoxImage.Warning); }
   return Task.CompletedTask;
  }); var cancel = Button("취소", () => { window.DialogResult = false; return Task.CompletedTask; }); cancel.IsCancel = true; cancel.Margin = new Thickness(8, 0, 0, 0); footer.Children.Add(accept); footer.Children.Add(cancel); DockPanel.SetDock(footer, Dock.Bottom); panel.Children.Add(footer); panel.Children.Add(list);
  int epoch = _epoch; _activeDialog = window;
  try { if (window.ShowDialog() == true && selected != null && epoch == _epoch) await ApplyCriteriaAsync(selected); }
  finally { if (_activeDialog == window) _activeDialog = null; }
 }
 private bool NumericSort => !IsMetadata(_sortKey) && _columnCatalog.Any(item => item.Key == _sortKey);
 private void Column(string title, string binding, string sort, double width, bool numeric = false)
 {
  var style = new Style(typeof(TextBlock), TryFindResource(typeof(TextBlock)) as Style); style.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis)); if (numeric) style.Setters.Add(new Setter(TextBlock.TextAlignmentProperty, TextAlignment.Right)); style.Setters.Add(new Setter(ToolTipService.ToolTipProperty, new Binding(binding)));
  _grid.Columns.Add(new DataGridTextColumn { Header = title, Binding = new Binding(binding) { TargetNullValue = "—", FallbackValue = "—", StringFormat = numeric ? "0.##" : null }, ElementStyle = style, SortMemberPath = sort, Width = width, MinWidth = numeric ? 48 : 90 });
 }
 private void ApplySort()
 {
  _resultView.CustomSort = new RowComparer(_sortKey, _descending);
  foreach (var column in _grid.Columns) column.SortDirection = column.SortMemberPath == _sortKey ? _descending ? ListSortDirection.Descending : ListSortDirection.Ascending : null;
 }
 private void GridSorting(object sender, DataGridSortingEventArgs args)
 {
  args.Handled = true; string key = args.Column.SortMemberPath;
  _descending = _sortKey == key ? !_descending : key is not ("name" or "english_name" or "nationality" or "club" or "role" or "age" or "group" or "id" or "football_category"); _sortKey = key; _suppress = true; _sort.SelectedValue = key; _suppress = false; UpdateDirection(); ApplySort();
 }
 private async Task SafeAsync(Func<Task> action)
 {
  try { await action(); } catch (OperationCanceledException) { _status.Text = "검색이 중지되었습니다."; } catch (Exception error) { App.Log(error); _status.Text = Friendly(error.Message); _errorText.Text = Friendly(error.Message); _errorPanel.Visibility = Visibility.Visible; }
 }
 private string Friendly(string message) { foreach (var field in _catalog.OrderByDescending(field => field.Key.Length)) message = message.Replace(field.Key, field.Label, StringComparison.Ordinal); return message; }
 private async Task SearchAsync()
 {
  if (_searching || _loading || !_initialized) return;
  ValidateCriteria(_criteria); var fields = ReadConditions(_criteria);
  uint? Whole(string text, string label, uint max) => Number(text, label, 0, max, true) is double value ? (uint)value : null;
  var filter = new { football_category = _criteria.FootballCategory, age_min = Whole(_criteria.AgeMin, "최소 나이", 129), age_max = Whole(_criteria.AgeMax, "최대 나이", 129), ca_min = Whole(_criteria.CaMin, "최소 CA", 200), ca_max = Whole(_criteria.CaMax, "최대 CA", 200), pa_min = Whole(_criteria.PaMin, "최소 PA", 200), pa_max = Whole(_criteria.PaMax, "최대 PA", 200), club_query = _criteria.Club.Trim(), role = _criteria.Role, group = _kind == "Player" && _criteria.Group.Length > 0 ? _criteria.Group : null, unemployed = _criteria.Employment == "unemployed", employed = _criteria.Employment == "employed", fields };
  var selectedFields = ViewFields().Where(key => !NumericSort || key != _sortKey).ToArray();
  var args = new { kind = _kind, query = _criteria.Name.Trim(), club_id = _criteria.Scope == "own" ? _ownClub?.Id : null, filter, sort = _sortKey is "name" or "age" or "ca" or "pa" or "club" or "role" or "nationality" ? _sortKey : "name", sort_field = NumericSort ? _sortKey : null, fields = selectedFields };
  int epoch = _epoch; _resultsInvalidated = false; _cancellation?.Dispose(); _cancellation = new(); var cancellation = _cancellation.Token; var finished = _searchFinished = new(TaskCreationOptions.RunContinuationsAsynchronously);
  _searching = true; _lastCancelled = false; _complete = false; _scanned = _total = _matched = _failed = 0; _rows.Clear(); _loadedFields.Clear(); _loadedFields.UnionWith(fields.Select(item => item.key).Concat(selectedFields).Concat(NumericSort ? new[] { _sortKey } : [])); _errors.Clear(); _errorPanel.Visibility = Visibility.Collapsed; _empty.Text = "검색 중…"; _emptyPanel.Visibility = Visibility.Visible; _progress.Visibility = Visibility.Visible; _progress.IsIndeterminate = true; _count.Text = "검색 중…"; _status.Text = "검색 대상을 확인하는 중…"; BuildColumns(); _resultView.CustomSort = null; UpdateButtons();
  ulong? cursor = null;
  try
  {
   // Always retain the server cursor before observing cancellation so cleanup can identify it.
   var start = await _bridge.CallAsync("query_start", args); cursor = start.GetProperty("query_id").GetUInt64();
   cancellation.ThrowIfCancellationRequested(); if (epoch != _epoch) throw new OperationCanceledException(cancellation);
   _queryId = cursor; _total = start.GetProperty("total").GetInt32(); _qaAfterStart?.Invoke();
   while (true)
   {
    cancellation.ThrowIfCancellationRequested(); var step = await _bridge.CallAsync("query_step", new { query_id = cursor }, cancellation);
    if (epoch != _epoch) throw new OperationCanceledException(cancellation);
    if (step.GetProperty("query_id").GetUInt64() != cursor) throw new InvalidOperationException("다른 검색 결과가 도착했습니다. 다시 검색하세요.");
    foreach (var item in step.GetProperty("items").EnumerateArray())
    {
     var values = new Dictionary<string, double>(); if (item.TryGetProperty("values", out var numeric)) foreach (var value in numeric.EnumerateObject()) if (value.Value.ValueKind == JsonValueKind.Number && value.Value.TryGetDouble(out double parsed)) values[value.Name] = parsed;
     _rows.Add(new SearchRow { Entity = Read<EntityRecord>(item), Values = values });
    }
    _scanned = step.GetProperty("scanned").GetInt32(); _total = step.GetProperty("total").GetInt32(); _matched = step.GetProperty("matched").GetInt32(); _failed = step.GetProperty("failed").GetInt32();
    if (step.TryGetProperty("errors", out var errors)) foreach (var error in errors.EnumerateArray()) if (_errors.Count < 30) _errors.Add(Friendly(error.GetString() ?? "상세 조회 실패"));
    _progress.IsIndeterminate = false; _progress.Maximum = Math.Max(1, _total); _progress.Value = _scanned; _count.Text = $"일치 {_matched:N0}명"; _status.Text = $"확인 {_scanned:N0} / {_total:N0} · 상세 확인 실패 {_failed:N0} · 완료 후 {Label(_sort)} 정렬"; _emptyPanel.Visibility = _rows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    if (step.GetProperty("complete").GetBoolean()) { _complete = true; break; }
    await Dispatcher.Yield(DispatcherPriority.Background);
   }
   if (_rows.Count != _matched) throw new InvalidOperationException("검색 결과 수가 맞지 않습니다. 다시 검색하세요.");
   _count.Text = $"검색 결과 {_matched:N0}명"; _status.Text = _failed == 0 ? $"총 {_total:N0}명 확인 · {_matched:N0}명 일치 · 프로필을 열려면 더블 클릭하세요." : $"{_matched:N0}명 일치 · {_failed:N0}명은 상세 확인 실패로 판정하지 못했습니다.";
   _empty.Text = _failed == 0 ? "조건에 맞는 대상이 없습니다.\n범위를 넓히거나 조건을 초기화해 보세요." : "확인한 대상 중 일치 결과가 없습니다.\n상세 조회에 실패한 대상은 판정하지 못했습니다.";
  }
  catch (OperationCanceledException) { _lastCancelled = true; if (epoch == _epoch) { _count.Text = $"중지됨 · {_matched:N0}명 확인"; _status.Text = $"검색 중지 · {_scanned:N0} / {_total:N0}명까지 확인한 결과입니다."; _empty.Text = "검색을 중지했습니다.\n조건을 바꿔 다시 검색할 수 있습니다."; } }
  catch (Exception) { if (epoch == _epoch) { _complete = false; _count.Text = $"검색 실패 · {_rows.Count:N0}명까지 확인"; _empty.Text = "검색을 완료하지 못했습니다.\n오류 내용을 확인하고 다시 검색하세요."; throw; } }
  finally
  {
   try { if (cursor.HasValue) await _bridge.CallAsync("query_cancel", new { query_id = cursor.Value }); }
   finally { _queryId = null; _searching = false; _progress.Visibility = Visibility.Collapsed; _emptyPanel.Visibility = _rows.Count == 0 ? Visibility.Visible : Visibility.Collapsed; _errorPanel.Visibility = _failed > 0 ? Visibility.Visible : Visibility.Collapsed; _errorText.Text = $"상세 확인 실패 {_failed:N0}명 · 이 대상은 일치 여부를 판단하지 않았습니다.\n" + string.Join("\n", _errors); ApplySort(); UpdateButtons(); finished.TrySetResult(); }
  }
 }
 public void InvalidateResults()
 {
  _activeDialog?.Close(); _activeDialog = null;
  ++_epoch; _resultsInvalidated = true; _cancellation?.Cancel(); _queryId = null;
  _rows.Clear(); _loadedFields.Clear(); _errors.Clear(); _scanned = _total = _matched = _failed = 0; _complete = false;
  _grid.SelectedItem = null; _openButton.IsEnabled = false; _progress.Visibility = _errorPanel.Visibility = Visibility.Collapsed;
  _emptyPanel.Visibility = Visibility.Visible; _empty.Text = "자료가 변경되었습니다. 검색 조건을 유지하고 다시 검색합니다.";
  _count.Text = "다시 검색 필요"; _status.Text = "이전 검색 결과를 비웠습니다. 검색 화면을 열거나 검색을 눌러 다시 조회하세요."; UpdateButtons();
 }
 public async Task StopAsync()
 {
  _cancellation?.Cancel(); var search = _searchFinished; if (search != null) await search.Task;
  var loading = _loadFinished; if (loading != null && _loading) { ++_epoch; await loading.Task; _loading = false; UpdateButtons(); }
 }
 private async Task OpenSelectedAsync() { if (_grid.SelectedItem is not SearchRow row) return; _navigationRequests++; await _open(row.Entity); }
 public void FocusSearch() { _name.Focus(); _name.SelectAll(); }

 private async Task<(SearchCriteria? Criteria, object? Report)> QaDialogAsync(SearchCriteria initial, bool apply, string outputDir, bool inspect, string? footballChoice = null)
 {
  var dialog = CriteriaWindow(initial); var done = new TaskCompletionSource<object?>(TaskCreationOptions.RunContinuationsAsynchronously);
  dialog.Loaded += async (_, _) => {
   try {
    if (footballChoice != null) (dialog.FindName("CriteriaFootballCategory") as ComboBox ?? throw new InvalidOperationException("축구 구분 선택기를 찾지 못했습니다.")).SelectedValue = footballChoice;
    object? report = inspect ? await dialog.QaAsync(outputDir) : null;
    var button = dialog.FindName(apply ? "CriteriaApplyButton" : "CriteriaCancelButton") as Button ?? throw new InvalidOperationException("검색 조건 창의 적용 / 취소 버튼을 찾지 못했습니다.");
    button.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent));
    if (dialog.IsVisible) throw new InvalidOperationException("검색 조건 창이 적용 / 취소 후 닫히지 않았습니다.");
    done.TrySetResult(report);
   } catch (Exception error) { done.TrySetException(error); dialog.Close(); }
  };
  bool accepted = dialog.ShowDialog() == true; var report = await done.Task;
  if (accepted != apply || (apply && dialog.Result == null) || (!apply && dialog.Result != null)) throw new InvalidOperationException("검색 조건 창의 적용 / 취소 상태가 올바르지 않습니다.");
  return (dialog.Result, report);
 }

 /// <summary>Verifies the men's/women's search controls against the dedicated, mixed-category snapshot fixture.</summary>
 public async Task<object> QaFootballCategoryAsync(string outputDir)
 {
  if (_live) throw new InvalidOperationException("축구 구분 검색 QA는 스냅샷에서만 실행합니다.");
  if (_ownClub?.Id != 10) throw new InvalidOperationException("축구 구분 검색 QA 전용 스냅샷을 불러오세요.");
  Directory.CreateDirectory(outputDir);
  var fixtures = new Dictionary<string, (uint[] Men, uint[] Women, uint[] Unknown)>
  {
   ["Player"] = ([1001, 1004], [1002, 1005], [1003, 1006]),
   ["Staff"] = ([201, 204], [202, 205], [203, 206]),
   ["Club"] = ([10], [20], [30])
  };
  var reports = new List<object>();
  async Task ClickCategory(string? category)
  {
   _footballButtons[category ?? ""].RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent));
   var finished = _searchFinished ?? throw new InvalidOperationException("축구 구분 버튼이 검색을 시작하지 않았습니다."); await finished.Task;
   if (!_complete || _failed != 0 || _criteria.FootballCategory != category) throw new InvalidOperationException("축구 구분 버튼 검색 실패.");
  }
  void AssertIds(IEnumerable<uint> expected, string context)
  {
   uint[] ids = _rows.Select(row => row.Entity.Id).Order().ToArray();
   if (!ids.SequenceEqual(expected.Order()) || _rows.Count != _matched) throw new InvalidOperationException("축구 구분 검색 결과 불일치 · " + context);
  }
  foreach (var (kind, fixture) in fixtures)
  {
   await SetKindAsync(kind); await ResetAsync();
   if (_criteria.FootballCategory != null || _footballBar.Visibility != Visibility.Visible) throw new InvalidOperationException("축구 구분 기본 선택은 전체여야 합니다.");
   foreach (var choice in FootballSearchCategories.Choices)
   {
    string? category = choice.Key.Length == 0 ? null : choice.Key; await ClickCategory(category);
    uint[] expected = category switch { "men" => fixture.Men, "women" => fixture.Women, _ => fixture.Men.Concat(fixture.Women).Concat(fixture.Unknown).ToArray() };
    AssertIds(expected, kind + " / " + choice.Label);
    if (category != null && _rows.Any(row => row.Entity.FootballCategory != category || row.FootballCategoryLabel != choice.Label)) throw new InvalidOperationException("다른 축구 구분 또는 미확인 대상이 선택된 결과에 섞였습니다.");
    if (category == null && _rows.Count(row => row.Entity.FootballCategory == "unknown") != fixture.Unknown.Length) throw new InvalidOperationException("전체 검색이 미확인 대상을 보존하지 못했습니다.");
    if (!_grid.Columns.OfType<DataGridTextColumn>().Any(column => column.Binding is Binding binding && binding.Path.Path == nameof(SearchRow.FootballCategoryLabel))) throw new InvalidOperationException("축구 구분 결과 열이 없습니다.");
    if (category != null && !CriteriaLabels().Contains("축구 구분: " + choice.Label)) throw new InvalidOperationException("축구 구분 조건 표시가 누락되었습니다.");
    await CaptureAsync(Path.Combine(outputDir, $"football-search-{kind.ToLowerInvariant()}-{category ?? "all"}.png"));
    reports.Add(new { kind, category = category ?? "all", ids = expected, count = _rows.Count });
   }
   var dialogStart = new SearchCriteria { FootballCategory = "men" };
   var applied = await QaDialogAsync(dialogStart, true, outputDir, false, "women");
   if (dialogStart.FootballCategory != "men" || applied.Criteria?.FootballCategory != "women") throw new InvalidOperationException("조건 창의 축구 구분 변경이 원본을 변경했거나 적용되지 않았습니다.");
   await ApplyCriteriaAsync(applied.Criteria!); AssertIds(fixture.Women, kind + " dialog");
   string beforeCancel = JsonSerializer.Serialize(_criteria); await QaDialogAsync(new SearchCriteria { FootballCategory = "men" }, false, outputDir, false, "");
   if (beforeCancel != JsonSerializer.Serialize(_criteria)) throw new InvalidOperationException("축구 구분 편집 취소가 검색 조건을 변경했습니다.");
   await ResetAsync(); AssertIds(fixture.Men.Concat(fixture.Women).Concat(fixture.Unknown), kind + " reset");
   if (_criteria.FootballCategory != null || System.Windows.Automation.AutomationProperties.GetHelpText(_footballButtons[""]) != "선택됨") throw new InvalidOperationException("초기화가 축구 구분 전체 선택을 복원하지 못했습니다.");
  }
  await SetKindAsync("Staff"); await ApplyCriteriaAsync(new() { Name = "202", Employment = "employed", FootballCategory = "women" }); AssertIds([202], "staff club category");
  await ClickCategory("men"); AssertIds([], "combined criteria men");
  if (_criteria.Name != "202" || _criteria.Employment != "employed") throw new InvalidOperationException("축구 구분 변경이 다른 검색 조건을 지웠습니다.");
  await ClickCategory("women"); AssertIds([202], "combined criteria women");
  var legacy = JsonSerializer.Deserialize<SavedSearch>("{\"Name\":\"legacy\",\"Kind\":\"Staff\",\"Criteria\":{\"Name\":\"202\"}}", JsonDefaults.Options)!;
  ValidateCriteria(legacy.Criteria); if (legacy.Criteria.FootballCategory != null) throw new InvalidOperationException("기존 저장 검색의 축구 구분 기본값은 전체여야 합니다.");
  var saved = new SavedSearch { Name = "축구 구분 QA", Kind = "Staff", Criteria = _criteria.Clone() };
  string presetPath = Path.Combine(outputDir, "football-search-presets-roundtrip.json"); await File.WriteAllTextAsync(presetPath, JsonSerializer.Serialize(new[] { legacy, saved }, new JsonSerializerOptions(JsonDefaults.Options) { WriteIndented = true }));
  var restored = JsonSerializer.Deserialize<List<SavedSearch>>(await File.ReadAllTextAsync(presetPath), JsonDefaults.Options)!.Last(); ValidateCriteria(restored.Criteria);
  if (JsonSerializer.Serialize(restored.Criteria) != JsonSerializer.Serialize(_criteria)) throw new InvalidOperationException("축구 구분 저장 조건 복원 실패.");
  var legacyEntity = JsonSerializer.Deserialize<EntityRecord>("{\"id\":1,\"kind\":\"Staff\"}", JsonDefaults.Options)!;
  if (legacyEntity.FootballCategory != "unknown" || legacyEntity.FootballCategoryLabel != "미확인") throw new InvalidOperationException("기존 자료의 미확인 구분 기본값이 올바르지 않습니다.");
  var invalid = _criteria.Clone(); invalid.FootballCategory = "female"; bool rejected = false; try { ValidateCriteria(invalid); } catch (InvalidOperationException) { rejected = true; }
  if (!rejected) throw new InvalidOperationException("지원하지 않는 축구 구분을 허용했습니다.");
  await ResetAsync(); await ClickCategory("women"); await CaptureAsync(Path.Combine(outputDir, "football-search-ready.png"));
  return new { success = true, live = false, gameWrites = 0, defaultCategory = "all", reports, unknownExcludedWhenSelected = true, unknownIncludedInAll = true, staffUsesClubCategory = true, actualQuickButtons = true, actualDialogSelection = true, dialogCancelPreservesCriteria = true, categoryColumn = true, chips = true, otherCriteriaPreserved = true, reset = true, legacyPresets = true, localPresetRoundTrip = true, legacyEntityUnknown = true, invalidCategoryRejected = true };
 }

 public async Task<object> QaPositionOrderAsync(string outputDir)
 {
  if (_live) throw new InvalidOperationException("포지션 정렬 QA는 스냅샷에서만 실행합니다.");
  string snapshot = App.Arg("--snapshot") ?? throw new InvalidOperationException("포지션 정렬 QA 스냅샷이 필요합니다.");
  using var document = JsonDocument.Parse(await File.ReadAllTextAsync(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(snapshot))!, "expected.json")));
  var expected = document.RootElement; var checks = new List<object>(); Directory.CreateDirectory(outputDir);
  void AssertOrder(JsonElement ids, string label)
  {
   uint[] actual = _resultView.Cast<SearchRow>().Select(row => row.Entity.Id).ToArray();
   if (!actual.SequenceEqual(ids.EnumerateArray().Select(id => id.GetUInt32())) || _sortKey != "role") throw new InvalidOperationException("포지션 정렬 순서 불일치 · " + label);
   checks.Add(new { label, ids = actual });
  }
  IEnumerable<DependencyObject> Descendants(DependencyObject parent)
  {
   for (int index = 0; index < VisualTreeHelper.GetChildrenCount(parent); index++) { var child = VisualTreeHelper.GetChild(parent, index); yield return child; foreach (var nested in Descendants(child)) yield return nested; }
  }
  async Task InvokeRoleHeader()
  {
   await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
   var header = Descendants(_grid).OfType<DataGridColumnHeader>().First(item => item.Column?.SortMemberPath == "role");
   // Invoke the real header click path; this WPF header does not expose the automation Invoke pattern.
   var click = typeof(DataGridColumnHeader).GetMethod("OnClick", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic) ?? throw new InvalidOperationException("포지션 열 제목을 실행할 수 없습니다.");
   click.Invoke(header, null); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  }
  await SetKindAsync("Player"); await ResetAsync(); _sort.SelectedValue = "role"; await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  if (_descending) throw new InvalidOperationException("포지션 선택기의 첫 정렬은 GK → ST여야 합니다.");
  AssertOrder(expected.GetProperty("player_ascending_ids"), "dropdown_ascending"); await CaptureAsync(Path.Combine(outputDir, "position-search-ascending.png"));
  _direction.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent)); AssertOrder(expected.GetProperty("player_descending_ids"), "direction_descending"); await CaptureAsync(Path.Combine(outputDir, "position-search-descending.png"));
  await InvokeRoleHeader(); AssertOrder(expected.GetProperty("player_ascending_ids"), "header_ascending");
  foreach (string category in new[] { "men", "women" })
  {
   _footballButtons[category].RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent)); if (_searchFinished != null) await _searchFinished.Task;
   if (!_complete || _failed != 0 || _criteria.FootballCategory != category) throw new InvalidOperationException("포지션 정렬의 축구 구분 조건 보존 실패.");
   var categoryExpected = expected.GetProperty("by_football_category").GetProperty(category);
   AssertOrder(categoryExpected.GetProperty("ascending"), category + "_ascending");
   _direction.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent)); AssertOrder(categoryExpected.GetProperty("descending"), category + "_descending");
   if (_criteria.FootballCategory != category || _rows.Any(row => row.Entity.FootballCategory != category)) throw new InvalidOperationException("정렬 방향 변경이 축구 구분 조건을 변경했습니다.");
   await InvokeRoleHeader(); AssertOrder(categoryExpected.GetProperty("ascending"), category + "_header_ascending");
  }
  await SetKindAsync("Staff"); await ResetAsync(); _sort.SelectedValue = "role"; await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  uint[] staffAscending = _rows.OrderBy(row => row.Role, StringComparer.CurrentCultureIgnoreCase).ThenBy(row => row.Entity.Id).Select(row => row.Entity.Id).ToArray();
  if (!_resultView.Cast<SearchRow>().Select(row => row.Entity.Id).SequenceEqual(staffAscending)) throw new InvalidOperationException("스태프 직무의 가나다순 정렬이 변경되었습니다.");
  _direction.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent));
  if (!_resultView.Cast<SearchRow>().Select(row => row.Entity.Id).SequenceEqual(_rows.OrderByDescending(row => row.Role, StringComparer.CurrentCultureIgnoreCase).ThenBy(row => row.Entity.Id).Select(row => row.Entity.Id))) throw new InvalidOperationException("스태프 직무 역순 정렬이 변경되었습니다.");
  await SetKindAsync("Player"); await ApplyCriteriaAsync(new() { FootballCategory = "men" }); _sort.SelectedValue = "role"; await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  AssertOrder(expected.GetProperty("by_football_category").GetProperty("men").GetProperty("ascending"), "ready_men_ascending"); await CaptureAsync(Path.Combine(outputDir, "position-search-ready.png"));
  return new { success = true, live = false, gameWrites = 0, actualDropdown = true, actualHeader = true, actualDirectionButton = true, categoryPreserved = true, compoundMinimumRank = true, unknownLastBothDirections = true, staffLexicalBothDirections = true, checks };
 }

 /// <summary>Runs the real criteria dialog and displayed search using only snapshot queries.</summary>
 public async Task<object> QaAsync(string outputDir)
 {
  if (_live) throw new InvalidOperationException("검색 화면 QA는 스냅샷에서만 실행합니다.");
  await SetKindAsync("Staff"); if (_sample == null) throw new InvalidOperationException("스태프 상세 샘플이 필요합니다.");
  Directory.CreateDirectory(outputDir);
  var sample = _sample; var expected = _catalog.ToDictionary(item => item.Key, item => item.Value);
  var attribute = _catalog.FirstOrDefault(item => item.Key == "staff_youth") ?? _catalog.First(item => item.Key.StartsWith("staff_"));
  var second = _catalog.FirstOrDefault(item => item.Key == "staff_determination") ?? _catalog.First(item => item.Key.StartsWith("staff_") && item.Key != attribute.Key);
  string score = _catalog.Any(item => item.Key == "score_head_youth") ? "score_head_youth" : attribute.Key;
  var single = new SearchCriteria { Name = sample.Id.ToString(CultureInfo.InvariantCulture), Conditions = [new(attribute.Key, Math.Max(attribute.Min, Math.Floor(attribute.Value) - 1), attribute.Max), new(second.Key, second.Min, second.Max)] };
  _suppress = true; _sort.SelectedValue = score; _sortKey = score; _view.SelectedValue = "selected"; _suppress = false; _descending = true;
  var applied = await QaDialogAsync(single, true, outputDir, true);
  await ApplyCriteriaAsync(applied.Criteria!);
  if (!_complete || _failed != 0 || _rows.Count != 1 || _rows[0].Entity.Id != sample.Id || !_rows[0].Values.TryGetValue(attribute.Key, out double actual) || actual != expected[attribute.Key] || !_rows[0].Values.TryGetValue(second.Key, out double actualSecond) || actualSecond != expected[second.Key] || !_rows[0].Values.TryGetValue(score, out double actualScore) || actualScore != expected[score]) throw new InvalidOperationException("스태프 능력치 / 점수 검색 결과가 상세 샘플과 일치하지 않습니다.");
  var verifiedValues = new { attribute = attribute.Label, value = actual, secondAttribute = second.Label, secondValue = actualSecond, score = _catalog.Single(item => item.Key == score).Label, scoreValue = actualScore };
  await CaptureAsync(Path.Combine(outputDir, "search-staff-filter.png"));
  string baseline = JsonSerializer.Serialize(_criteria); uint[] baselineRows = _rows.Select(row => row.Entity.Id).ToArray();
  await QaDialogAsync(new SearchCriteria(), false, outputDir, false);
  if (baseline != JsonSerializer.Serialize(_criteria) || !baselineRows.SequenceEqual(_rows.Select(row => row.Entity.Id))) throw new InvalidOperationException("새 검색 취소가 기존 검색 조건 또는 결과를 변경했습니다.");
  var changedDraft = _criteria.Clone(); changedDraft.Name = "__cancel_must_not_apply__";
  await QaDialogAsync(changedDraft, false, outputDir, false);
  if (baseline != JsonSerializer.Serialize(_criteria)) throw new InvalidOperationException("검색 편집 취소가 적용된 조건을 변경했습니다.");

  if (_ownClub == null) throw new InvalidOperationException("여러 스태프의 정렬 검증에는 우리 구단 스냅샷이 필요합니다.");
  double threshold = Math.Max(attribute.Min, Math.Min(10, attribute.Value));
  var multiple = new SearchCriteria { Scope = "own", Employment = "employed", Conditions = [new(attribute.Key, threshold, attribute.Max), new(second.Key, second.Min, second.Max)] };
  var multipleDialog = await QaDialogAsync(multiple, true, outputDir, false); await ApplyCriteriaAsync(multipleDialog.Criteria!);
  if (!_complete || _failed != 0 || _rows.Count < 2 || _rows.Any(row => row.Entity.ClubId != _ownClub.Id || !row.Values.TryGetValue(attribute.Key, out var value) || value < threshold || value > attribute.Max || !row.Values.ContainsKey(second.Key) || !row.Values.ContainsKey(score))) throw new InvalidOperationException("우리 구단 복수 스태프 검색 결과 검증 실패: 전체 상세를 포함한 스냅샷이 필요합니다.");
  var descendingValues = _resultView.Cast<SearchRow>().Select(row => row.Values[score]).ToArray();
  if (descendingValues.Zip(descendingValues.Skip(1)).Any(pair => pair.First < pair.Second)) throw new InvalidOperationException("직무 점수 내림차순 정렬 검증 실패");
  _descending = false; ApplySort(); var ascendingValues = _resultView.Cast<SearchRow>().Select(row => row.Values[score]).ToArray();
  if (ascendingValues.Zip(ascendingValues.Skip(1)).Any(pair => pair.First > pair.Second)) throw new InvalidOperationException("직무 점수 오름차순 정렬 검증 실패");
  _descending = true; UpdateDirection(); ApplySort();
  foreach (var row in _resultView.Cast<SearchRow>().Take(3))
  {
   var detail = Read<DetailRecord>(await _bridge.CallAsync("detail", new { kind = "Staff", id = row.Entity.Id }));
   var detailValues = detail.Fields.Concat(detail.Scores.ValueKind == JsonValueKind.Array ? Read<List<EditableField>>(detail.Scores) : []).ToDictionary(item => item.Key, item => item.Value);
   foreach (string key in new[] { attribute.Key, second.Key, score }.Distinct()) if (!detailValues.TryGetValue(key, out double expectedValue) || expectedValue != row.Values[key]) throw new InvalidOperationException("검색 열 값이 실제 스태프 상세와 일치하지 않습니다.");
  }
  int multipleMatches = _rows.Count;
  await CaptureAsync(Path.Combine(outputDir, "search-staff-multiple.png"));
  _suppress = true; _view.SelectedValue = "coaching"; _suppress = false; await RefreshColumnsAsync();
  if (!_complete || _rows.Count != multipleMatches || ViewFields().Any(key => !_loadedFields.Contains(key)) || _grid.Columns.Count < 10) throw new InvalidOperationException("코칭 보기 변경이 필요한 열을 다시 조회하지 못했습니다.");
  await CaptureAsync(Path.Combine(outputDir, "search-coaching-view.png"));
  _suppress = true; _view.SelectedValue = "selected"; _suppress = false; await RefreshColumnsAsync();
  var saved = new SavedSearch { Name = "QA 복수 스태프", Kind = _kind, Criteria = multiple.Clone() };
  string local = Path.Combine(outputDir, "search-presets-roundtrip.json"); await File.WriteAllTextAsync(local, JsonSerializer.Serialize(new[] { saved }, new JsonSerializerOptions(JsonDefaults.Options) { WriteIndented = true }));
  var restored = JsonSerializer.Deserialize<List<SavedSearch>>(await File.ReadAllTextAsync(local), JsonDefaults.Options)!.Single(); ValidateCriteria(restored.Criteria);
  if (restored.Kind != _kind || JsonSerializer.Serialize(restored.Criteria) != JsonSerializer.Serialize(multiple)) throw new InvalidOperationException("로컬 검색 조건 저장 / 불러오기 검증 실패");
  var empty = multiple.Clone(); empty.Name = "__touchline_no_such_person_8E1349__"; await ApplyCriteriaAsync(empty); if (!_complete || _rows.Count != 0 || _matched != 0 || _emptyPanel.Visibility != Visibility.Visible) throw new InvalidOperationException("검색 결과 없음 화면 검증 실패");
  await CaptureAsync(Path.Combine(outputDir, "search-empty.png"));
  await ResetAsync(); if (_criteria.Conditions.Count != 0 || _criteria.Name != "" || _criteria.Scope != "all" || _criteria.Employment != "all" || _criteria.FootballCategory != null) throw new InvalidOperationException("검색 조건 초기화 검증 실패");
  _qaAfterStart = () => _cancellation?.Cancel(); try { await SearchAsync(); } finally { _qaAfterStart = null; }
  if (!_lastCancelled || _queryId != null || _searching) throw new InvalidOperationException("검색 커서 취소 검증 실패");
  await ApplyCriteriaAsync(multiple); _grid.SelectedItem = _resultView.Cast<SearchRow>().First(); await CaptureAsync(Path.Combine(outputDir, "search-ready.png"));
  string beforeProfile = JsonSerializer.Serialize(_criteria); var idsBeforeProfile = _rows.Select(row => row.Entity.Id).ToArray(); int before = _navigationRequests; await OpenSelectedAsync();
  if (_navigationRequests != before + 1) throw new InvalidOperationException("프로필 열기 검증 실패");
  await SetKindAsync("Staff"); if (beforeProfile != JsonSerializer.Serialize(_criteria) || !idsBeforeProfile.SequenceEqual(_rows.Select(row => row.Entity.Id))) throw new InvalidOperationException("프로필 복귀 후 검색 조건 / 결과 보존 실패");
  return new { success = true, kind = _kind, defaultScope = "all", dialog = applied.Report, actualDialogApply = true, newDialogCancelPreservesApplied = true, editDialogCancelPreservesApplied = true, verifiedValues, multipleMatches, numericAscendingAndDescending = true, displayedValuesMatchDetails = true, tableBelowToolbar = true, fullWidthResults = true, viewColumnsRequeried = true, localPresetRoundTrip = true, resultCount = _matched, emptyState = true, reset = true, cancellationByQueryId = true, navigationRequested = true, profileReturnPreservesCriteria = true, live = false };
 }
 private async Task CaptureAsync(string path)
 {
  await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  if (ActualWidth <= 0 || ActualHeight <= 0) throw new InvalidOperationException("검색 화면을 표시한 뒤 QA를 실행하세요.");
  Rect toolbarBounds = _toolbar.TransformToAncestor(this).TransformBounds(new Rect(_toolbar.RenderSize));
  Rect tableBounds = _tableCard.TransformToAncestor(this).TransformBounds(new Rect(_tableCard.RenderSize));
  if (_toolbar.ActualHeight <= 0 || tableBounds.Top < toolbarBounds.Bottom - 0.5 || tableBounds.Width < ActualWidth - 45 || tableBounds.Height < 100) throw new InvalidOperationException("검색 결과 표가 도구 모음을 덮거나 사용 가능한 영역을 벗어났습니다.");
  Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!); var bitmap = new RenderTargetBitmap((int)Math.Ceiling(ActualWidth), (int)Math.Ceiling(ActualHeight), 96, 96, PixelFormats.Pbgra32); bitmap.Render(this); var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(path); encoder.Save(stream);
 }
}
