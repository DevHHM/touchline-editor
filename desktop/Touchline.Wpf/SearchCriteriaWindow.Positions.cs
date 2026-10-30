using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Automation.Provider;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shapes;

namespace Touchline.Wpf;

public sealed partial class SearchCriteriaWindow
{
 private readonly Dictionary<string,ToggleButton> _pitchButtons=[];
 private readonly ComboBox _positionProficiency=ChoiceBox();
 private readonly TextBlock _positionSelection=Label("포지션을 선택하세요.",11);
 private Button? _positionShortcut;
 private RuleEditor? _activePositionEditor;
 private bool _syncPositionPitch;

 private FrameworkElement BuildPositionPitch()
 {
  var panel=new StackPanel{Margin=new Thickness(0,8,0,0)};
  panel.Children.Add(Label("포지션",15,true));
  var hint=Label("선택한 위치 중 하나 이상에서 최소 숙련도를 만족하는 선수를 찾습니다.",11);hint.Margin=new Thickness(0,5,0,8);panel.Children.Add(hint);
  _positionProficiency.ItemsSource=PositionSearch.Proficiencies;_positionProficiency.SelectedValue="20";
  _positionProficiency.ToolTip="자연스러움은 20입니다. 다른 값은 정확한 숙련도 숫자로 선택합니다.";
  AutomationProperties.SetName(_positionProficiency,"포지션 최소 숙련도");
  _positionProficiency.SelectionChanged+=(_,_)=>{if(_syncPositionPitch)return;if(_activePositionEditor!=null){_activePositionEditor.Rule.Value=Selected(_positionProficiency,"20");RefreshRuleRows();}};
  panel.Children.Add(Labeled("최소 숙련도",_positionProficiency));
  var pitch=new Canvas{Width=212,Height=272,Background=new SolidColorBrush(Color.FromRgb(25,48,47)),ClipToBounds=true,Focusable=false};
  var line=new SolidColorBrush(Color.FromRgb(79,114,105));
  void Box(double x,double y,double width,double height){var shape=new Rectangle{Width=width,Height=height,Stroke=line,StrokeThickness=1,IsHitTestVisible=false};Canvas.SetLeft(shape,x);Canvas.SetTop(shape,y);pitch.Children.Add(shape);}
  Box(9,9,194,254);Box(60,9,92,35);Box(60,228,92,35);
  pitch.Children.Add(new Line{X1=9,X2=203,Y1=136,Y2=136,Stroke=line,IsHitTestVisible=false});
  var circle=new Ellipse{Width=50,Height=50,Stroke=line,IsHitTestVisible=false};Canvas.SetLeft(circle,81);Canvas.SetTop(circle,111);pitch.Children.Add(circle);
  var available=PositionSearch.Available(_ruleCatalog);var style=PositionToggleStyle();
  foreach(var (code,x,y) in new (string,double,double)[]{("ST",106,27),("AML",36,68),("AMC",106,68),("AMR",176,68),("ML",36,109),("MC",106,109),("MR",176,109),("WBL",36,150),("DM",106,150),("WBR",176,150),("DL",36,191),("DC",106,191),("DR",176,191),("GK",106,242)}){
   string key=PositionSearch.FieldKey(code);var button=new ToggleButton{Content=code,Width=48,Height=30,FontSize=11,FontWeight=FontWeights.Bold,Style=style,IsEnabled=available.Contains(key),Tag=key};
   button.ToolTip=button.IsEnabled?code+" 선택 / 해제 · Space 또는 Enter":code+" 숙련도를 현재 자료에서 조회할 수 없습니다.";AutomationProperties.SetName(button,code+" 포지션 선택");
   button.Checked+=(_,_)=>PositionToggled(key,true);button.Unchecked+=(_,_)=>PositionToggled(key,false);
   button.PreviewKeyDown+=(_,args)=>{if(args.Key==Key.Enter){button.IsChecked=button.IsChecked!=true;args.Handled=true;}};
   Canvas.SetLeft(button,x-24);Canvas.SetTop(button,y-15);pitch.Children.Add(button);_pitchButtons[key]=button;
  }
  KeyboardNavigation.SetDirectionalNavigation(pitch,KeyboardNavigationMode.Cycle);panel.Children.Add(pitch);
  _positionSelection.Margin=new Thickness(0,8,0,8);panel.Children.Add(_positionSelection);
  var clear=Button("이 포지션 조건 지우기");clear.FontSize=11;clear.Click+=(_,_)=>{if(_activePositionEditor!=null)_ruleEditors.Remove(_activePositionEditor);_activePositionEditor=null;RefreshRuleRows();};panel.Children.Add(clear);
  return new ScrollViewer{Content=panel,VerticalScrollBarVisibility=ScrollBarVisibility.Auto,HorizontalScrollBarVisibility=ScrollBarVisibility.Disabled};
 }
 private static Style PositionToggleStyle()
 {
  var style=new Style(typeof(ToggleButton));
  style.Setters.Add(new Setter(Control.ForegroundProperty,new SolidColorBrush(Color.FromRgb(218,230,226))));
  style.Setters.Add(new Setter(Control.BackgroundProperty,new SolidColorBrush(Color.FromRgb(36,65,59))));
  style.Setters.Add(new Setter(Control.BorderBrushProperty,new SolidColorBrush(Color.FromRgb(96,130,120))));
  var border=new FrameworkElementFactory(typeof(Border));border.SetValue(Border.CornerRadiusProperty,new CornerRadius(5));border.SetValue(Border.BorderThicknessProperty,new Thickness(1));border.SetBinding(Border.BackgroundProperty,new System.Windows.Data.Binding("Background"){RelativeSource=new(System.Windows.Data.RelativeSourceMode.TemplatedParent)});border.SetBinding(Border.BorderBrushProperty,new System.Windows.Data.Binding("BorderBrush"){RelativeSource=new(System.Windows.Data.RelativeSourceMode.TemplatedParent)});
  var content=new FrameworkElementFactory(typeof(ContentPresenter));content.SetValue(FrameworkElement.HorizontalAlignmentProperty,HorizontalAlignment.Center);content.SetValue(FrameworkElement.VerticalAlignmentProperty,VerticalAlignment.Center);border.AppendChild(content);
  style.Setters.Add(new Setter(Control.TemplateProperty,new ControlTemplate(typeof(ToggleButton)){VisualTree=border}));
  var selected=new Trigger{Property=ToggleButton.IsCheckedProperty,Value=true};selected.Setters.Add(new Setter(Control.BackgroundProperty,new SolidColorBrush(Color.FromRgb(115,217,170))));selected.Setters.Add(new Setter(Control.ForegroundProperty,new SolidColorBrush(Color.FromRgb(13,40,29))));style.Triggers.Add(selected);
  var focus=new Trigger{Property=UIElement.IsKeyboardFocusedProperty,Value=true};focus.Setters.Add(new Setter(Control.BorderBrushProperty,Brushes.White));style.Triggers.Add(focus);
  var disabled=new Trigger{Property=UIElement.IsEnabledProperty,Value=false};disabled.Setters.Add(new Setter(UIElement.OpacityProperty,.35));style.Triggers.Add(disabled);return style;
 }
 private void PositionToggled(string key,bool selected)
 {
  if(_syncPositionPitch)return;
  if(_activePositionEditor==null){if(!selected)return;if(_ruleEditors.Count>=SearchConditionCatalog.Limit){ShowError("상세 조건은 최대 64개입니다.");SyncPositionPitch();return;}var rule=new SearchConditionRule{Source=PositionSearch.Source,Key=PositionSearch.Key,Operator="gte",Value=Selected(_positionProficiency,"20"),Positions=[key]};AddRule(rule);return;}
  var positions=_activePositionEditor.Rule.Positions;
  if(selected&&!positions.Contains(key))positions.Add(key);else if(!selected)positions.Remove(key);
  if(positions.Count==0){_ruleEditors.Remove(_activePositionEditor);_activePositionEditor=null;}
  RefreshRuleRows();ClearError();
 }
 private void SyncPositionPitch()
 {
  if(_kind!="Player"||_syncPositionPitch)return;_syncPositionPitch=true;
  try{
   if(_activePositionEditor==null||!_ruleEditors.Contains(_activePositionEditor))_activePositionEditor=_ruleEditors.FirstOrDefault(e=>PositionSearch.IsPosition(e.Rule));
   var rule=_activePositionEditor?.Rule;
   foreach(var (key,button) in _pitchButtons)button.IsChecked=rule?.Positions.Contains(key)==true;
   if(rule!=null)_positionProficiency.SelectedValue=rule.Value;
   _positionSelection.Text=rule==null?"아직 선택한 포지션이 없습니다.":PositionSearch.Describe(rule);
   if(_positionShortcut!=null)_positionShortcut.Content=rule==null?"경기장에서 포지션 선택…":string.Join(" / ",rule.Positions.Select(PositionSearch.Code))+" · "+(rule.Value=="20"?"자연스러움":rule.Value+" 이상");
  }finally{_syncPositionPitch=false;}
 }
 private FrameworkElement BuildPositionRuleControls(RuleEditor editor)
 {
  var rule=editor.Rule;var grid=new Grid{Margin=new Thickness(0,6,0,0)};grid.ColumnDefinitions.Add(new(){Width=new GridLength(75)});grid.ColumnDefinitions.Add(new());grid.ColumnDefinitions.Add(new(){Width=new GridLength(156)});
  bool first=ReferenceEquals(_ruleEditors.FirstOrDefault(e=>e.Rule.Enabled),editor);var join=ChoiceBox();join.ItemsSource=first?new[]{new SearchChoice("and","만약")}:new[]{new SearchChoice("and","그리고"),new SearchChoice("or","또는")};join.SelectedValue=first?"and":rule.Join;join.IsEnabled=!first;join.Margin=new Thickness(0,0,6,0);join.SelectionChanged+=(_,_)=>{rule.Join=Selected(join,"and");UpdateExpression();};grid.Children.Add(join);
  string positionNames=string.Join(" / ",rule.Positions.Select(PositionSearch.Code));var choose=Button(positionNames+"  ▾");choose.Content=new TextBlock{Text=positionNames+"  ▾",TextWrapping=TextWrapping.Wrap,TextAlignment=TextAlignment.Center,MaxHeight=34};choose.Padding=new Thickness(7,4,7,4);choose.Margin=new Thickness(0,0,6,0);choose.ToolTip=positionNames+" · 경기장에서 선택 · 선택한 위치 중 하나 이상 일치";choose.Click+=(_,_)=>{_activePositionEditor=editor;_tabs.SelectedIndex=3;if(_detailBrowserTabs!=null)_detailBrowserTabs.SelectedIndex=1;SyncPositionPitch();};Grid.SetColumn(choose,1);grid.Children.Add(choose);
  var proficiency=ChoiceBox();proficiency.ItemsSource=PositionSearch.Proficiencies;proficiency.SelectedValue=rule.Value;AutomationProperties.SetName(proficiency,"이 조건의 최소 포지션 숙련도");proficiency.SelectionChanged+=(_,_)=>{rule.Value=Selected(proficiency,"20");UpdateExpression();SyncPositionPitch();};Grid.SetColumn(proficiency,2);grid.Children.Add(proficiency);return grid;
 }
 private FrameworkElement BuildPositionShortcut()
 {
  var panel=new StackPanel();_positionShortcut=Button("경기장에서 포지션 선택…");_positionShortcut.Click+=(_,_)=>{_tabs.SelectedIndex=3;if(_detailBrowserTabs!=null)_detailBrowserTabs.SelectedIndex=1;SyncPositionPitch();};panel.Children.Add(_positionShortcut);
  if(_original.Role.Length>0){panel.Children.Add(Label("이전 검색의 포지션 표기 조건도 유지됩니다.",10));panel.Children.Add(_role);}return panel;
 }
 private async Task<object?> QaPositionPitchAsync(string directory)
 {
  if(_kind!="Player")return null;
  var saved=ReadDetailedRules();double width=Width,height=Height;var available=_pitchButtons.Where(p=>p.Value.IsEnabled).Take(2).ToArray();
  if(available.Length<2)throw new InvalidOperationException("포지션 QA에는 조회 가능한 위치가 두 개 이상 필요합니다.");
  try{
   LoadDetailedRules([]);_positionProficiency.SelectedValue="20";_tabs.SelectedIndex=3;_detailBrowserTabs!.SelectedIndex=1;
   foreach(var item in available){var toggle=new ToggleButtonAutomationPeer(item.Value).GetPattern(PatternInterface.Toggle) as IToggleProvider??throw new InvalidOperationException("포지션 키보드/접근성 선택 패턴이 없습니다.");toggle.Toggle();}
   var rules=ReadDetailedRules();var compiled=SearchConditionCatalog.Compile(rules,_ruleCatalog);
   if(rules.Count!=1||compiled.Count!=1||compiled[0].source!=PositionSearch.Source||compiled[0].positions?.Count!=2||Convert.ToInt32(compiled[0].value)!=20)throw new InvalidOperationException("복수 포지션이 하나의 자연스러움 조건으로 묶이지 않았습니다.");
   string serialized=System.Text.Json.JsonSerializer.Serialize(Snapshot());var restored=System.Text.Json.JsonSerializer.Deserialize<SearchCriteria>(serialized)!;LoadDraft(restored);
   if(available.Any(p=>p.Value.IsChecked!=true)||ReadDetailedRules()[0].Positions.Count!=2)throw new InvalidOperationException("저장한 포지션 조건과 피치 선택을 복원하지 못했습니다.");
   _positionProficiency.SelectedValue="17";if(ReadDetailedRules()[0].Value!="17")throw new InvalidOperationException("숙련도 선택과 조건 행이 동기화되지 않았습니다.");_positionProficiency.SelectedValue="20";
   Width=1000;Height=740;await SettleAsync();Capture(System.IO.Path.Combine(directory,"position-search-pitch-1000x740.png"));
   Width=820;Height=620;await SettleAsync();Capture(System.IO.Path.Combine(directory,"position-search-pitch-820x620.png"));
   if(_ruleEditors[0].Row!.ActualWidth<300||available.Any(p=>p.Value.ActualWidth<40||p.Value.ActualHeight<28))throw new InvalidOperationException("포지션 선택 UI의 클릭 영역이 너무 작습니다.");
   foreach(var button in _pitchButtons.Values.Where(b=>b.IsEnabled))button.IsChecked=true;
   await SettleAsync();Capture(System.IO.Path.Combine(directory,"position-search-all-820x620.png"));if(ReadDetailedRules().Count!=1||ReadDetailedRules()[0].Positions.Count!=_pitchButtons.Count(p=>p.Value.IsEnabled))throw new InvalidOperationException("모든 포지션을 하나의 조건에 넣지 못했습니다.");
   _ruleEditors[0].Delete!.RaiseEvent(new RoutedEventArgs(ButtonBase.ClickEvent));if(_pitchButtons.Values.Any(b=>b.IsChecked==true)||ReadDetailedRules().Count!=0)throw new InvalidOperationException("조건 삭제 뒤 피치 선택이 남았습니다.");
   return new{success=true,gameWrites=false,keyboardTogglePattern=true,oneConditionWithTwoAlternatives=true,naturalThreshold=20,exactNumericThreshold=17,persistenceRoundTrip=true,deleteSynchronizesPitch=true};
  }finally{LoadDetailedRules(saved);Width=width;Height=height;}
 }
}
