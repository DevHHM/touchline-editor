using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

/// <summary>Local, transactional search-condition editing; this window has no bridge or game access.</summary>
public sealed class SearchCriteriaWindow : Window
{
 private readonly string _kind;
 private readonly SearchCriteria _original;
 private readonly List<EditableField> _catalog;
 private readonly Dictionary<string, EditableField> _fields;
 private readonly Dictionary<string, ConditionEditor> _conditions = [];
 private readonly Dictionary<string, CheckBox> _checks = [];
 private readonly List<AttributePage> _pages = [];
 private readonly StackPanel _conditionRows = new();
 private readonly TextBlock _conditionCount = Label("선택한 숫자 조건", 14, true), _error = Label("", 12), _empty = Label("능력치 또는 직무 점수 탭에서 원하는 항목을 선택하세요.");
 private readonly TabControl _tabs = new() { Name = "CriteriaTabs", Background = Brushes.Transparent, BorderThickness = new Thickness(0) };
 private readonly TextBox _name = Input(), _club = Input();
 private readonly ComboBox _scope = ChoiceBox(), _role = ChoiceBox(), _employment = ChoiceBox(), _group = ChoiceBox(), _footballCategory = ChoiceBox();
 private readonly Dictionary<string, TextBox> _bounds = [];
 private bool _sync;
 public SearchCriteria? Result { get; private set; }

 private sealed class ConditionEditor
 {
  public string Key { get; init; } = "";
  public ComboBox Operator { get; } = Operators();
  public TextBox First { get; } = Input();
  public TextBox Second { get; } = Input();
  public FrameworkElement Row { get; set; } = null!;
 }
 private sealed class AttributePage
 {
  public List<string> Keys { get; } = [];
  public List<string> CoreKeys { get; } = [];
  public ScrollViewer Scroll { get; } = new() { VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
  public ComboBox Operator { get; } = Operators();
  public TextBox First { get; } = Input("15");
  public TextBox Second { get; } = Input("20");
  public TextBlock Count { get; } = Label("0개 선택", 13, true);
  public Button Apply { get; } = Button("선택한 0개에 적용");
 }

 public SearchCriteriaWindow(string kind, List<EditableField> catalog, List<SearchChoice> roles, List<SearchChoice> groups, EntityRecord? ownClub, SearchCriteria initial)
 {
  _kind = kind; _original = ModelCopy(initial);
  NameScope.SetNameScope(this, new NameScope());
  _catalog = catalog.Where(field => !string.IsNullOrWhiteSpace(field.Key) && double.IsFinite(field.Min) && double.IsFinite(field.Max) && field.Min <= field.Max).DistinctBy(field => field.Key).ToList();
  _fields = _catalog.ToDictionary(field => field.Key);
  Title = kind == "Staff" ? "스태프 검색 조건" : "검색 조건";
  Width = 1000; Height = 740; MinWidth = 820; MinHeight = 620;
  WindowStartupLocation = WindowStartupLocation.CenterOwner; ShowInTaskbar = false;
  ResizeMode = ResizeMode.CanResize; WindowStyle = WindowStyle.None;
  Background = Brush("BaseBrush"); Foreground = Brush("InkBrush"); FontSize = 13;
  Typography.Initialize(this);
  var frame = new Border { BorderBrush = Brush("BorderBrush"), BorderThickness = new Thickness(1), Background = Background };
  var root = new Grid(); root.RowDefinitions.Add(new() { Height = new GridLength(54) }); root.RowDefinitions.Add(new()); root.RowDefinitions.Add(new() { Height = GridLength.Auto }); root.RowDefinitions.Add(new() { Height = new GridLength(62) }); frame.Child = root; Content = frame;
  var heading = new DockPanel { Margin = new Thickness(20, 0, 12, 0) };
  var close = Button("×"); close.Width = 36; close.FontSize = 22; close.Padding = new Thickness(0); close.ToolTip = "취소하고 닫기"; close.Click += (_, _) => Close(); DockPanel.SetDock(close, Dock.Right); heading.Children.Add(close);
  heading.Children.Add(Label(Title, 20, true)); heading.MouseLeftButtonDown += (_, args) => { if (args.OriginalSource is TextBlock && args.LeftButton == MouseButtonState.Pressed) DragMove(); }; root.Children.Add(heading);
  Grid.SetRow(_tabs, 1); _tabs.Margin = new Thickness(18, 0, 18, 0); root.Children.Add(_tabs); RegisterName(_tabs.Name, _tabs);
  _tabs.Items.Add(new TabItem { Header = "검색 조건", Content = BuildStandardPage(roles, groups, ownClub), Padding = new Thickness(20, 9, 20, 9), FontWeight = FontWeights.Bold });
  _tabs.Items.Add(new TabItem { Header = "능력치", Content = BuildAttributePage(false), Padding = new Thickness(20, 9, 20, 9), FontWeight = FontWeights.Bold });
  _tabs.Items.Add(new TabItem { Header = "직무 점수", Content = BuildAttributePage(true), Padding = new Thickness(20, 9, 20, 9), FontWeight = FontWeights.Bold });
  _error.Foreground = new SolidColorBrush(Color.FromRgb(245, 160, 160)); _error.Margin = new Thickness(22, 8, 22, 0); _error.Visibility = Visibility.Collapsed; Grid.SetRow(_error, 2); root.Children.Add(_error);
  var footer = new DockPanel { Margin = new Thickness(20, 10, 20, 12) }; Grid.SetRow(footer, 3); root.Children.Add(footer);
  var buttons = new StackPanel { Orientation = Orientation.Horizontal }; DockPanel.SetDock(buttons, Dock.Right); footer.Children.Add(buttons);
  var cancel = Button("취소"); cancel.Name = "CriteriaCancelButton"; cancel.IsCancel = true; cancel.MinWidth = 92; cancel.Click += (_, _) => Close(); buttons.Children.Add(cancel); RegisterName(cancel.Name, cancel);
  var apply = Button("조건 적용"); apply.Name = "CriteriaApplyButton"; apply.MinWidth = 115; apply.Margin = new Thickness(8, 0, 0, 0); apply.Style = TryFindResource("PrimaryButton") as Style; apply.FontWeight = FontWeights.Bold; apply.Click += (_, _) => Commit(); buttons.Children.Add(apply); RegisterName(apply.Name, apply);
  footer.Children.Add(Label("선택한 조건을 모두 만족하는 대상을 검색합니다.", 12));
  PreviewKeyDown += (_, args) => { if (args.Key == Key.Escape) { args.Handled = true; Close(); } };
  LoadDraft(_original);
 }

 public static SearchCriteria ModelCopy(SearchCriteria criteria) => criteria.Clone();
 private static Brush Brush(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Label(string text, double size = 12, bool bold = false) => new() { Text = text, FontSize = size, FontWeight = bold ? FontWeights.Bold : FontWeights.Medium, Foreground = Brush(bold ? "InkBrush" : "MutedBrush"), TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
 private static TextBox Input(string text = "") => new() { Text = text, MinHeight = 30, Padding = new Thickness(7, 4, 7, 4), FontWeight = FontWeights.Medium, VerticalContentAlignment = VerticalAlignment.Center };
 private static Button Button(string text) => new() { Content = text, Padding = new Thickness(10, 6, 10, 6), FontWeight = FontWeights.Medium, VerticalAlignment = VerticalAlignment.Center };
 private static ComboBox ChoiceBox() => new() { DisplayMemberPath = nameof(SearchChoice.Label), SelectedValuePath = nameof(SearchChoice.Key), MinHeight = 30, FontWeight = FontWeights.Medium, MaxDropDownHeight = 320 };
 private static ComboBox Operators()
 {
  var box = ChoiceBox(); box.ItemsSource = new[] { new SearchChoice("min", "이상 ≥"), new SearchChoice("max", "이하 ≤"), new SearchChoice("range", "범위") }; box.SelectedValue = "min"; return box;
 }
 private static string Selected(ComboBox box, string fallback = "") => box.SelectedValue as string ?? fallback;
 private static FrameworkElement Labeled(string text, FrameworkElement input)
 {
  var panel = new StackPanel { Margin = new Thickness(0, 0, 14, 8) }; var caption = Label(text); caption.Margin = new Thickness(0, 0, 0, 4); panel.Children.Add(caption); panel.Children.Add(input); return panel;
 }
 private static void Populate(ComboBox box, IEnumerable<SearchChoice> choices, string selected, string all = "전체")
 {
  var list = choices.DistinctBy(choice => choice.Key).ToList(); if (!list.Any(choice => choice.Key == "")) list.Insert(0, new("", all));
  if (!list.Any(choice => choice.Key == selected)) list.Add(new(selected, selected)); box.ItemsSource = list; box.SelectedValue = selected;
 }
 private FrameworkElement BuildStandardPage(List<SearchChoice> roles, List<SearchChoice> groups, EntityRecord? ownClub)
 {
  var page = new Grid { Margin = new Thickness(2, 14, 2, 0) }; page.RowDefinitions.Add(new() { Height = GridLength.Auto }); page.RowDefinitions.Add(new() { Height = GridLength.Auto }); page.RowDefinitions.Add(new());
  var basic = new Grid(); basic.ColumnDefinitions.Add(new()); basic.ColumnDefinitions.Add(new());
  for (int i = 0; i < 5; i++) basic.RowDefinitions.Add(new() { Height = GridLength.Auto });
  var scope = new List<SearchChoice> { new("all", "전체 데이터베이스") }; if (ownClub != null) scope.Add(new("own", "우리 구단 · " + ownClub.DisplayName));
  _scope.ItemsSource = scope; _scope.SelectedValue = scope.Any(choice => choice.Key == _original.Scope) ? _original.Scope : "all";
  _role.Name = "CriteriaRole"; RegisterName(_role.Name, _role);
  _role.ItemsSource = SearchRoleChoices.Build(_kind, roles, _original.Role); _role.SelectedValue = _original.Role;
  Populate(_group, groups, _original.Group, "모든 선수단");
  _employment.ItemsSource = new[] { new SearchChoice("all", "전체"), new SearchChoice("unemployed", "무소속"), new SearchChoice("employed", "소속 있음") }; _employment.SelectedValue = "all";
  _footballCategory.Name = "CriteriaFootballCategory"; RegisterName(_footballCategory.Name, _footballCategory); _footballCategory.ItemsSource = FootballSearchCategories.Choices; _footballCategory.SelectedValue = ""; _footballCategory.ToolTip = FootballSearchCategories.Hint;
  foreach (var (label, input, row, col) in new (string, FrameworkElement, int, int)[] { ("이름 · 영문 이름 · UID", _name, 0, 0), ("소속 구단 이름", _club, 0, 1), ("검색 범위", _scope, 1, 0), ("현재 직무 / 포지션", _role, 1, 1), ("고용 상태", _employment, 2, 0), ("선수단 구분", _group, 2, 1) })
  { var control = Labeled(label, input); Grid.SetRow(control, row); Grid.SetColumn(control, col); basic.Children.Add(control); if (input == _group && _kind != "Player") control.Visibility = Visibility.Collapsed; }
  var football = Labeled("축구 구분", _footballCategory); Grid.SetRow(football, 3); basic.Children.Add(football); football.Visibility = FootballSearchCategories.Supports(_kind) ? Visibility.Visible : Visibility.Collapsed;
  var footballHint = Label("미확인 대상은 전체에 포함됩니다.\n스태프는 소속 구단 기준입니다.", 11); footballHint.Margin = new Thickness(0, 4, 14, 8); footballHint.ToolTip = FootballSearchCategories.Hint; Grid.SetRow(footballHint, 3); Grid.SetColumn(footballHint, 1); basic.Children.Add(footballHint); footballHint.Visibility = football.Visibility;
  var ranges = new UniformGrid { Columns = 3 };
  ranges.Children.Add(Bounds("나이", "age")); ranges.Children.Add(Bounds("현재 능력 (CA)", "ca")); ranges.Children.Add(Bounds("잠재 능력 (PA)", "pa")); Grid.SetRow(ranges, 4); Grid.SetColumnSpan(ranges, 2); basic.Children.Add(ranges);
  bool person = _kind is "Player" or "Staff" or "Person"; ranges.Visibility = person ? Visibility.Visible : Visibility.Collapsed; _scope.IsEnabled = person; _employment.IsEnabled = person;
  page.Children.Add(basic);
  var selected = new DockPanel { Margin = new Thickness(0, 5, 0, 8) }; var clear = Button("숫자 조건 지우기"); clear.Click += (_, _) => { foreach (string key in _conditions.Keys.ToArray()) RemoveCondition(key); }; DockPanel.SetDock(clear, Dock.Right); selected.Children.Add(clear); selected.Children.Add(_conditionCount); Grid.SetRow(selected, 1); page.Children.Add(selected);
  var scroll = new ScrollViewer { Content = _conditionRows, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled }; Grid.SetRow(scroll, 2); page.Children.Add(scroll); return page;
 }
 private FrameworkElement Bounds(string title, string key)
 {
  var grid = new Grid(); grid.ColumnDefinitions.Add(new()); grid.ColumnDefinitions.Add(new() { Width = new GridLength(18) }); grid.ColumnDefinitions.Add(new()); var min = Input(); var max = Input(); min.ToolTip = "최소 · 비우면 제한 없음"; max.ToolTip = "최대 · 비우면 제한 없음"; _bounds[key + "Min"] = min; _bounds[key + "Max"] = max; grid.Children.Add(min); var dash = Label("–"); dash.HorizontalAlignment = HorizontalAlignment.Center; Grid.SetColumn(dash, 1); grid.Children.Add(dash); Grid.SetColumn(max, 2); grid.Children.Add(max); return Labeled(title + " · 최소 / 최대", grid);
 }
 private FrameworkElement BuildAttributePage(bool scores)
 {
  var state = new AttributePage(); _pages.Add(state); state.Second.Text = scores ? "300" : "20";
  var page = new Grid { Margin = new Thickness(2, 14, 2, 0) }; page.RowDefinitions.Add(new() { Height = GridLength.Auto }); page.RowDefinitions.Add(new()); page.RowDefinitions.Add(new() { Height = GridLength.Auto });
  var heading = new DockPanel { Margin = new Thickness(0, 0, 0, 10) }; var actions = new StackPanel { Orientation = Orientation.Horizontal };
  var all = Button("전체 선택"); all.Click += (_, _) => { foreach (string key in state.Keys) _checks[key].IsChecked = true; }; actions.Children.Add(all);
  var none = Button("선택 해제"); none.Margin = new Thickness(6, 0, 0, 0); none.Click += (_, _) => { foreach (string key in state.Keys) _checks[key].IsChecked = false; }; actions.Children.Add(none); DockPanel.SetDock(actions, Dock.Right); heading.Children.Add(actions);
  var headingText = new StackPanel(); headingText.Children.Add(Label(scores ? "직무 점수" : "능력치 선택", 16, true)); headingText.Children.Add(Label(scores ? "직무 점수는 검증된 능력치로 계산한 사용자 기준 / 300입니다." : "체크한 항목만 검색 조건에 포함됩니다. 조건별 값은 검색 조건 탭에서 조정하세요.")); heading.Children.Add(headingText); page.Children.Add(heading);
  var body = new DockPanel();
  if (!scores && _kind == "Staff") { var presets = Presets(); DockPanel.SetDock(presets, Dock.Top); body.Children.Add(presets); }
  var available = _catalog.Where(field => field.Key.StartsWith("score_", StringComparison.Ordinal) == scores).ToArray();
  var content = new StackPanel();
  if (!scores && _kind == "Staff")
  {
   var core = available.Where(field => field.Key.StartsWith("staff_", StringComparison.Ordinal)).ToArray(); state.CoreKeys.AddRange(core.Select(field => field.Key));
   string[] mental = ["staff_discipline", "staff_youth", "staff_determination", "staff_management", "staff_motivation", "staff_knowledge"];
   string[] assessment = ["staff_judging_ability", "staff_judging_potential", "staff_physiotherapy", "staff_analysis", "staff_sports_science", "staff_negotiation", "staff_judging_staff"];
   content.Children.Add(CheckColumns(state, [
    ("훈련", core.Where(field => !mental.Contains(field.Key) && !assessment.Contains(field.Key)).ToArray()),
    ("정신 · 관리", core.Where(field => mental.Contains(field.Key)).ToArray()),
    ("평가 · 의료", core.Where(field => assessment.Contains(field.Key)).ToArray())]));
   var ancillary = available.Where(field => !state.CoreKeys.Contains(field.Key)).ToArray();
   if (ancillary.Length > 0) content.Children.Add(new Expander { Header = $"추가 조건 · 성격 / 명성 / 계약 등 {ancillary.Length}개", Margin = new Thickness(0, 9, 0, 0), IsExpanded = false, FontWeight = FontWeights.Medium, Content = GroupedChecks(state, ancillary) });
  }
  else content.Children.Add(GroupedChecks(state, available));
  if (state.Keys.Count == 0) content.Children.Add(Label(scores ? "이 종류에는 직무 점수가 없습니다." : "이 자료에는 선택할 수 있는 상세 능력치가 없습니다."));
  state.Scroll.Content = content; body.Children.Add(state.Scroll); Grid.SetRow(body, 1); page.Children.Add(body);
  var common = new Grid { Margin = new Thickness(0, 12, 0, 4), Background = Brush("PanelBrush") }; common.ColumnDefinitions.Add(new()); common.ColumnDefinitions.Add(new() { Width = new GridLength(110) }); common.ColumnDefinitions.Add(new() { Width = new GridLength(108) }); common.ColumnDefinitions.Add(new() { Width = new GridLength(108) }); common.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
  state.Count.Margin = new Thickness(12, 10, 12, 10); common.Children.Add(state.Count); state.Operator.Margin = new Thickness(0, 8, 8, 8); Grid.SetColumn(state.Operator, 1); common.Children.Add(state.Operator);
  var first = Spinner(state.First); first.Margin = new Thickness(0, 8, 8, 8); Grid.SetColumn(first, 2); common.Children.Add(first); var second = Spinner(state.Second); second.Margin = new Thickness(0, 8, 8, 8); second.Visibility = Visibility.Hidden; Grid.SetColumn(second, 3); common.Children.Add(second);
  state.Operator.SelectionChanged += (_, _) => second.Visibility = Selected(state.Operator) == "range" ? Visibility.Visible : Visibility.Hidden; state.Apply.Margin = new Thickness(0, 8, 8, 8); state.Apply.Click += (_, _) => ApplyCommon(state); Grid.SetColumn(state.Apply, 4); common.Children.Add(state.Apply); Grid.SetRow(common, 2); page.Children.Add(common); return page;
 }
 private FrameworkElement GroupedChecks(AttributePage state, EditableField[] fields)
 {
  var groups = fields.GroupBy(field => string.IsNullOrWhiteSpace(field.Section) ? "기타 능력" : field.Section).SelectMany(group => group.Chunk(9).Select(chunk => (group.Key, chunk))).ToArray();
  return CheckColumns(state, groups);
 }
 private FrameworkElement CheckColumns(AttributePage state, (string Label, EditableField[] Fields)[] groups)
 {
  var columns = new Grid(); var stacks = Enumerable.Range(0, 3).Select(_ => new StackPanel { Margin = new Thickness(0, 0, 14, 0) }).ToArray(); var heights = new int[3];
  for (int index = 0; index < 3; index++) { columns.ColumnDefinitions.Add(new()); Grid.SetColumn(stacks[index], index); columns.Children.Add(stacks[index]); }
  foreach (var group in groups)
  {
   int column = Array.IndexOf(heights, heights.Min()); var title = Label(group.Label, 13, true); title.Margin = new Thickness(0, 4, 0, 5); stacks[column].Children.Add(title); heights[column] += 2;
   foreach (var field in group.Fields)
   {
    var text = Label(field.Label, 13); text.Foreground = Brush("InkBrush"); text.TextWrapping = TextWrapping.NoWrap; text.TextTrimming = TextTrimming.CharacterEllipsis;
    var check = new CheckBox { Content = text, HorizontalContentAlignment = HorizontalAlignment.Stretch, Padding = new Thickness(0, 3, 0, 3), MinHeight = 24, FontWeight = FontWeights.Medium, Foreground = Brush("InkBrush"), ToolTip = $"{field.Label} · {field.Min:G}–{field.Max:G}" };
    _checks.Add(field.Key, check); state.Keys.Add(field.Key); check.Checked += (_, _) => { if (!_sync) AddCondition(field.Key, Math.Clamp(15, field.Min, field.Max), null); }; check.Unchecked += (_, _) => { if (!_sync) RemoveCondition(field.Key); }; stacks[column].Children.Add(check); heights[column]++;
   }
  }
  return columns;
 }
 private FrameworkElement Presets()
 {
  var panel = new WrapPanel { Margin = new Thickness(0, 0, 0, 8) }; var title = Label("빠른 능력 기준", 12, true); title.Margin = new Thickness(0, 0, 10, 0); panel.Children.Add(title);
  foreach (var (name, keys, minimum) in new (string, string[], double)[] { ("유소년", ["staff_youth"], 15), ("수석 코치", ["staff_management", "staff_knowledge"], 14), ("스카우트", ["staff_judging_ability", "staff_judging_potential"], 15), ("치료", ["staff_physiotherapy"], 15), ("스포츠 과학", ["staff_sports_science"], 15) })
  {
   var button = Button(name); button.Margin = new Thickness(0, 0, 6, 4); button.IsEnabled = keys.All(key => _fields.ContainsKey(key)); button.Click += (_, _) => { foreach (string key in keys) AddCondition(key, minimum, null); ClearError(); }; panel.Children.Add(button);
  }
  return panel;
 }
 private static FrameworkElement Spinner(TextBox input)
 {
  var grid = new Grid(); grid.ColumnDefinitions.Add(new() { Width = new GridLength(24) }); grid.ColumnDefinitions.Add(new()); grid.ColumnDefinitions.Add(new() { Width = new GridLength(24) });
  foreach (int delta in new[] { -1, 1 }) { var button = Button(delta < 0 ? "−" : "+"); button.Padding = new Thickness(0); button.MinHeight = 30; button.Click += (_, _) => { if (double.TryParse(input.Text, NumberStyles.Float, CultureInfo.InvariantCulture, out double value) && double.IsFinite(value)) input.Text = (value + delta).ToString("0.########", CultureInfo.InvariantCulture); }; Grid.SetColumn(button, delta < 0 ? 0 : 2); grid.Children.Add(button); }
  Grid.SetColumn(input, 1); input.Padding = new Thickness(3, 4, 3, 4); input.HorizontalContentAlignment = HorizontalAlignment.Center; grid.Children.Add(input); return grid;
 }
 private void AddCondition(string key, double? min, double? max)
 {
  if (!_conditions.TryGetValue(key, out var editor))
  {
   editor = new() { Key = key }; _conditions.Add(key, editor);
   var row = new Grid { Margin = new Thickness(0, 0, 4, 4), Background = Brush("PanelBrush") }; row.ColumnDefinitions.Add(new()); row.ColumnDefinitions.Add(new() { Width = new GridLength(110) }); row.ColumnDefinitions.Add(new() { Width = new GridLength(100) }); row.ColumnDefinitions.Add(new() { Width = new GridLength(100) }); row.ColumnDefinitions.Add(new() { Width = new GridLength(42) });
   var label = Label(_fields.TryGetValue(key, out var field) ? field.Label : key, 13, true); label.Margin = new Thickness(10, 5, 8, 5); label.ToolTip = field == null ? "현재 자료에서 지원하지 않는 조건입니다. 제거 후 다시 선택하세요." : $"{field.Section} · {field.Min:G}–{field.Max:G}"; row.Children.Add(label);
   editor.Operator.Margin = new Thickness(0, 4, 8, 4); Grid.SetColumn(editor.Operator, 1); row.Children.Add(editor.Operator); editor.First.Margin = new Thickness(0, 4, 8, 4); Grid.SetColumn(editor.First, 2); row.Children.Add(editor.First); editor.Second.Margin = new Thickness(0, 4, 8, 4); Grid.SetColumn(editor.Second, 3); row.Children.Add(editor.Second);
   var remove = Button("×"); remove.Padding = new Thickness(6, 3, 6, 3); remove.Margin = new Thickness(0, 4, 6, 4); remove.ToolTip = "이 조건 제거"; remove.Click += (_, _) => RemoveCondition(key); Grid.SetColumn(remove, 4); row.Children.Add(remove); editor.Row = row;
   editor.Operator.SelectionChanged += (_, _) => { editor.Second.Visibility = Selected(editor.Operator) == "range" ? Visibility.Visible : Visibility.Hidden; ClearError(); };
  }
  editor.Operator.SelectedValue = min.HasValue && max.HasValue ? "range" : max.HasValue ? "max" : "min"; editor.First.Text = Format(min ?? max); editor.Second.Text = Format(max); editor.Second.Visibility = min.HasValue && max.HasValue ? Visibility.Visible : Visibility.Hidden;
  _sync = true; if (_checks.TryGetValue(key, out var check)) check.IsChecked = true; _sync = false; RefreshConditions();
 }
 private void RemoveCondition(string key)
 {
  _conditions.Remove(key); _sync = true; if (_checks.TryGetValue(key, out var check)) check.IsChecked = false; _sync = false; RefreshConditions(); ClearError();
 }
 private void RefreshConditions()
 {
  _conditionRows.Children.Clear(); foreach (var editor in _conditions.Values) _conditionRows.Children.Add(editor.Row); if (_conditions.Count == 0) _conditionRows.Children.Add(_empty); _conditionCount.Text = $"선택한 숫자 조건 · {_conditions.Count}개";
  foreach (var page in _pages) { int count = page.Keys.Count(key => _conditions.ContainsKey(key)); page.Count.Text = $"{count}개 선택"; page.Apply.Content = $"선택한 {count}개에 적용"; page.Apply.IsEnabled = count > 0; }
 }
 private void ApplyCommon(AttributePage page)
 {
  try
  {
   var keys = page.Keys.Where(key => _conditions.ContainsKey(key)).ToArray(); var values = keys.Select(key => ReadCondition(key, Selected(page.Operator), page.First.Text, page.Second.Text)).ToArray();
   foreach (var condition in values) AddCondition(condition.Key, condition.Min, condition.Max); ClearError();
  }
  catch (InvalidOperationException error) { ShowError(error.Message); }
 }
 private static string Format(double? value) => value?.ToString("G17", CultureInfo.InvariantCulture) ?? "";
 private static double Parse(string text, string label, double minimum, double maximum, bool whole = false)
 {
  if (!double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out double value) || !double.IsFinite(value) || value < minimum || value > maximum || whole && value != Math.Truncate(value)) throw new InvalidOperationException($"{label}: {minimum:G}–{maximum:G} 범위의 {(whole ? "정수" : "숫자")}를 입력하세요."); return value;
 }
 private SearchNumericCondition ReadCondition(string key, string comparison, string first, string second)
 {
  if (!_fields.TryGetValue(key, out var field)) throw new InvalidOperationException($"현재 자료에서 지원하지 않는 조건입니다: {key}. 조건을 제거하고 다시 선택하세요.");
  double value = Parse(first, field.Label, field.Min, field.Max); double? low = comparison == "max" ? null : value; double? high = comparison == "max" ? value : comparison == "range" ? Parse(second, field.Label + " 최대", field.Min, field.Max) : null;
  if (low > high) throw new InvalidOperationException(field.Label + ": 최소값이 최대값보다 큽니다."); return new(key, low, high);
 }
 private void LoadDraft(SearchCriteria draft)
 {
  _name.Text = draft.Name; _club.Text = draft.Club; _scope.SelectedValue = draft.Scope; if (_scope.SelectedIndex < 0) _scope.SelectedValue = "all"; _role.SelectedValue = draft.Role; _employment.SelectedValue = draft.Employment; if (_employment.SelectedIndex < 0) _employment.SelectedValue = "all"; _group.SelectedValue = draft.Group;
  _footballCategory.SelectedValue = draft.FootballCategory ?? "";
  foreach (var (key, value) in new[] { ("ageMin", draft.AgeMin), ("ageMax", draft.AgeMax), ("caMin", draft.CaMin), ("caMax", draft.CaMax), ("paMin", draft.PaMin), ("paMax", draft.PaMax) }) _bounds[key].Text = value;
  foreach (string key in _conditions.Keys.ToArray()) RemoveCondition(key); foreach (var condition in draft.Conditions) AddCondition(condition.Key, condition.Min, condition.Max); RefreshConditions(); ClearError();
 }
 public SearchCriteria Snapshot()
 {
  if (_conditions.Count > 64) throw new InvalidOperationException("숫자 조건은 최대 64개입니다. 선택한 조건을 줄여주세요.");
  string category = Selected(_footballCategory);
  var result = new SearchCriteria { Name = _name.Text.Trim(), Club = _club.Text.Trim(), Scope = Selected(_scope, "all"), FootballCategory = FootballSearchCategories.Supports(_kind) && category.Length > 0 ? category : null, Role = Selected(_role), Employment = Selected(_employment, "all"), Group = _kind == "Player" ? Selected(_group) : "", AgeMin = _bounds["ageMin"].Text.Trim(), AgeMax = _bounds["ageMax"].Text.Trim(), CaMin = _bounds["caMin"].Text.Trim(), CaMax = _bounds["caMax"].Text.Trim(), PaMin = _bounds["paMin"].Text.Trim(), PaMax = _bounds["paMax"].Text.Trim() };
  if (!FootballSearchCategories.IsValid(result.FootballCategory)) throw new InvalidOperationException("축구 구분을 확인하세요.");
  if (result.Name.Length > 200 || result.Club.Length > 200) throw new InvalidOperationException("이름과 구단 검색어는 각각 200자 이내로 입력하세요.");
  foreach (var (key, label, limit) in new[] { ("age", "나이", 129d), ("ca", "현재 능력", 200d), ("pa", "잠재 능력", 200d) })
  { string lowText = _bounds[key + "Min"].Text.Trim(), highText = _bounds[key + "Max"].Text.Trim(); double? low = lowText.Length == 0 ? null : Parse(lowText, label + " 최소", 0, limit, true); double? high = highText.Length == 0 ? null : Parse(highText, label + " 최대", 0, limit, true); if (low > high) throw new InvalidOperationException(label + ": 최소값이 최대값보다 큽니다."); }
  result.Conditions = _conditions.Values.Select(editor => ReadCondition(editor.Key, Selected(editor.Operator), editor.First.Text, editor.Second.Text)).ToList(); return result;
 }
 private void Commit()
 {
  try { var result = Snapshot(); Result = result; DialogResult = true; }
  catch (InvalidOperationException error) { Result = null; ShowError(error.Message); }
 }
 private void ShowError(string text) { _error.Text = text; _error.Visibility = Visibility.Visible; }
 private void ClearError() { _error.Text = ""; _error.Visibility = Visibility.Collapsed; }

 /// <summary>Checks the actual position picker locally without changing the committed draft.</summary>
 public object QaRoleChoices()
 {
  var choices = _role.Items.Cast<SearchChoice>().ToArray();
  if (choices.Length == 0 || choices[0].Key != "" || choices.Select(choice => choice.Key).Distinct(StringComparer.Ordinal).Count() != choices.Length)
   throw new InvalidOperationException("직무 / 포지션 선택기의 전체 항목 또는 중복 키가 잘못되었습니다.");
  if (_kind == "Player")
  {
   var ranks = choices.Skip(1).Select(choice => PositionOrder.Rank(choice.Key)).ToArray();
   if (!ranks.SequenceEqual(ranks.Order())) throw new InvalidOperationException("검색 조건의 포지션 순서가 GK → ST 순서와 다릅니다.");
  }
  else if (!choices.Skip(1).Select(choice => choice.Key).SequenceEqual(choices.Skip(1).Select(choice => choice.Key).Order(StringComparer.CurrentCultureIgnoreCase)))
   throw new InvalidOperationException("스태프 직무 선택기의 가나다순이 잘못되었습니다.");
  string selected = Selected(_role);
  if (selected != _original.Role) throw new InvalidOperationException("포지션 선택기에서 기존 검색 조건을 잃었습니다.");
  try
  {
   foreach (var choice in choices)
   {
    _role.SelectedValue = choice.Key;
    if (Selected(_role) != choice.Key || Snapshot().Role != choice.Key)
     throw new InvalidOperationException("포지션 선택기의 값이 실제 검색 조건과 다릅니다.");
   }
  }
  finally { _role.SelectedValue = selected; }
  return new { kind = _kind, keys = choices.Select(choice => choice.Key).ToArray(), selected, firstIsAll = true, uniqueKeys = true, selectionPreserved = true, everyChoiceRoundTrips = true, uncommitted = Result == null };
 }

 /// <summary>Exercises actual checkbox/common-value controls locally, then restores the draft. Does not apply or close.</summary>
 public async Task<object> QaAsync(string outputDir)
 {
  if (!IsVisible) throw new InvalidOperationException("조건 창을 표시한 뒤 QA를 실행하세요.");
  Directory.CreateDirectory(outputDir); var before = Snapshot(); int originalTab = _tabs.SelectedIndex; string originalProof = JsonSerializer.Serialize(_original); double originalWidth = Width, originalHeight = Height;
  var sample = _catalog.Where(field => !field.Key.StartsWith("score_") && field.Min <= 15 && field.Max >= 15)
   .OrderByDescending(field => _kind == "Staff" && (field.Key is "staff_youth" or "staff_determination")).Take(2).ToArray();
  try
  {
   foreach (string key in _conditions.Keys.ToArray()) RemoveCondition(key);
   foreach (var field in sample) _checks[field.Key].IsChecked = true;
   var page = _pages[0]; page.Operator.SelectedValue = "min"; page.First.Text = "15"; ApplyCommon(page);
   var selected = Snapshot(); if (sample.Length > 0 && (selected.Conditions.Count != sample.Length || selected.Conditions.Any(condition => condition.Min != 15 || condition.Max != null))) throw new InvalidOperationException("체크박스 / 공통 최소값 검증 실패");
   var copy = ModelCopy(selected); if (copy.Conditions.Count > 0) copy.Conditions[0].Min = 16; if (selected.Conditions.Any(condition => condition.Min != 15)) throw new InvalidOperationException("검색 조건 복사본이 원본을 변경했습니다.");
   var validations = new List<string>();
   void Reject(Action change, string name)
   {
    change(); bool rejected = false; try { Snapshot(); } catch (InvalidOperationException) { rejected = true; }
    if (!rejected) throw new InvalidOperationException("잘못된 검색 조건을 허용했습니다: " + name); validations.Add(name); LoadDraft(selected);
   }
   if (sample.Length > 0)
   {
    string key = sample[0].Key; var field = sample[0];
    Reject(() => { var editor = _conditions[key]; editor.Operator.SelectedValue = "range"; editor.First.Text = Format(field.Max); editor.Second.Text = Format(field.Min); }, "minimum_greater_than_maximum");
    Reject(() => _conditions[key].First.Text = Format(field.Max + 1), "outside_field_range");
    Reject(() => _conditions[key].First.Text = "NaN", "non_finite_number");
   }
   Reject(() => _bounds["ageMin"].Text = "130", "age_outside_range");
   if (_catalog.Count >= 65) Reject(() => { foreach (var field in _catalog.Take(65)) AddCondition(field.Key, Math.Clamp(15, field.Min, field.Max), null); }, "maximum_64_conditions");
   var reports = new List<object>();
   for (int tab = 0; tab < _tabs.Items.Count; tab++) { _tabs.SelectedIndex = tab; await SettleAsync(); var path = Path.Combine(outputDir, $"criteria-{tab}.png"); Capture(path); reports.Add(new { tab, path, width = ActualWidth, height = ActualHeight }); }
   Width = 820; Height = 620; _tabs.SelectedIndex = 1; await SettleAsync(); page.Scroll.ScrollToTop(); await SettleAsync();
   var visibleCore = page.CoreKeys.Count(key => { var check = _checks[key]; var bounds = check.TransformToAncestor(page.Scroll).TransformBounds(new Rect(check.RenderSize)); return check.IsVisible && bounds.Top >= -1 && bounds.Bottom <= page.Scroll.ActualHeight + 1 && bounds.Left >= -1 && bounds.Right <= page.Scroll.ActualWidth + 1; });
   if (visibleCore != page.CoreKeys.Count) throw new InvalidOperationException($"최소 창에서 스태프 핵심 능력치가 잘립니다: {visibleCore} / {page.CoreKeys.Count}");
   string minimumPath = Path.Combine(outputDir, "criteria-attributes-820x620.png"); Capture(minimumPath); reports.Add(new { tab = 1, path = minimumPath, width = ActualWidth, height = ActualHeight, visibleCore });
   _tabs.SelectedIndex = 0; await SettleAsync(); Capture(Path.Combine(outputDir, "criteria-conditions-820x620.png"));
   if (JsonSerializer.Serialize(_original) != originalProof || Result != null) throw new InvalidOperationException("검증 중 원본 조건이 변경되었습니다.");
   return new { success = true, localOnly = true, checkboxCount = _checks.Count, coreStaffAttributes = page.CoreKeys.Count, coreStaffAttributesVisibleAtMinimum = visibleCore, scoreCount = _pages[1].Keys.Count, selected = selected.Conditions.Count, commonThreshold = 15, modelCopyIndependent = true, uncommitted = Result == null, validations, reports };
  }
  finally { LoadDraft(before); _tabs.SelectedIndex = originalTab; Width = originalWidth; Height = originalHeight; }
 }
 private async Task SettleAsync() { await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle); await Task.Delay(40); UpdateLayout(); }
 private void Capture(string path)
 {
  if (Content is not FrameworkElement content || content.ActualWidth <= 0 || content.ActualHeight <= 0) throw new InvalidOperationException("조건 창 크기를 확인할 수 없습니다.");
  var bitmap = new RenderTargetBitmap((int)Math.Ceiling(content.ActualWidth), (int)Math.Ceiling(content.ActualHeight), 96, 96, PixelFormats.Pbgra32); bitmap.Render(content); var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(path); encoder.Save(stream);
 }
}
