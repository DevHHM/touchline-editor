using System.Collections;
using System.ComponentModel;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

/// <summary>Read-only club roster with complete paging and filters based on explicit squad membership.</summary>
public sealed class ClubRosterView : UserControl
{
 private sealed record RosterPage(List<EntityRecord> Items, int Total);
 private readonly Func<int, CancellationToken, Task<RosterPage>> _fetch;
 private readonly Func<CancellationToken, Task<ClubGrowthResponse>>? _fetchGrowth;
 private readonly string _growthUnavailableReason;
 private BridgeClient? _localizationBridge;
 private NameLocalizationWindow? _localizationWindow;
 private readonly EntityRecord _club;
 private readonly bool _live;
 private readonly Func<EntityRecord, Task>? _open;
 private readonly CancellationTokenSource _lifetime = new();
 private readonly StackPanel _root = new();
 private readonly WrapPanel _filters = new() { Margin = new Thickness(0, 2, 0, 6) };
 private readonly Dictionary<string, Button> _groupButtons = new(StringComparer.Ordinal);
 private readonly TextBlock _heading = Text("선수단", 20, true);
 private readonly TextBlock _coverage = Text("소속 팀 정보를 불러옵니다.", 11);
 private readonly TextBlock _status = Text("선수 명단을 불러오는 중…", 12);
 private readonly TextBlock _empty = Text("선수가 없습니다.", 14);
 private readonly TextBlock _growthStatus = Text("성장 정보를 불러오는 중…", 11);
 private readonly TextBlock _growthDetails = Text("선수를 선택하면 적용 팀과 계산 근거를 확인할 수 있습니다.", 11);
 private readonly Button _refreshGrowth = new() { Content = "성장 정보 새로 읽기", Padding = new Thickness(10, 5, 10, 5), FontSize = 11, Margin = new Thickness(10, 0, 0, 6) };
 private readonly Button _openProfile = new() { Content = "선택한 선수 프로필 ↗", Padding = new Thickness(12, 7, 12, 7), HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 10, 0, 0) };
 private readonly Button _localize = new() { Content = "선수명 한글화…", Padding = new Thickness(12, 7, 12, 7), Margin = new Thickness(9, 10, 0, 0) };
 private readonly DataGrid _grid = new() { IsReadOnly = true, AutoGenerateColumns = false, CanUserAddRows = false, CanUserDeleteRows = false, CanUserSortColumns = true, CanUserResizeColumns = true, CanUserReorderColumns = true, SelectionMode = DataGridSelectionMode.Single, FrozenColumnCount = 1, RowHeight = 39, Height = 440, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
 private readonly List<EntityRecord> _roster = [];
 private readonly Grid _tableHost = new();
 private IReadOnlyList<SquadGroupOption> _groups = [];
 private ListCollectionView? _rows;
 private ScrollViewer? _viewport;
 private TaskCompletionSource? _inflight;
 private string? _preferredGroup;
 private bool _active = true, _busy, _loaded, _growthBusy;
 private Task? _growthTask;
 private int _generation, _sourceTotal;
 public bool IsBusy => _busy || _growthBusy;
 public bool CanPollConnection => !IsBusy && _localizationWindow?.IsBusy != true;
 public bool HasPendingChanges => false;
 public string SelectedGroup { get; private set; } = SquadGroups.All;
 public event Action<string>? GroupChanged;

 public ClubRosterView(BridgeClient bridge, EntityRecord club, bool live, Func<EntityRecord, Task>? open = null, string? selectedGroup = null, ClubGrowthAvailability? growthAvailability = null)
  : this(async (offset, cancellation) =>
  {
   var response = await bridge.CallAsync("search", new { kind = "Player", club_id = club.Id, limit = 500, offset, sort = "ca", descending = true }, cancellation);
   return new RosterPage(response.GetProperty("items").Deserialize<List<EntityRecord>>(JsonDefaults.Options) ?? throw new InvalidDataException("선수 명단을 읽지 못했습니다."), response.GetProperty("total").GetInt32());
  }, club, live, open, selectedGroup, (growthAvailability ?? ClubGrowthAvailability.Unavailable).GateFetcher(async cancellation =>
  {
   var response = await bridge.CallAsync("club_growth", new { club_id = club.Id }, cancellation);
   return response.Deserialize<ClubGrowthResponse>(JsonDefaults.Options) ?? throw new InvalidDataException("성장 정보를 읽지 못했습니다.");
  }), (growthAvailability ?? ClubGrowthAvailability.Unavailable).UnavailableMessage) { _localizationBridge = bridge; UpdateControls(); }

 private ClubRosterView(Func<int, CancellationToken, Task<RosterPage>> fetch, EntityRecord club, bool live, Func<EntityRecord, Task>? open, string? selectedGroup = null, Func<CancellationToken, Task<ClubGrowthResponse>>? fetchGrowth = null, string? growthUnavailableReason = null)
 {
  _fetch = fetch; _fetchGrowth = fetchGrowth; _growthUnavailableReason = growthUnavailableReason ?? "이 자료에는 성장 참고치가 없습니다. 선수 명단과 CA·PA를 표시합니다."; _club = club; _live = live; _open = open; _preferredGroup = selectedGroup;
  FontFamily = Typography.AppFont; FontWeight = FontWeights.Medium; FontSize = 13; Foreground = Brush("InkBrush"); UseLayoutRounding = true; SnapsToDevicePixels = true;
  TextOptions.SetTextFormattingMode(this, TextFormattingMode.Display); SetResourceReference(TextOptions.TextRenderingModeProperty, "AppTextRenderingMode");
  Content = _root; _root.Children.Add(_heading); _root.Children.Add(_coverage); _root.Children.Add(_filters); _root.Children.Add(_status);
  AddNameColumn(); AddColumn("포지션", nameof(EntityRecord.Role), 90, 1); AddColumn("소속 팀 / 임대", nameof(EntityRecord.Group), 120, 1.3);
  AddColumn("나이", nameof(EntityRecord.RosterAge), 54); AddColumn("CA", nameof(EntityRecord.RosterCa), 52); AddColumn("PA", nameof(EntityRecord.RosterPa), 52);
  AddGrowthColumn("성장 참고치", nameof(EntityRecord.GrowthReferenceText), nameof(EntityRecord.GrowthReference), 100);
  AddGrowthColumn("CA 대비", nameof(EntityRecord.GrowthGapText), nameof(EntityRecord.GrowthGap), 78);
  _root.Children.Add(Text(ClubGrowthItem.Disclaimer + " · CA 대비 = 참고치 − 현재 CA · 음수는 쇠퇴 예측 아님", 11));
  var growthBar = new DockPanel(); DockPanel.SetDock(_refreshGrowth, Dock.Right); growthBar.Children.Add(_refreshGrowth); growthBar.Children.Add(_growthStatus); _root.Children.Add(growthBar);
  _tableHost.Children.Add(_grid);
  _empty.HorizontalAlignment = HorizontalAlignment.Center; _empty.VerticalAlignment = VerticalAlignment.Center; _empty.Margin = new Thickness(20); _empty.IsHitTestVisible = false; _empty.Visibility = Visibility.Collapsed; _tableHost.Children.Add(_empty);
  _root.Children.Add(new Border { Child = _tableHost, Padding = new Thickness(1), CornerRadius = new CornerRadius(7), BorderBrush = Brush("BorderBrush"), BorderThickness = new Thickness(1), Background = Brush("PanelBrush"), ClipToBounds = true });
  var footer = new WrapPanel(); footer.Children.Add(_openProfile); footer.Children.Add(_localize); footer.Children.Add(new TextBlock { Text = "프로필: 이름 클릭 또는 행 두 번 클릭\n우클릭: 이름 복사 · Ctrl+C / Ctrl+Shift+C", Foreground = Brush("MutedBrush"), FontSize = 11, Margin = new Thickness(12, 10, 0, 0), VerticalAlignment = VerticalAlignment.Center }); _root.Children.Add(footer);
  _growthDetails.Margin = new Thickness(0, 8, 0, 0); _root.Children.Add(_growthDetails);
  EntityRoleSorting.Attach(_grid);
  NameCopy.AttachGrid(_grid, item => item as EntityRecord, message => _status.Text = message);
  _grid.Sorting += GrowthSorting;
  _grid.SelectionChanged += (_, _) => { UpdateGrowthDetails(); UpdateControls(); };
  _grid.MouseDoubleClick += async (_, args) => { if (IsDataRow(args.OriginalSource)) await OpenAsync(_grid.SelectedItem as EntityRecord); };
  _grid.PreviewKeyDown += async (_, args) => { if (args.Key == Key.Enter && _grid.SelectedItem is EntityRecord person) { args.Handled = true; await OpenAsync(person); } };
  _openProfile.Click += async (_, _) => await OpenAsync(_grid.SelectedItem as EntityRecord);
  _refreshGrowth.Click += async (_, _) => await RefreshGrowthAsync();
  _localize.Click += (_, _) => OpenNameLocalization();
  Loaded += (_, _) => { AttachViewport(); Dispatcher.BeginInvoke(UpdateViewportHeight, DispatcherPriority.Loaded); };
  Unloaded += (_, _) => DetachViewport();
  UpdateControls();
 }

 private static Brush Brush(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Text(string value, double size = 12, bool bold = false) => new() { Text = value, FontSize = size, Foreground = Brush(bold ? "InkBrush" : "MutedBrush"), FontWeight = bold ? FontWeights.SemiBold : FontWeights.Medium, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 0, 8) };

 private void AddNameColumn()
 {
  var name = new FrameworkElementFactory(typeof(Button));
  name.SetResourceReference(StyleProperty, "QuietButton"); name.SetValue(PaddingProperty, new Thickness(0)); name.SetValue(HorizontalContentAlignmentProperty, HorizontalAlignment.Left); name.SetValue(ForegroundProperty, Brush("AccentBrush"));
  name.SetBinding(ToolTipProperty, new Binding(nameof(EntityRecord.DisplayName)));
  var label = new FrameworkElementFactory(typeof(TextBlock)); label.SetBinding(TextBlock.TextProperty, new Binding(nameof(EntityRecord.DisplayName))); label.SetValue(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis); name.AppendChild(label);
  name.AddHandler(Button.ClickEvent, new RoutedEventHandler(async (sender, args) => { args.Handled = true; if (sender is FrameworkElement { DataContext: EntityRecord person }) await OpenAsync(person); }));
  _grid.Columns.Add(new DataGridTemplateColumn { Header = "이름 ↗", CellTemplate = new DataTemplate { VisualTree = name }, SortMemberPath = nameof(EntityRecord.DisplayName), Width = new DataGridLength(2, DataGridLengthUnitType.Star), MinWidth = 150 });
 }

 private void AddColumn(string label, string property, double minWidth, double star = 0)
 {
  var style = new Style(typeof(TextBlock), Application.Current.TryFindResource(typeof(TextBlock)) as Style);
  style.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis)); style.Setters.Add(new Setter(ToolTipService.ToolTipProperty, new Binding(property)));
  if (star == 0) style.Setters.Add(new Setter(TextBlock.TextAlignmentProperty, TextAlignment.Right));
  _grid.Columns.Add(new DataGridTextColumn { Header = label, Binding = new Binding(property) { TargetNullValue = "—" }, SortMemberPath = property, ElementStyle = style, MinWidth = minWidth, Width = star > 0 ? new DataGridLength(star, DataGridLengthUnitType.Star) : new DataGridLength(minWidth) });
 }

 private void AddGrowthColumn(string label, string display, string sort, double width)
 {
  var style = new Style(typeof(TextBlock), Application.Current.TryFindResource(typeof(TextBlock)) as Style);
  style.Setters.Add(new Setter(TextBlock.TextAlignmentProperty, TextAlignment.Right));
  style.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis));
  style.Setters.Add(new Setter(ToolTipService.ToolTipProperty, new Binding(nameof(EntityRecord.GrowthDetails))));
  _grid.Columns.Add(new DataGridTextColumn { Header = label, Binding = new Binding(display), SortMemberPath = sort, ElementStyle = style, Width = width, MinWidth = width });
 }

 private sealed class GrowthComparer(string property, bool descending) : IComparer
 {
  public int Compare(object? x, object? y)
  {
   if (x is not EntityRecord a || y is not EntityRecord b) return 0;
   long? av = property == nameof(EntityRecord.GrowthGap) ? a.GrowthGap : a.GrowthReference;
   long? bv = property == nameof(EntityRecord.GrowthGap) ? b.GrowthGap : b.GrowthReference;
   if (av.HasValue != bv.HasValue) return av.HasValue ? -1 : 1;
   int result = av.HasValue ? av.Value.CompareTo(bv!.Value) : 0;
   return result == 0 ? a.Id.CompareTo(b.Id) : descending ? -result : result;
  }
 }

 private void GrowthSorting(object sender, DataGridSortingEventArgs args)
 {
  if (_rows == null) return;
  if (args.Column.SortMemberPath is not (nameof(EntityRecord.GrowthReference) or nameof(EntityRecord.GrowthGap)))
  {
   if (_rows.CustomSort is GrowthComparer) _rows.CustomSort = null;
   return;
  }
  args.Handled = true; bool descending = args.Column.SortDirection == ListSortDirection.Ascending;
  _rows.CustomSort = new GrowthComparer(args.Column.SortMemberPath, descending);
  foreach (var column in _grid.Columns) column.SortDirection = null;
  args.Column.SortDirection = descending ? ListSortDirection.Descending : ListSortDirection.Ascending;
 }

 private static bool IsDataRow(object source)
 {
  for (var element = source as DependencyObject; element != null; element = element is Visual ? VisualTreeHelper.GetParent(element) : LogicalTreeHelper.GetParent(element)) if (element is DataGridRow) return true;
  return false;
 }

 public async Task LoadAsync()
 {
  if (!_active || _busy || _loaded) return;
  _busy = true; var completion = _inflight = new(TaskCreationOptions.RunContinuationsAsynchronously); int generation = _generation; UpdateControls();
  try
  {
   var roster = new List<EntityRecord>(); var identities = new HashSet<string>(StringComparer.Ordinal); int offset = 0, total = -1;
   while (_active && generation == _generation)
   {
    var page = await _fetch(offset, _lifetime.Token);
    if (!_active || generation != _generation) return;
    if (page.Total < 0 || total >= 0 && total != page.Total) throw new InvalidDataException("조회 중 선수 수가 변경되었습니다. 새로고침한 뒤 다시 확인하세요.");
    total = page.Total;
    if (page.Items.Count == 0 && offset < total) throw new InvalidDataException("선수 명단의 일부를 받지 못했습니다. 새로고침한 뒤 다시 확인하세요.");
    foreach (var person in page.Items)
    {
     if (person.Kind != "Player" || person.ClubId != _club.Id || !identities.Add(person.Identity)) throw new InvalidDataException("선수 명단의 소속 또는 중복 정보를 확인하지 못했습니다.");
     roster.Add(person);
    }
    offset += page.Items.Count;
    if (offset > total) throw new InvalidDataException("선수 명단의 전체 인원수와 조회 결과가 일치하지 않습니다.");
    _status.Text = $"선수 명단 {offset:N0} / {total:N0}명 불러오는 중…";
    if (offset >= total) break;
   }
   if (!_active || generation != _generation) return;
   _sourceTotal = total; _roster.AddRange(roster); _groups = SquadGroups.Options(roster.Select(person => person.Group));
   _rows = new ListCollectionView(_roster); _rows.SortDescriptions.Add(new SortDescription(nameof(EntityRecord.RosterCa), ListSortDirection.Descending)); _rows.SortDescriptions.Add(new SortDescription(nameof(EntityRecord.DisplayName), ListSortDirection.Ascending));
   _rows.Filter = item => item is EntityRecord person && (SelectedGroup == SquadGroups.All || SquadGroups.Key(person.Group) == SelectedGroup); _grid.ItemsSource = _rows;
   _grid.Columns.First(column => column.SortMemberPath == nameof(EntityRecord.RosterCa)).SortDirection = ListSortDirection.Descending;
   _loaded = true; BuildGroups(); SelectGroup(SquadGroups.DefaultKey(_groups, _preferredGroup));
   _coverage.Text = $"{(_live ? "현재 연결 자료" : "저장된 스냅샷")} · 전체 {total:N0}명 조회 완료 · 실제 소속 팀 기준";
   if (_groups.Any(group => group.Key == SquadGroups.Unknown)) _coverage.Text += " · 미확인 소속은 별도 표시";
   _ = Dispatcher.BeginInvoke(UpdateViewportHeight, DispatcherPriority.Loaded);
   _busy = false; UpdateControls(); await RefreshGrowthAsync();
  }
  catch (OperationCanceledException) when (!_active || _lifetime.IsCancellationRequested) { }
  catch (Exception error) { App.Log(error); if (_active) { _status.Text = "명단 조회 실패 · " + error.Message; _coverage.Text = "전체 명단을 확인하지 못했습니다. 새로고침하여 다시 불러오세요."; _empty.Text = "선수 명단을 불러오지 못했습니다."; _empty.Visibility = Visibility.Visible; } }
  finally { _busy = false; UpdateControls(); completion.TrySetResult(); }
 }

 public Task RefreshGrowthAsync()
 {
  if (!_active || !_loaded || _growthBusy || _busy) return Task.CompletedTask;
  if (_fetchGrowth == null)
  {
   foreach (var person in _roster) person.SetGrowth(null, _growthUnavailableReason);
   _growthStatus.Text = _growthUnavailableReason; _growthStatus.ToolTip = _growthUnavailableReason;
   _rows?.Refresh(); UpdateGrowthDetails(); UpdateControls();
   return Task.CompletedTask;
  }
  return _growthTask = LoadGrowthAsync(_generation);
 }

 private async Task LoadGrowthAsync(int generation)
 {
  _growthBusy = true;
  foreach (var person in _roster) person.SetGrowth(null, "성장 정보를 불러오는 중입니다.");
  _growthStatus.Text = "성장 정보를 불러오는 중… · 현재 표는 기존 명단 수치입니다.";
  UpdateGrowthDetails(); UpdateControls();
  try
  {
   if (_fetchGrowth == null) throw new InvalidOperationException("이 자료에는 성장 참고치가 없습니다.");
   var response = await _fetchGrowth(_lifetime.Token);
   if (!_active || generation != _generation) return;
   if (response.ClubId != _club.Id || response.Items == null || response.Items.Select(item => item.PlayerId).Distinct().Count() != response.Items.Count)
    throw new InvalidDataException("성장 정보의 구단 또는 선수 중복 정보를 확인하지 못했습니다.");
   var items = response.Items.ToDictionary(item => item.PlayerId);
   string missing = "응답에 이 선수의 성장 정보가 없습니다." + (response.Warnings.Count > 0 ? "\n" + string.Join("\n", response.Warnings) : "");
   foreach (var person in _roster) person.SetGrowth(items.GetValueOrDefault(person.Id), missing);
   int calculated = _roster.Count(person => person.GrowthReference.HasValue), unknown = _roster.Count - calculated;
   _growthStatus.Text = calculated == 0 && _roster.Count > 0 ? "성장 참고치 미확인 · 선수 선택으로 원인을 확인하거나 새로 읽어 주세요." : $"성장 참고치 {calculated:N0}명 · 미확인 {unknown:N0}명";
   if (!string.IsNullOrWhiteSpace(response.GameDate)) _growthStatus.Text += " · 게임 날짜 " + response.GameDate;
   if (response.Warnings.Count > 0) _growthStatus.Text += $" · 확인 사항 {response.Warnings.Count}건";
   _growthStatus.ToolTip = $"{response.Scope}\n공식 버전: {response.FormulaVersion}\n{ClubGrowthItem.Disclaimer}\n{string.Join("\n", response.Warnings)}";
   _rows?.Refresh(); UpdateGrowthDetails();
  }
  catch (OperationCanceledException) when (!_active || _lifetime.IsCancellationRequested) { }
  catch (Exception error)
  {
   App.Log(error);
   if (_active && generation == _generation)
   {
    string reason = "성장 정보 조회 실패 · " + error.Message;
    foreach (var person in _roster) person.SetGrowth(null, reason);
    _growthStatus.Text = "성장 정보 조회 실패 · 기존 명단 수치를 표시합니다. 새로 읽어 주세요."; _growthStatus.ToolTip = reason;
    _rows?.Refresh(); UpdateGrowthDetails();
   }
  }
  finally { _growthBusy = false; UpdateControls(); _ = Dispatcher.BeginInvoke(UpdateViewportHeight, DispatcherPriority.Loaded); }
 }

 private void UpdateGrowthDetails()
 {
  _growthDetails.Text = _grid.SelectedItem is EntityRecord person ? person.DisplayName + " · " + person.GrowthDetails : _fetchGrowth == null ? _growthUnavailableReason : "선수를 선택하면 실제 적용 팀·명성·분모·계산 근거와 미확인 원인을 확인할 수 있습니다.";
  _ = Dispatcher.BeginInvoke(UpdateViewportHeight, DispatcherPriority.Loaded);
 }

 private void BuildGroups()
 {
  _filters.Children.Clear(); _groupButtons.Clear();
  foreach (var group in _groups)
  {
   var button = new Button { Content = group.Display, Tag = group.Key, Margin = new Thickness(0, 0, 7, 7), Padding = new Thickness(12, 7, 12, 7), FontSize = 12, ToolTip = group.Key == SquadGroups.All ? "이 구단의 전체 선수" : $"소속 팀: {group.Label} · {group.Count:N0}명" };
   button.Click += (_, _) => SelectGroup(group.Key); _groupButtons.Add(group.Key, button); _filters.Children.Add(button);
  }
 }

 private void SelectGroup(string key)
 {
  if (!_active || !_loaded || !_groupButtons.ContainsKey(key)) return;
  SelectedGroup = key; _preferredGroup = key; _rows?.Refresh(); _grid.SelectedItem = null;
  foreach (var (group, button) in _groupButtons)
  {
   bool selected = group == key; button.Background = Brush(selected ? "AccentBrush" : "RaisedBrush"); button.Foreground = selected ? new SolidColorBrush(Color.FromRgb(17, 42, 37)) : Brush("InkBrush"); button.BorderBrush = Brush(selected ? "AccentBrush" : "BorderBrush"); button.FontWeight = selected ? FontWeights.SemiBold : FontWeights.Medium;
   System.Windows.Automation.AutomationProperties.SetHelpText(button, selected ? "선택됨" : "선택되지 않음");
  }
  var option = _groups.First(group => group.Key == key); _heading.Text = key == SquadGroups.All ? "전체 선수단" : option.Label + " 선수단";
  _status.Text = $"{option.Label} {_grid.Items.Count:N0}명 · 전체 {_sourceTotal:N0}명";
  _empty.Text = "이 구단에 등록된 선수가 없습니다."; _empty.Visibility = _grid.Items.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
  if (_grid.Items.Count > 0) _grid.ScrollIntoView(_grid.Items[0]); UpdateControls(); GroupChanged?.Invoke(key);
 }

 private void UpdateControls()
 {
  _filters.IsEnabled = _active && !_busy && _loaded; _grid.IsEnabled = _active && !_busy && _loaded;
  _openProfile.IsEnabled = _active && !_busy && _open != null && _grid.SelectedItem is EntityRecord;
  _refreshGrowth.IsEnabled = _active && !_busy && !_growthBusy && _loaded && _fetchGrowth != null;
  _refreshGrowth.ToolTip = _fetchGrowth == null ? _growthUnavailableReason : "현재 선수단의 성장 참고치를 다시 조회합니다.";
  _localize.IsEnabled = _active && !_busy && !_growthBusy && _loaded && _localizationBridge != null && _grid.Items.Count > 0;
  _localize.ToolTip = $"현재 선수단 범위 {SelectedGroup} · {_grid.Items.Count}명의 이름 한글화 (최대 200명)";
 }

 private uint[] LocalizationPlayerIds()
 {
  var ids = _grid.Items.OfType<EntityRecord>().Select(person => person.Id).Distinct().ToArray();
  if (ids.Length is < 1 or > 200) throw new InvalidOperationException($"현재 범위는 {ids.Length}명입니다. 1군·U-20·U-18 등으로 범위를 좁혀 1–200명을 선택하세요.");
  return ids;
 }
 private void OpenNameLocalization()
 {
  if (!_active || _busy || _growthBusy || _localizationBridge == null) return;
  try
  {
   var window = new NameLocalizationWindow(_localizationBridge, LocalizationPlayerIds(), _club.DisplayName + " · " + _heading.Text) { Owner = Window.GetWindow(this) };
   _localizationWindow = window; try { window.ShowDialog(); } finally { _localizationWindow = null; }
   if (window.FilesChanged) _status.Text = "한글화 파일 변경 완료 · FM 재시작 후 세이브 다시 불러오기";
  }
  catch (Exception error) { App.Log(error); _status.Text = error.Message; }
 }

 private async Task OpenAsync(EntityRecord? person)
 {
  if (!_active || _busy || person == null || _open == null || !_roster.Contains(person)) return;
  try { await _open(person); } catch (Exception error) { App.Log(error); if (_active) _status.Text = "프로필을 열지 못했습니다 · " + error.Message; }
 }

 private void AttachViewport()
 {
  DetachViewport();
  for (DependencyObject? parent = VisualTreeHelper.GetParent(this); parent != null; parent = VisualTreeHelper.GetParent(parent)) if (parent is ScrollViewer viewport) { _viewport = viewport; viewport.SizeChanged += ViewportSizeChanged; break; }
 }
 private void DetachViewport() { if (_viewport != null) _viewport.SizeChanged -= ViewportSizeChanged; _viewport = null; }
 private void ViewportSizeChanged(object sender, SizeChangedEventArgs args) => UpdateViewportHeight();
 private void UpdateViewportHeight()
 {
  if (!_active || _viewport == null || !_tableHost.IsVisible || _viewport.ActualHeight <= 0) return;
  double top = _tableHost.TransformToAncestor(_viewport).Transform(new Point()).Y + _viewport.VerticalOffset;
  _grid.Height = Math.Clamp(_viewport.ActualHeight - top - 70 - _growthDetails.ActualHeight, 180, 1000);
 }

 public void Invalidate()
 {
  if (!_active) return; _active = false; ++_generation; _lifetime.Cancel();
  _localizationWindow?.InvalidateConnection();
  foreach (var person in _roster) person.SetGrowth(null, "연결 또는 화면이 변경되었습니다. 다시 불러오세요.");
  _growthStatus.Text = "성장 정보가 무효화되었습니다. 다시 연결하여 불러오세요."; _growthStatus.ToolTip = null;
  UpdateGrowthDetails(); DetachViewport(); UpdateControls();
 }
 public async Task StopAsync() { Invalidate(); if (_inflight != null) await _inflight.Task; if (_growthTask != null) await _growthTask; _lifetime.Dispose(); }

 public async Task<object> QaPositionOrderAsync(string directory, uint[] ascending, uint[] descending)
 {
  if (_live) throw new InvalidOperationException("포지션 정렬 QA는 스냅샷에서만 실행합니다.");
  if (!_loaded) await LoadAsync();
  SelectGroup(SquadGroups.All);
  var report = EntityRoleSorting.VerifyGrid(_grid, ascending, descending);
  await CaptureAsync(Path.Combine(directory, "club-position-order.png"));
  return report;
 }

 public async Task<object> QaAsync(string directory)
 {
  if (_live) throw new InvalidOperationException("선수단 화면 QA는 스냅샷에서 실행합니다.");
  if (!_loaded) await LoadAsync(); if (!_loaded) throw new InvalidOperationException("선수단 로드 실패 · " + _status.Text);
  Directory.CreateDirectory(directory); string selected = SelectedGroup;
  var reports = new List<object>();
  foreach (var group in _groups)
  {
   _groupButtons[group.Key].RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
   var visible = _grid.Items.OfType<EntityRecord>().ToList();
   if (visible.Count != group.Count || visible.Any(person => person.Kind != "Player" || person.ClubId != _club.Id || group.Key != SquadGroups.All && SquadGroups.Key(person.Group) != group.Key)) throw new InvalidOperationException("선수단 필터 인원수 또는 소속 불일치 · " + group.Label);
   reports.Add(new { group.Key, group.Label, group.Count, visible = visible.Select(person => person.Identity).ToArray() });
   if (group.Key is "1군" or "U20" or "U18" or SquadGroups.All) await CaptureAsync(Path.Combine(directory, "club-roster-" + (group.Key == SquadGroups.All ? "all" : group.Key) + ".png"));
  }
  AssertNumericSorting(); SelectGroup(selected);
  if (_grid.Items.Count > 0) { _grid.SelectedIndex = 0; if (_open != null && !_openProfile.IsEnabled) throw new InvalidOperationException("선수 프로필 연결이 비활성화되었습니다."); }
  var synthetic = await SyntheticQaAsync();
  return new { success = true, live = false, club = _club.Identity, sourceTotal = _sourceTotal, loaded = _roster.Count, selectedGroup = SelectedGroup, groups = reports, synthetic, gameWrites = 0 };
 }

 private void AssertNumericSorting()
 {
  SelectGroup(SquadGroups.All);
  foreach (string property in new[] { nameof(EntityRecord.RosterAge), nameof(EntityRecord.RosterCa), nameof(EntityRecord.RosterPa) })
  {
   _rows!.SortDescriptions.Clear(); _rows.SortDescriptions.Add(new SortDescription(property, ListSortDirection.Ascending));
   long[] actual = _grid.Items.OfType<EntityRecord>().Select(person => property == nameof(EntityRecord.RosterAge) ? person.RosterAge : property == nameof(EntityRecord.RosterCa) ? person.RosterCa : person.RosterPa).Where(value => value.HasValue).Select(value => value!.Value).ToArray();
   if (!actual.SequenceEqual(actual.Order())) throw new InvalidOperationException("선수단 숫자 정렬 실패 · " + property);
  }
  _rows!.SortDescriptions.Clear(); _rows.SortDescriptions.Add(new SortDescription(nameof(EntityRecord.RosterCa), ListSortDirection.Descending)); _rows.SortDescriptions.Add(new SortDescription(nameof(EntityRecord.DisplayName), ListSortDirection.Ascending));
 }

 private static async Task<object> SyntheticQaAsync()
 {
  var club = new EntityRecord { Kind = "Club", Id = 9000001, Name = "선수단 QA" };
  string[] labels = ["1군", "U-20", "U-18", "U-21", "2군", "", "임대 중 · 외부 구단", "1군 · 임대 영입", "U-18 · 임대 영입", "U-18 B", "Reserve"];
  var people = Enumerable.Range(0, 527).Select(index => new EntityRecord { Id = (uint)(index + 1), Kind = "Player", ClubId = club.Id, Name = $"선수 {index:000}", Group = labels[index % labels.Length], Age = (uint)(index % 23 + 16), Ca = (ushort)(index % 200 + 1), Pa = (ushort)(201 - index % 200) }).ToList();
  var offsets = new List<int>(); EntityRecord? opened = null;
  Task<RosterPage> Fetch(int offset, CancellationToken token) { token.ThrowIfCancellationRequested(); offsets.Add(offset); return Task.FromResult(new RosterPage(people.Skip(offset).Take(500).ToList(), people.Count)); }
  var view = new ClubRosterView(Fetch, club, false, person => { opened = person; return Task.CompletedTask; }); await view.LoadAsync();
  if (!view._loaded || view._roster.Count != 527 || !offsets.SequenceEqual(new[] { 0, 500 }) || view.SelectedGroup != "1군") throw new InvalidOperationException("선수단 전체 페이지 또는 1군 기본 선택 검증 실패.");
  foreach (var group in view._groups)
  {
   view._groupButtons[group.Key].RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
   var rows = view._grid.Items.OfType<EntityRecord>().ToList();
   if (rows.Count != group.Count || rows.Any(person => group.Key != SquadGroups.All && SquadGroups.Key(person.Group) != group.Key)) throw new InvalidOperationException("선수단 합성 필터 검증 실패.");
  }
  if (SquadGroups.Key("1군 · 임대 영입") != "1군" || SquadGroups.Key("U-18 · 임대 영입") != "U18" || SquadGroups.Key("임대 중 · 외부 구단") != SquadGroups.Loan || SquadGroups.Key("U-18 B") != "U-18 B") throw new InvalidOperationException("선수단 소속·임대 표기 검증 실패.");
  view.AssertNumericSorting(); view.SelectGroup("U20"); view._grid.SelectedIndex = 0;
  var expected = (EntityRecord)view._grid.SelectedItem; view._openProfile.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
  if (opened != expected) throw new InvalidOperationException("선수단 프로필 버튼 대상 검증 실패.");
  var restored = new ClubRosterView(Fetch, club, false, null, view.SelectedGroup); await restored.LoadAsync();
  if (restored.SelectedGroup != "U20") throw new InvalidOperationException("선수단 선택 복원 검증 실패.");
  var empty = new ClubRosterView((_, _) => Task.FromResult(new RosterPage([], 0)), club, false, null, "U20"); await empty.LoadAsync();
  if (!empty._loaded || empty.SelectedGroup != SquadGroups.All || empty._grid.Items.Count != 0 || empty._empty.Visibility != Visibility.Visible) throw new InvalidOperationException("빈 선수단 또는 사라진 선택 복구 검증 실패.");
  var youthOnly = people.Where(person => SquadGroups.Key(person.Group) == "U18").ToList();
  var withoutFirst = new ClubRosterView((_, _) => Task.FromResult(new RosterPage(youthOnly, youthOnly.Count)), club, false, null); await withoutFirst.LoadAsync();
  if (withoutFirst.SelectedGroup != SquadGroups.All || withoutFirst._grid.Items.Count != youthOnly.Count) throw new InvalidOperationException("1군이 없는 구단의 기본 선택 검증 실패.");
  var pending = new TaskCompletionSource<RosterPage>(TaskCreationOptions.RunContinuationsAsynchronously);
  var stale = new ClubRosterView((_, _) => pending.Task, club, false, null); var loading = stale.LoadAsync(); stale.Invalidate(); pending.SetResult(new RosterPage(people, people.Count)); await loading;
  if (stale._loaded || stale._roster.Count != 0 || stale.IsBusy) throw new InvalidOperationException("이전 선수단 응답 무효화 검증 실패.");
  await view.StopAsync(); await restored.StopAsync(); await stale.StopAsync(); await empty.StopAsync(); await withoutFirst.StopAsync();
  return new { rows = 527, pages = 2, groupCounts = true, exactLoanSuffix = true, dynamicGroups = true, numericSort = true, profileTarget = true, selectionRestored = true, emptyRoster = true, noFirstTeamFallback = true, staleResponseIgnored = true };
 }

 private async Task CaptureAsync(string path)
 {
  var window = Window.GetWindow(this) ?? throw new InvalidOperationException("선수단 화면을 표시한 뒤 QA를 실행하세요.");
  _viewport?.ScrollToTop(); await Dispatcher.InvokeAsync(window.UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  var bitmap = new RenderTargetBitmap((int)Math.Ceiling(window.ActualWidth), (int)Math.Ceiling(window.ActualHeight), 96, 96, PixelFormats.Pbgra32); bitmap.Render(window);
  var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(path); encoder.Save(stream);
 }
}
