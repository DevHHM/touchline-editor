using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;

namespace Touchline.Wpf;

public sealed partial class LeagueBrowserView : UserControl
{
 private readonly BridgeClient _bridge;
 private readonly Func<EntityRecord, Task> _openClub;
 private readonly ComboBox _nations = new() { DisplayMemberPath = nameof(LeagueNationChoice.Name), SelectedValuePath = nameof(LeagueNationChoice.Key), MinWidth = 160 };
 private readonly ComboBox _categories = new() { DisplayMemberPath = "Label", SelectedValuePath = "Key", MinWidth = 130 };
 private readonly TextBox _leagueSearch = new() { ToolTip = "리그 이름 또는 ID 검색 (Ctrl+F)", MaxLength = 200 };
 private readonly TextBox _clubSearch = new() { ToolTip = "선택한 리그 안에서 구단 이름·영문 이름·ID 검색", MaxLength = 200 };
 private readonly DataGrid _leagues = new() { IsReadOnly = true, RowHeight = 64, CanUserSortColumns = false };
 private readonly DataGrid _clubs = new() { IsReadOnly = true, RowHeight = 48 };
 private readonly TextBlock _summary = Text("리그 소속을 불러오는 중…", 12, true);
 private readonly TextBlock _heading = Text("리그를 선택하세요", 23);
 private readonly TextBlock _description = Text("", 12, true);
 private readonly TextBlock _status = Text("", 12, true);
 private readonly TextBlock _warnings = Text("", 12, true);
 private readonly TextBlock _empty = Text("표시할 구단이 없습니다.", 14, true);
 private readonly Button _open = new() { Content = "구단 상세 열기 ↗", IsEnabled = false };
 private LeagueCatalog? _catalog;
 private bool _updating, _invalidated;
 private int _loadGeneration;
 public LeagueBrowserView(BridgeClient bridge, Func<EntityRecord, Task> openClub)
 {
  _bridge = bridge; _openClub = openClub;
  var root = new Grid { Margin = new Thickness(24, 22, 24, 18) }; Content = root;
  foreach (var height in new[] { GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto }) root.RowDefinitions.Add(new() { Height = height });
  var title = new DockPanel { Margin = new Thickness(0, 0, 0, 18) };
  var reset = new Button { Content = "필터 초기화", Padding = new Thickness(12, 7, 12, 7), VerticalAlignment = VerticalAlignment.Top };
  reset.Click += (_, _) => RestoreState(new()); DockPanel.SetDock(reset, Dock.Right); title.Children.Add(reset);
  var titleText = new StackPanel(); titleText.Children.Add(Text("리그·구단", 25)); _summary.Margin = new Thickness(0, 7, 0, 0); titleText.Children.Add(_summary); title.Children.Add(titleText); root.Children.Add(title);
  var filters = new Grid { Margin = new Thickness(0, 0, 0, 18) };
  filters.ColumnDefinitions.Add(new() { Width = new GridLength(240) }); filters.ColumnDefinitions.Add(new() { Width = new GridLength(180) }); filters.ColumnDefinitions.Add(new());
  filters.Children.Add(Labeled("대회 국가", _nations)); var category = Labeled("남자 / 여자 축구", _categories); Grid.SetColumn(category, 1); filters.Children.Add(category);
  var hint = Text("리그를 고르면 소속 구단을 볼 수 있습니다.\n구단을 두 번 클릭하거나 Enter로 상세 화면을 여세요.", 12, true); hint.Margin = new Thickness(12, 0, 0, 0); Grid.SetColumn(hint, 2); filters.Children.Add(hint); Grid.SetRow(filters, 1); root.Children.Add(filters);
  var body = new Grid(); body.ColumnDefinitions.Add(new() { Width = new GridLength(300), MinWidth = 240 }); body.ColumnDefinitions.Add(new() { Width = new GridLength(18) }); body.ColumnDefinitions.Add(new()); Grid.SetRow(body, 2); root.Children.Add(body);
  var left = new Grid(); left.RowDefinitions.Add(new() { Height = GridLength.Auto }); left.RowDefinitions.Add(new()); left.Children.Add(Labeled("리그 검색", _leagueSearch)); Grid.SetRow(_leagues, 1); left.Children.Add(_leagues); body.Children.Add(left);
  _leagues.HeadersVisibility = DataGridHeadersVisibility.None;
  var template = new DataTemplate(); var stack = new FrameworkElementFactory(typeof(StackPanel));
  var name = new FrameworkElementFactory(typeof(TextBlock)); name.SetBinding(TextBlock.TextProperty, new Binding(nameof(LeagueGroup.Name))); name.SetValue(TextBlock.FontSizeProperty, 14.0); name.SetValue(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis); stack.AppendChild(name);
  var subtitle = new FrameworkElementFactory(typeof(TextBlock)); subtitle.SetBinding(TextBlock.TextProperty, new Binding(nameof(LeagueGroup.Subtitle))); subtitle.SetValue(TextBlock.ForegroundProperty, Brush("MutedBrush")); subtitle.SetValue(TextBlock.FontSizeProperty, 11.0); subtitle.SetValue(TextBlock.MarginProperty, new Thickness(0, 6, 0, 0)); subtitle.SetValue(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis); stack.AppendChild(subtitle); template.VisualTree = stack;
  _leagues.Columns.Add(new DataGridTemplateColumn { CellTemplate = template, Width = new DataGridLength(1, DataGridLengthUnitType.Star) });
  var right = new Grid(); foreach (var height in new[] { GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto }) right.RowDefinitions.Add(new() { Height = height }); Grid.SetColumn(right, 2); body.Children.Add(right);
  var intro = new StackPanel { Margin = new Thickness(0, 0, 0, 14) }; intro.Children.Add(_heading); _description.Margin = new Thickness(0, 6, 0, 0); intro.Children.Add(_description); right.Children.Add(intro);
  var clubFilter = Labeled("구단 검색", _clubSearch); Grid.SetRow(clubFilter, 1); right.Children.Add(clubFilter);
  var table = new Grid(); Grid.SetRow(table, 2); table.Children.Add(_clubs); _empty.HorizontalAlignment = HorizontalAlignment.Center; _empty.VerticalAlignment = VerticalAlignment.Center; _empty.IsHitTestVisible = false; table.Children.Add(_empty); right.Children.Add(table);
  _clubs.Columns.Add(Column("구단", nameof(EntityRecord.DisplayName), new DataGridLength(1, DataGridLengthUnitType.Star)));
  _clubs.Columns.Add(Column("축구 구분", nameof(EntityRecord.FootballCategoryLabel), new DataGridLength(100)));
  _clubs.Columns.Add(Column("ID", nameof(EntityRecord.Id), new DataGridLength(88)));
  var footer = new DockPanel { Margin = new Thickness(0, 12, 0, 0) }; DockPanel.SetDock(_open, Dock.Right); footer.Children.Add(_open); footer.Children.Add(_status); Grid.SetRow(footer, 3); right.Children.Add(footer);
  _warnings.Margin = new Thickness(0, 16, 0, 0); Grid.SetRow(_warnings, 3); root.Children.Add(_warnings);
  _categories.ItemsSource = new[] { new { Key = "", Label = "모든 축구" }, new { Key = "men", Label = "남자축구" }, new { Key = "women", Label = "여자축구" }, new { Key = "unknown", Label = "미확인" } }; _categories.SelectedIndex = 0;
  _nations.SelectionChanged += (_, _) => UpdateLeagues(); _categories.SelectionChanged += (_, _) => UpdateLeagues(); _leagueSearch.TextChanged += (_, _) => UpdateLeagues(); _clubSearch.TextChanged += (_, _) => UpdateClubs();
  _leagues.SelectionChanged += (_, _) => { if (!_updating) UpdateClubs(); };
  _clubs.SelectionChanged += (_, _) => _open.IsEnabled = !_invalidated && _clubs.SelectedItem is EntityRecord;
  _clubs.MouseDoubleClick += async (_, e) => { if (e.OriginalSource is DependencyObject source && HasRow(source)) await OpenSelectedAsync(); };
  _clubs.KeyDown += async (_, e) => { if (e.Key == Key.Enter) { e.Handled = true; await OpenSelectedAsync(); } };
  _open.Click += async (_, _) => await OpenSelectedAsync();
 }
 private static Brush Brush(string key) => (Brush)Application.Current.FindResource(key);
 private static TextBlock Text(string text, double size, bool muted = false) => new() { Text = text, FontSize = size, TextWrapping = TextWrapping.Wrap, Foreground = Brush(muted ? "MutedBrush" : "InkBrush") };
 private static StackPanel Labeled(string name, FrameworkElement input) { var panel = new StackPanel { Margin = new Thickness(0, 0, 12, 10) }; var label = Text(name, 11, true); label.Margin = new Thickness(0, 0, 0, 6); panel.Children.Add(label); panel.Children.Add(input); System.Windows.Automation.AutomationProperties.SetName(input, name); return panel; }
 private static DataGridTextColumn Column(string title, string property, DataGridLength width) => new() { Header = title, Binding = new Binding(property), Width = width, ElementStyle = new Style(typeof(TextBlock)) { Setters = { new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis) } } };
 private static bool HasRow(DependencyObject source) { while (source != null) { if (source is DataGridRow) return true; source = VisualTreeHelper.GetParent(source); } return false; }
 public void FocusSearch() { _leagueSearch.Focus(); _leagueSearch.SelectAll(); }
 public LeagueBrowserState CaptureState() => new(_nations.SelectedValue as string ?? "", _categories.SelectedValue as string ?? "", _leagueSearch.Text, _clubSearch.Text, (_leagues.SelectedItem as LeagueGroup)?.Key);
 public void Invalidate() { _invalidated = true; ++_loadGeneration; _catalog = null; _leagues.ItemsSource = null; _clubs.ItemsSource = null; _open.IsEnabled = false; _status.Text = "연결된 세이브가 바뀌어 다시 불러와야 합니다."; }
 public async Task LoadAsync(LeagueBrowserState? state = null, bool refresh = false)
 {
  if (_catalog != null && !refresh) { if (state != null) RestoreState(state); return; }
  int generation = ++_loadGeneration; state ??= CaptureState();
  var data = await _bridge.CallAsync("league_catalog", new { refresh });
  if (generation != _loadGeneration) return;
  _catalog = data.Deserialize<LeagueCatalog>(JsonDefaults.Options) ?? throw new InvalidOperationException("리그 카탈로그를 읽을 수 없습니다."); _invalidated = false;
  _updating = true; _nations.ItemsSource = LeagueBrowserData.Nations(_catalog.Leagues); _updating = false;
  _summary.Text = $"{_catalog.Leagues.Count(group => group.Id.HasValue):N0}개 리그 · {_catalog.TotalClubs:N0}개 구단 · 소속 확인 {_catalog.KnownClubs:N0}개";
  _warnings.Text = string.Join("\n", _catalog.Warnings); _warnings.Visibility = _catalog.Warnings.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
  RestoreState(state);
 }
 public void RestoreState(LeagueBrowserState state)
 {
  _updating = true;
  _nations.SelectedValue = _nations.Items.Cast<LeagueNationChoice>().Any(nation => nation.Key == state.NationKey) ? state.NationKey : "";
  _categories.SelectedValue = state.Category; if (_categories.SelectedIndex < 0) _categories.SelectedIndex = 0;
  _leagueSearch.Text = state.LeagueQuery; _clubSearch.Text = state.ClubQuery; _updating = false; UpdateLeagues(state.LeagueKey);
 }
 private void UpdateLeagues(string? selectedKey = null)
 {
  if (_updating || _catalog == null) return;
  selectedKey ??= (_leagues.SelectedItem as LeagueGroup)?.Key;
  var groups = LeagueBrowserData.FilterLeagues(_catalog.Leagues, CaptureState());
  _updating = true; _leagues.ItemsSource = groups; _leagues.SelectedItem = groups.FirstOrDefault(group => group.Key == selectedKey) ?? groups.FirstOrDefault(); _updating = false; UpdateClubs();
 }
 private void UpdateClubs()
 {
  if (_updating) return;
  var league = _leagues.SelectedItem as LeagueGroup; var clubs = LeagueBrowserData.FilterClubs(league, _clubSearch.Text); _clubs.ItemsSource = clubs;
  _heading.Text = league?.Name ?? "검색 결과가 없습니다"; _description.Text = league?.Subtitle ?? "국가·축구 구분 또는 검색어를 바꿔보세요.";
  _status.Text = $"{clubs.Count:N0}개 구단"; _empty.Visibility = clubs.Count == 0 ? Visibility.Visible : Visibility.Collapsed; _open.IsEnabled = false;
 }
 private async Task OpenSelectedAsync()
 {
  if (_invalidated || _clubs.SelectedItem is not EntityRecord club) return;
  try { await _openClub(club); } catch (Exception error) { App.Log(error); _status.Text = error.Message; }
 }
}
