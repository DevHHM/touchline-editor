using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace Touchline.Wpf;

/// <summary>Local draft only; Cancel never changes the applied layout.</summary>
public sealed class SearchColumnWindow : Window
{
 private readonly string _kind;
 private readonly List<SearchColumnSpec> _available;
 private readonly ObservableCollection<SearchColumnSpec> _selected = [];
 private readonly Dictionary<string, CheckBox> _checks = [];
 private readonly Dictionary<string, FrameworkElement> _rows = [];
 private readonly TextBox _find = new() { MinHeight = 32, Padding = new Thickness(9,5,9,5), ToolTip = "열 이름 또는 키로 검색" };
 private readonly ComboBox _category = new() { MinHeight = 32 };
 private readonly ListBox _order = new() { DisplayMemberPath = "Label", Background = Brushes.Transparent, BorderThickness = new Thickness(0) };
 private readonly TextBlock _count = Text("",14,true), _error = Text("");
 private bool _sync;
 public List<string>? Result { get; private set; }
 public SearchColumnWindow(string kind, List<SearchColumnSpec> available, IEnumerable<string> selected)
 {
  _kind=kind; _available=available; Title="검색 결과 · 열 설정";
  Width=900; Height=660; MinWidth=720; MinHeight=480; MaxHeight=SystemParameters.WorkArea.Height-32;
  WindowStartupLocation=WindowStartupLocation.CenterOwner; ShowInTaskbar=false; Background=Brush("BaseBrush"); Foreground=Brush("InkBrush"); FontFamily=Typography.AppFont; FontWeight=FontWeights.Medium; UseLayoutRounding=true;
  Typography.Initialize(this);
  var root=new Grid { Margin=new Thickness(20) }; root.RowDefinitions.Add(new(){Height=GridLength.Auto});root.RowDefinitions.Add(new());root.RowDefinitions.Add(new(){Height=GridLength.Auto});Content=root;
  var header=new StackPanel { Margin=new Thickness(0,0,0,16) }; header.Children.Add(Text("필요한 정보로 구성하기",22,true));header.Children.Add(Text("왼쪽에서 열을 선택하고, 오른쪽에서 표시 순서를 정하세요. 이름은 첫 번째 열에 고정됩니다."));root.Children.Add(header);
  var body=new Grid();body.ColumnDefinitions.Add(new(){Width=new GridLength(1.25,GridUnitType.Star)});body.ColumnDefinitions.Add(new(){Width=new GridLength(18)});body.ColumnDefinitions.Add(new());Grid.SetRow(body,1);root.Children.Add(body);
  var left=new Grid();left.RowDefinitions.Add(new(){Height=GridLength.Auto});left.RowDefinitions.Add(new(){Height=GridLength.Auto});left.RowDefinitions.Add(new());body.Children.Add(Panel(left));
  _find.Name="ColumnSearch";_find.Margin=new Thickness(0,0,0,8);left.Children.Add(_find);
  _category.Items.Add("전체 정보");foreach(string section in available.Select(c=>c.Section).Distinct())_category.Items.Add(section);_category.SelectedIndex=0;_category.Margin=new Thickness(0,0,0,10);Grid.SetRow(_category,1);left.Children.Add(_category);
  var choices=new StackPanel();var scroll=new ScrollViewer { Content=choices,VerticalScrollBarVisibility=ScrollBarVisibility.Auto,HorizontalScrollBarVisibility=ScrollBarVisibility.Disabled };Grid.SetRow(scroll,2);left.Children.Add(scroll);
  foreach(var column in available)
  {
   var label=new StackPanel();label.Children.Add(Text(column.Label,13,true));label.Children.Add(Text(column.Section,11));
   var check=new CheckBox { Content=label,Margin=new Thickness(2,5,2,5),Padding=new Thickness(4),VerticalContentAlignment=VerticalAlignment.Center,ToolTip=column.Hint,IsEnabled=column.Key!="name" };
   check.Checked+=(_,_)=>Select(column,true);check.Unchecked+=(_,_)=>Select(column,false);_checks[column.Key]=check;_rows[column.Key]=check;choices.Children.Add(check);
  }
  var right=new Grid();right.RowDefinitions.Add(new(){Height=GridLength.Auto});right.RowDefinitions.Add(new());right.RowDefinitions.Add(new(){Height=GridLength.Auto});var rightPanel=Panel(right);Grid.SetColumn(rightPanel,2);body.Children.Add(rightPanel);
  _count.Margin=new Thickness(0,0,0,10);right.Children.Add(_count);_order.ItemsSource=_selected;_order.Name="SelectedColumns";
  var itemStyle=new Style(typeof(ListBoxItem)); itemStyle.Setters.Add(new Setter(Control.PaddingProperty,new Thickness(10,8,10,8)));itemStyle.Setters.Add(new Setter(Control.ForegroundProperty,Brush("InkBrush")));
  var itemBorder=new FrameworkElementFactory(typeof(Border));itemBorder.SetBinding(Border.BackgroundProperty,new System.Windows.Data.Binding("Background"){RelativeSource=new System.Windows.Data.RelativeSource(System.Windows.Data.RelativeSourceMode.TemplatedParent)});itemBorder.SetBinding(Border.PaddingProperty,new System.Windows.Data.Binding("Padding"){RelativeSource=new System.Windows.Data.RelativeSource(System.Windows.Data.RelativeSourceMode.TemplatedParent)});itemBorder.SetValue(Border.CornerRadiusProperty,new CornerRadius(4));itemBorder.AppendChild(new FrameworkElementFactory(typeof(ContentPresenter)));itemStyle.Setters.Add(new Setter(Control.TemplateProperty,new ControlTemplate(typeof(ListBoxItem)){VisualTree=itemBorder}));
  var selectedStyle=new Trigger{Property=ListBoxItem.IsSelectedProperty,Value=true};selectedStyle.Setters.Add(new Setter(Control.BackgroundProperty,Brush("AccentBrush")));selectedStyle.Setters.Add(new Setter(Control.ForegroundProperty,new SolidColorBrush(Color.FromRgb(17,42,37))));itemStyle.Triggers.Add(selectedStyle);_order.ItemContainerStyle=itemStyle;
  Grid.SetRow(_order,1);right.Children.Add(_order);
  var controls=new WrapPanel { Margin=new Thickness(0,10,0,0) };var up=Button("위로 ↑",()=>Move(-1));var down=Button("아래로 ↓",()=>Move(1));var remove=Button("제거",Remove);foreach(var b in new[]{up,down,remove}){b.Margin=new Thickness(0,0,6,0);controls.Children.Add(b);}Grid.SetRow(controls,2);right.Children.Add(controls);
  var footer=new DockPanel { Margin=new Thickness(0,16,0,0) };Grid.SetRow(footer,2);root.Children.Add(footer);
  var actions=new StackPanel { Orientation=Orientation.Horizontal };DockPanel.SetDock(actions,Dock.Right);footer.Children.Add(actions);
  var reset=Button("기본 열로 복원",()=>SetSelection(SearchColumns.Default(_kind)));var cancel=Button("취소",Close);cancel.IsCancel=true;var apply=Button("열 적용",()=>{Result=_selected.Select(c=>c.Key).ToList();DialogResult=true;});apply.Name="ApplyColumns";apply.Style=TryFindResource("PrimaryButton") as Style;
  foreach(var b in new[]{reset,cancel,apply}){b.Margin=new Thickness(8,0,0,0);actions.Children.Add(b);}
  _error.Foreground=new SolidColorBrush(Color.FromRgb(245,160,160));footer.Children.Add(_error);
  _find.TextChanged+=(_,_)=>Filter();_category.SelectionChanged+=(_,_)=>Filter();SetSelection(selected);
 }
 private static Brush Brush(string key)=>Application.Current.TryFindResource(key) as Brush??new SolidColorBrush(Color.FromRgb(18,24,32));
 private static TextBlock Text(string text,double size=12,bool strong=false)=>new(){Text=text,FontSize=size,FontWeight=strong?FontWeights.SemiBold:FontWeights.Medium,Foreground=Brush(strong?"InkBrush":"MutedBrush"),TextWrapping=TextWrapping.Wrap};
 private static Button Button(string text,Action action){var b=new Button{Content=text,Padding=new Thickness(12,8,12,8)};b.Click+=(_,_)=>action();return b;}
 private static Border Panel(UIElement content)=>new(){Child=content,Padding=new Thickness(12),CornerRadius=new CornerRadius(8),BorderThickness=new Thickness(1),BorderBrush=Brush("BorderBrush"),Background=Brush("PanelBrush")};
 private void Select(SearchColumnSpec column,bool selected)
 {
  if(_sync)return;
  if(selected&&!_selected.Contains(column))
  {
   if(column.Numeric&&_selected.Count(c=>c.Numeric)>=SearchColumns.MaxNumericColumns){_sync=true;_checks[column.Key].IsChecked=false;_sync=false;_error.Text=$"능력치 열은 최대 {SearchColumns.MaxNumericColumns}개입니다.";return;}
   _selected.Add(column);_order.SelectedItem=column;
  }
  else if(!selected&&column.Key!="name")_selected.Remove(column);
  _count.Text=$"표시할 열 · {_selected.Count}개";_error.Text="";
 }
 private void SetSelection(IEnumerable<string> keys)
 {
  var order=SearchColumns.Normalize(keys,_available);_sync=true;_selected.Clear();foreach(string key in order)_selected.Add(_available.First(c=>c.Key==key));foreach(var pair in _checks)pair.Value.IsChecked=order.Contains(pair.Key);_sync=false;_count.Text=$"표시할 열 · {_selected.Count}개";_error.Text="";
 }
 private void Move(int delta){int i=_order.SelectedIndex;if(i<1)return;int next=i+delta;if(next<1||next>=_selected.Count)return;_selected.Move(i,next);_order.SelectedIndex=next;}
 private void Remove(){if(_order.SelectedItem is SearchColumnSpec c&&c.Key!="name")_checks[c.Key].IsChecked=false;}
 private void Filter(){string term=_find.Text.Trim();string section=_category.SelectedItem as string??"전체 정보";foreach(var c in _available)_rows[c.Key].Visibility=(section=="전체 정보"||section==c.Section)&&(term.Length==0||c.Label.Contains(term,StringComparison.CurrentCultureIgnoreCase)||c.Key.Contains(term,StringComparison.OrdinalIgnoreCase))?Visibility.Visible:Visibility.Collapsed;}
 internal void QaSelect(string key,bool select)=>_checks[key].IsChecked=select;
 internal void QaMove(string key,int delta){_order.SelectedItem=_selected.Single(c=>c.Key==key);Move(delta);}
 internal List<string> QaKeys()=>_selected.Select(c=>c.Key).ToList();
 internal int QaFilter(string text){_find.Text=text;return _rows.Values.Count(row=>row.Visibility==Visibility.Visible);}
 internal void QaAccept(){Result=_selected.Select(c=>c.Key).ToList();DialogResult=true;}
}
