using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shapes;
using Path = System.IO.Path;

namespace Touchline.Wpf;

public sealed partial class SearchCriteriaWindow
{
 private List<SearchConditionDefinition> _ruleCatalog = [];
 private readonly List<RuleEditor> _ruleEditors = [];
 private readonly StackPanel _ruleRows = new();
 private readonly TextBlock _ruleCount = Label("상세 조건 없음", 14, true), _expression = Label("조건을 추가하면 계산 순서를 표시합니다.", 12);
 private readonly TextBox _conditionSearch = Input();
 private readonly ComboBox _conditionCategory = ChoiceBox();
 private readonly ListBox _conditionList = new() { DisplayMemberPath = nameof(SearchConditionDefinition.Label), BorderThickness = new Thickness(0), Background = Brushes.Transparent, HorizontalContentAlignment = HorizontalAlignment.Stretch };
 private TabControl? _detailBrowserTabs;
 private bool _loadingRules;
 private sealed class RuleEditor(SearchConditionRule rule, SearchConditionDefinition? definition)
 {
  public SearchConditionRule Rule { get; } = rule;
  public SearchConditionDefinition? Definition { get; } = definition;
  public ComboBox? ValueChoice { get; set; }
  public FrameworkElement? Row { get; set; }
  public Button? MoveUp { get; set; }
  public Button? MoveDown { get; set; }
  public Button? Delete { get; set; }
  public CheckBox? EnabledControl { get; set; }
 }

 private FrameworkElement BuildDetailedPage(List<SearchChoice> roles, List<SearchChoice> groups)
 {
  _ruleCatalog = SearchConditionCatalog.Build(_kind, _catalog, roles, groups, _nations).Where(definition => !AbilityVisibility.Current.IsHidden(definition.Key)).ToList();
  var grid = new Grid { Margin = new Thickness(0, 14, 0, 4) }; grid.ColumnDefinitions.Add(new() { Width = new GridLength(224) }); grid.ColumnDefinitions.Add(new() { Width = new GridLength(14) }); grid.ColumnDefinitions.Add(new());
  var browser = new Grid(); browser.RowDefinitions.Add(new() { Height = GridLength.Auto }); browser.RowDefinitions.Add(new() { Height = GridLength.Auto }); browser.RowDefinitions.Add(new()); browser.RowDefinitions.Add(new() { Height = GridLength.Auto });
  _conditionSearch.Name = "ConditionCatalogSearch"; _conditionSearch.ToolTip = "조건 이름 · 분류 · 필드 키로 검색"; _conditionSearch.Margin = new Thickness(0, 0, 0, 7); RegisterName(_conditionSearch.Name, _conditionSearch);
  var search = Labeled("조건 찾기", _conditionSearch); search.Margin = new Thickness(0, 0, 0, 0); browser.Children.Add(search);
  _conditionCategory.ItemsSource = new[] { new SearchChoice("", "모든 분류") }.Concat(_ruleCatalog.Select(d => d.Category).Distinct().Order().Select(c => new SearchChoice(c, c))).ToArray(); _conditionCategory.SelectedValue = ""; _conditionCategory.Margin = new Thickness(0, 0, 0, 8); Grid.SetRow(_conditionCategory, 1); browser.Children.Add(_conditionCategory);
  _conditionList.SelectionChanged += (_, _) => { if (_conditionList.SelectedItem is SearchConditionDefinition field) _conditionList.ToolTip = $"{field.Category} · {field.RangeLabel}\n{field.Key}\n더블 클릭하여 추가"; };
  _conditionList.MouseDoubleClick += (_, args) => { if (_conditionList.SelectedItem is SearchConditionDefinition field) { AddRule(SearchConditionCatalog.NewRule(field)); args.Handled = true; } };
  _conditionList.KeyDown += (_, args) => { if (args.Key == Key.Enter && _conditionList.SelectedItem is SearchConditionDefinition field) { AddRule(SearchConditionCatalog.NewRule(field)); args.Handled = true; } };
  Grid.SetRow(_conditionList, 2); browser.Children.Add(_conditionList);
  var addSelected = Button("선택한 조건 추가  →"); addSelected.Margin = new Thickness(0, 8, 0, 0); addSelected.Click += (_, _) => { if (_conditionList.SelectedItem is SearchConditionDefinition d) AddRule(SearchConditionCatalog.NewRule(d)); }; Grid.SetRow(addSelected, 3); browser.Children.Add(addSelected);
  _conditionSearch.TextChanged += (_, _) => FilterCatalog(); _conditionCategory.SelectionChanged += (_, _) => FilterCatalog(); FilterCatalog();
  if (_kind == "Player") {
   var browserTabs = _detailBrowserTabs = new TabControl { Background = Brushes.Transparent, BorderThickness = new Thickness(0) };
   browserTabs.Items.Add(new TabItem { Header = "조건 목록", Content = browser, Padding = new Thickness(10, 6, 10, 6) });
   browserTabs.Items.Add(new TabItem { Header = "포지션", Content = BuildPositionPitch(), Padding = new Thickness(10, 6, 10, 6) }); grid.Children.Add(browserTabs);
  } else grid.Children.Add(browser);
  var editor = new Grid(); editor.RowDefinitions.Add(new() { Height = GridLength.Auto }); editor.RowDefinitions.Add(new() { Height = GridLength.Auto }); editor.RowDefinitions.Add(new()); editor.RowDefinitions.Add(new() { Height = GridLength.Auto }); Grid.SetColumn(editor, 2); grid.Children.Add(editor);
  var heading = new DockPanel { Margin = new Thickness(0, 0, 0, 10) }; var menu = Button("조건 추가  ▾"); menu.Name = "AddDetailedCondition"; RegisterName(menu.Name, menu); menu.Style = TryFindResource("PrimaryButton") as Style;
  menu.Click += (_, _) => { var context = BuildConditionMenu(); context.PlacementTarget = menu; context.Placement = PlacementMode.Bottom; context.IsOpen = true; }; DockPanel.SetDock(menu, Dock.Right); heading.Children.Add(menu); heading.Children.Add(_ruleCount); editor.Children.Add(heading);
  var help = Label("각 조건을 켜거나 끄고, 비교 방식과 값을 바꾸세요. ↑ ↓로 순서를 바꿀 수 있습니다.", 11); help.Margin = new Thickness(0, 0, 0, 8); Grid.SetRow(help, 1); editor.Children.Add(help);
  var scroll = new ScrollViewer { Content = _ruleRows, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, Padding = new Thickness(0, 0, 5, 0) }; Grid.SetRow(scroll, 2); editor.Children.Add(scroll);
  var bottom = new StackPanel { Margin = new Thickness(0, 10, 0, 0) }; var actions = new WrapPanel();
  var migrate = Button("빠른 능력치를 상세 조건으로 이동"); migrate.FontSize = 11; migrate.Click += (_, _) => {
   try { var values = _conditions.Values.Select(e => ReadCondition(e.Key, Selected(e.Operator), e.First.Text, e.Second.Text)).ToList(); if (_ruleEditors.Count + values.Count > SearchConditionCatalog.Limit) throw new InvalidOperationException("상세 조건은 최대 64개입니다."); foreach (var rule in SearchConditionCatalog.PromoteLegacy(values)) AddRule(rule); foreach (string key in _conditions.Keys.ToArray()) RemoveCondition(key); }
   catch (InvalidOperationException ex) { ShowError(ex.Message); }
  }; actions.Children.Add(migrate);
  var clear = Button("상세 조건 비우기"); clear.FontSize = 11; clear.Margin = new Thickness(6, 0, 0, 0); clear.Click += (_, _) => { _ruleEditors.Clear(); RefreshRuleRows(); }; actions.Children.Add(clear); bottom.Children.Add(actions);
  _expression.Margin = new Thickness(0, 8, 0, 5); _expression.MaxHeight = 54; bottom.Children.Add(_expression); var logic = Label(SearchConditionCatalog.LogicHint, 10); bottom.Children.Add(logic); Grid.SetRow(bottom, 3); editor.Children.Add(bottom);
  return grid;
 }
 private void FilterCatalog()
 {
  string text = _conditionSearch.Text.Trim(), category = Selected(_conditionCategory);
  _conditionList.ItemsSource = _ruleCatalog.Where(d => (category.Length == 0 || d.Category == category) && (text.Length == 0 || $"{d.Label} {d.Category} {d.Key}".Contains(text, StringComparison.CurrentCultureIgnoreCase))).ToArray();
 }
 private ContextMenu BuildConditionMenu()
 {
  var menu = new ContextMenu();
  foreach (var group in _ruleCatalog.GroupBy(d => d.Category)) {
   var category = new MenuItem { Header = group.Key };
   foreach (var definition in group) { var item = new MenuItem { Header = definition.Label, ToolTip = $"{definition.RangeLabel} · {definition.Key}" }; item.Click += (_, _) => AddRule(SearchConditionCatalog.NewRule(definition)); category.Items.Add(item); }
   menu.Items.Add(category);
  }
  return menu;
 }
 private static FrameworkElement BoundedSpinner(TextBox input, double minimum, double maximum, double step)
 {
  var grid = new Grid(); grid.ColumnDefinitions.Add(new() { Width = new GridLength(24) }); grid.ColumnDefinitions.Add(new()); grid.ColumnDefinitions.Add(new() { Width = new GridLength(24) });
  input.HorizontalContentAlignment = HorizontalAlignment.Center; input.Padding = new Thickness(2,3,2,3);
  foreach (int delta in new[] { -1,1 }) { var button = Button(delta < 0 ? "−" : "+"); button.Padding = new Thickness(0); button.MinHeight = 30; button.ToolTip = $"{step:G}씩 조절 ({minimum:G}–{maximum:G})"; button.Click += (_, _) => { double number = double.TryParse(input.Text,NumberStyles.Float,CultureInfo.InvariantCulture,out double parsed) && double.IsFinite(parsed) ? parsed : minimum; input.Text = Math.Clamp(number + delta*step,minimum,maximum).ToString("0.########",CultureInfo.InvariantCulture); }; Grid.SetColumn(button,delta < 0 ? 0 : 2); grid.Children.Add(button); }
  Grid.SetColumn(input,1); grid.Children.Add(input); return grid;
 }
 private void AddRule(SearchConditionRule rule)
 {
  if (_ruleEditors.Count >= SearchConditionCatalog.Limit && !_loadingRules) { ShowError("상세 조건은 최대 64개입니다."); return; }
  if(PositionSearch.IsPosition(rule)&&rule.Positions.Count==0)rule=PositionSearch.NewRule(_ruleCatalog);
  var editor=new RuleEditor(rule.Clone(),_ruleCatalog.FirstOrDefault(d => d.Source == rule.Source && d.Key == rule.Key));_ruleEditors.Add(editor);if(PositionSearch.IsPosition(rule))_activePositionEditor=editor; RefreshRuleRows(); ClearError();
 }
 private void LoadDetailedRules(IEnumerable<SearchConditionRule>? rules)
 {
  _loadingRules = true; _ruleEditors.Clear(); foreach (var rule in rules ?? []) _ruleEditors.Add(new(rule.Clone(),_ruleCatalog.FirstOrDefault(d => d.Source == rule.Source && d.Key == rule.Key))); _loadingRules = false; RefreshRuleRows();
 }
 private List<SearchConditionRule> ReadDetailedRules()
 {
  foreach (var editor in _ruleEditors) if (editor.ValueChoice is { IsEditable: true } choice && editor.Rule.Enabled) {
   var option = editor.Definition?.Options?.Where(o => string.Equals(o.Label,choice.Text.Trim(),StringComparison.CurrentCultureIgnoreCase) || o.Value == choice.Text.Trim()).ToArray();
   if (option?.Length != 1) throw new InvalidOperationException((editor.Definition?.Label ?? editor.Rule.Key) + ": 목록에서 정확한 값을 선택하세요."); editor.Rule.Value = option[0].Value;
  }
  return _ruleEditors.Select(e => e.Rule.Clone()).ToList();
 }
 private void RefreshRuleRows()
 {
  _ruleRows.Children.Clear();
  if (_ruleEditors.Count == 0) { var empty = new StackPanel { Margin = new Thickness(18, 50, 18, 0) }; empty.Children.Add(Label("조건을 조합해 필요한 대상을 찾으세요.",16,true)); var text = Label("왼쪽 목록을 더블 클릭하거나 ‘조건 추가’를 누르세요.\n능력치·신체·정신·계약·직무 점수 등 현재 자료에서 지원하는 항목만 표시합니다.",12); text.Margin = new Thickness(0,10,0,0); empty.Children.Add(text); _ruleRows.Children.Add(empty); }
  for (int i=0;i<_ruleEditors.Count;i++) _ruleRows.Children.Add(BuildRuleRow(_ruleEditors[i],i));
  UpdateExpression();SyncPositionPitch();
 }
 private FrameworkElement BuildRuleRow(RuleEditor editor,int index)
 {
  var rule=editor.Rule;var d=editor.Definition; var card=new Border { Background=Brush("PanelBrush"),BorderBrush=Brush("BorderBrush"),BorderThickness=new Thickness(1),CornerRadius=new CornerRadius(5),Padding=new Thickness(9,7,9,7),Margin=new Thickness(0,0,0,7) }; var grid=new Grid();grid.RowDefinitions.Add(new(){Height=GridLength.Auto});grid.RowDefinitions.Add(new(){Height=GridLength.Auto});card.Child=grid;editor.Row=card;
  var heading=new DockPanel();var actions=new StackPanel{Orientation=Orientation.Horizontal};DockPanel.SetDock(actions,Dock.Right);heading.Children.Add(actions);
  foreach(var (label,delta) in new[]{("↑",-1),("↓",1),("×",0)}) {var button=Button(label);button.Padding=new Thickness(2,0,2,0);button.Width=25;button.Height=24;button.Margin=new Thickness(3,0,0,0);button.ToolTip=delta<0?"위로 이동":delta>0?"아래로 이동":"조건 삭제";button.IsEnabled=delta==0||index+delta>=0&&index+delta<_ruleEditors.Count;button.Click+=(_,_)=>{if(delta==0)_ruleEditors.Remove(editor);else{int current=_ruleEditors.IndexOf(editor);_ruleEditors.RemoveAt(current);_ruleEditors.Insert(current+delta,editor);}RefreshRuleRows();};actions.Children.Add(button);if(delta<0)editor.MoveUp=button;else if(delta>0)editor.MoveDown=button;else editor.Delete=button;}
  var enabled=new CheckBox{IsChecked=rule.Enabled,VerticalAlignment=VerticalAlignment.Center,Margin=new Thickness(0,0,8,0),ToolTip="이 조건 사용"};DockPanel.SetDock(enabled,Dock.Left);heading.Children.Add(enabled);var name=Label(d?.Label??rule.Key,13,true);name.ToolTip=d==null?"현재 자료에서 지원하지 않습니다. 제거하거나 사용을 해제하세요.":$"{d.Category} · {d.RangeLabel} · {d.Key}";name.TextWrapping=TextWrapping.NoWrap;name.TextTrimming=TextTrimming.CharacterEllipsis;heading.Children.Add(name);grid.Children.Add(heading);
  if(PositionSearch.IsPosition(rule)){var positionControls=BuildPositionRuleControls(editor);Grid.SetRow(positionControls,1);grid.Children.Add(positionControls);positionControls.IsEnabled=rule.Enabled;card.Opacity=rule.Enabled?1:.58;editor.EnabledControl=enabled;enabled.Checked+=(_,_)=>{rule.Enabled=true;RefreshRuleRows();};enabled.Unchecked+=(_,_)=>{rule.Enabled=false;RefreshRuleRows();};return card;}
  var controls=new Grid{Margin=new Thickness(0,6,0,0)};controls.ColumnDefinitions.Add(new(){Width=new GridLength(75)});controls.ColumnDefinitions.Add(new(){Width=new GridLength(100)});controls.ColumnDefinitions.Add(new());controls.ColumnDefinitions.Add(new(){Width=new GridLength(108)});Grid.SetRow(controls,1);grid.Children.Add(controls);
  bool firstActive=ReferenceEquals(_ruleEditors.FirstOrDefault(e=>e.Rule.Enabled),editor);var join=ChoiceBox();join.ItemsSource=firstActive?new[]{new SearchChoice("and","만약")}:new[]{new SearchChoice("and","그리고"),new SearchChoice("or","또는")};join.SelectedValue=firstActive?"and":rule.Join;join.IsEnabled=!firstActive;join.Margin=new Thickness(0,0,6,0);join.ToolTip=firstActive?"첫 번째 사용 조건입니다.":SearchConditionCatalog.LogicHint;join.SelectionChanged+=(_,_)=>{rule.Join=Selected(join,"and");UpdateExpression();};controls.Children.Add(join);
  var op=ChoiceBox();op.ItemsSource=d==null?SearchConditionCatalog.NumberOperators:SearchConditionCatalog.Operators(d);op.SelectedValue=rule.Operator;op.Margin=new Thickness(0,0,6,0);Grid.SetColumn(op,1);controls.Children.Add(op);
  var first=Input(rule.Value);FrameworkElement value;
  if(d?.Type is "choice" or "reference" or "boolean") {var select=new ComboBox{ItemsSource=d.Options,DisplayMemberPath=nameof(SearchConditionOption.Label),SelectedValuePath=nameof(SearchConditionOption.Value),SelectedValue=rule.Value,MinHeight=30,IsEditable=false,IsTextSearchEnabled=true,MaxDropDownHeight=300};TextSearch.SetTextPath(select,nameof(SearchConditionOption.Label));editor.ValueChoice=select;select.SelectionChanged+=(_,_)=>{if(select.SelectedValue is string v)rule.Value=v;UpdateExpression();};value=select;}
  else {editor.ValueChoice=null;first.TextChanged+=(_,_)=>{rule.Value=first.Text;UpdateExpression();};value=d?.Type=="number"?BoundedSpinner(first,d.Min,d.Max,d.Step):first;}
  Grid.SetColumn(value,2);value.Margin=new Thickness(0,0,6,0);controls.Children.Add(value);var second=Input(rule.Value2);second.TextChanged+=(_,_)=>{rule.Value2=second.Text;UpdateExpression();};var secondHost=d?.Type=="number"?BoundedSpinner(second,d.Min,d.Max,d.Step):second;Grid.SetColumn(secondHost,3);controls.Children.Add(secondHost);
  void UpdateOperator(){rule.Operator=Selected(op,rule.Operator);secondHost.Visibility=rule.Operator=="between"?Visibility.Visible:Visibility.Collapsed;controls.ColumnDefinitions[3].Width=rule.Operator=="between"?new GridLength(108):new GridLength(0);UpdateExpression();}
  op.SelectionChanged+=(_,_)=>UpdateOperator();UpdateOperator();controls.IsEnabled=rule.Enabled;card.Opacity=rule.Enabled?1:.58;
  editor.EnabledControl=enabled;enabled.Checked+=(_,_)=>{rule.Enabled=true;RefreshRuleRows();};enabled.Unchecked+=(_,_)=>{rule.Enabled=false;RefreshRuleRows();};return card;
 }
 private void UpdateExpression()
 {
  var active=_ruleEditors.Where(e=>e.Rule.Enabled).ToArray();_ruleCount.Text=$"상세 조건 {_ruleEditors.Count}개 · 사용 {active.Length}개";
  var groups=new List<List<string>>();foreach(var editor in active){if(groups.Count==0||editor.Rule.Join=="or")groups.Add([]);groups[^1].Add(SearchConditionCatalog.Describe(editor.Rule,_ruleCatalog));}
  _expression.Text=groups.Count==0?"상세 조건 없음 · 기본 조건만 적용":string.Join("  또는  ",groups.Select(g=>"("+string.Join(" 그리고 ",g)+")"));_expression.ToolTip=_expression.Text;
 }

 public async Task<object> QaDetailedAsync(string directory)
 {
  Directory.CreateDirectory(directory);var before=ReadDetailedRules();int tab=_tabs.SelectedIndex;double width=Width,height=Height;
  try {
   LoadDetailedRules([]);var numeric=_ruleCatalog.FirstOrDefault(d=>d.Source=="field"&&d.Min<=15&&d.Max>=20)??_ruleCatalog.First(d=>d.Type=="number");var a=SearchConditionCatalog.NewRule(numeric);a.Operator="between";a.Value=numeric.Min.ToString(CultureInfo.InvariantCulture);a.Value2=numeric.Max.ToString(CultureInfo.InvariantCulture);AddRule(a);var b=a.Clone();b.Join="or";b.Operator="ne";b.Value=Math.Clamp(15,numeric.Min,numeric.Max).ToString(CultureInfo.InvariantCulture);AddRule(b);var off=a.Clone();off.Enabled=false;off.Value="invalid draft";AddRule(off);
   var compiled=SearchConditionCatalog.Compile(ReadDetailedRules(),_ruleCatalog);if(compiled.Count!=2||compiled[1].join!="or"||compiled[1].op!="ne")throw new InvalidOperationException("상세 조건식 연결 검증 실패");
   _conditionSearch.Text=numeric.Key;FilterCatalog();if(!_conditionList.Items.Cast<SearchConditionDefinition>().Any(d=>d.Key==numeric.Key))throw new InvalidOperationException("조건 키 검색 실패");_conditionSearch.Text="";
   _tabs.SelectedIndex=3;await SettleAsync();Capture(Path.Combine(directory,"criteria-detailed.png"));Width=820;Height=620;await SettleAsync();Capture(Path.Combine(directory,"criteria-detailed-820x620.png"));
   foreach(var row in _ruleEditors.Where(e=>e.Row!=null)){if(row.Row!.ActualWidth<300)throw new InvalidOperationException("상세 조건 행이 너무 좁습니다.");}
   if(_detailBrowserTabs!=null){_detailBrowserTabs.SelectedIndex=1;await SettleAsync();Capture(Path.Combine(directory,"criteria-position-pitch-820x620.png"));_detailBrowserTabs.SelectedIndex=0;}
   bool draftPreserved=ReadDetailedRules()[2].Value=="invalid draft";var firstEditor=_ruleEditors[0];firstEditor.MoveDown!.RaiseEvent(new RoutedEventArgs(ButtonBase.ClickEvent));if(!ReferenceEquals(_ruleEditors[1],firstEditor))throw new InvalidOperationException("상세 조건 아래 이동 실패");firstEditor.MoveUp!.RaiseEvent(new RoutedEventArgs(ButtonBase.ClickEvent));if(!ReferenceEquals(_ruleEditors[0],firstEditor))throw new InvalidOperationException("상세 조건 위 이동 실패");firstEditor.EnabledControl!.IsChecked=false;if(firstEditor.Rule.Enabled||SearchConditionCatalog.Compile(ReadDetailedRules(),_ruleCatalog).Count!=1)throw new InvalidOperationException("상세 조건 사용 해제 실패");firstEditor.EnabledControl!.IsChecked=true;_ruleEditors[2].Delete!.RaiseEvent(new RoutedEventArgs(ButtonBase.ClickEvent));if(_ruleEditors.Count!=2)throw new InvalidOperationException("상세 조건 삭제 실패");
   var positions=await QaPositionPitchAsync(directory);
   var menus = await QaConditionMenusAsync(directory);
   return new {success=true,localOnly=true,typedRules=compiled.Count,disabledDraftPreserved=draftPreserved,catalogFields=_ruleCatalog.Count,pitchFields=_pitchButtons.Count(p=>p.Value.IsEnabled),searchByKey=true,reorderDeleteEnable=true,pitchAddsCondition=_pitchButtons.Any(p=>p.Value.IsEnabled),positions,menus};
  } finally {LoadDetailedRules(before);_tabs.SelectedIndex=tab;Width=width;Height=height;}
 }
}
