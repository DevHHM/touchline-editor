using System.ComponentModel;
using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;
using System.Windows.Threading;
using Microsoft.Win32;

namespace Touchline.Wpf;

public partial class MainWindow : Window
{
 private readonly BridgeSession _bridgeSession = new();
 private BridgeClient? _bridge => _bridgeSession.Client;
 private EntityRecord? _ownClub;
 private DetailRecord? _detail;
 private bool _ready, _live, _busy, _closing, _precision, _syncSelection;
 private int _renderVersion;
 private int _offset, _total, _searchVersion, _routeIndex=-1;
 private string _kind="Player", _tab="개요";
 private string? _listKind;
 private readonly List<(string Kind,uint Id)> _routes=[];
 private readonly DispatcherTimer _searchTimer=new() { Interval=TimeSpan.FromMilliseconds(280) };
 private readonly DispatcherTimer _scoreTimer=new() { Interval=TimeSpan.FromMilliseconds(380) };
 private readonly List<ToolsWindow> _toolWindows=[];
 private StackPanel? _scoresHost;
 private int _scoreVersion;
 private readonly List<string> _qaChecks=[];
 private readonly string? _qaOutput=App.Arg("--ui-check");
 private bool _qaRunning;

 public MainWindow()
 {
  InitializeComponent();
  InitializeRecentHistory();
  InitializeAbilityVisibility();
  BuildVersionLabel.Text="NATIVE  ·  WPF + RUST  ·  "+typeof(App).Assembly.GetName().Version?.ToString(3);
  InitializeResponsiveLayout();
  InitializeUsability();
  InitializeUpdates();
  InitializeLiveWatch();
  NameCopy.AttachGrid(EntityGrid, item => item as EntityRecord, Status);
  _searchTimer.Tick += async (_,_) => { _searchTimer.Stop(); await SearchAsync(); };
  _scoreTimer.Tick += async (_,_) => { _scoreTimer.Stop(); await RefreshDraftScores(); };
  Loaded += OnLoaded;
  Closing += OnClosing;
 }
 private static Brush Brush(string color) => (Brush)new BrushConverter().ConvertFromString(color)!;
 private Brush ResourceBrush(string name) => (Brush)FindResource(name);
 private static TextBlock Text(string text,double size=13,string? color=null,bool bold=false)
 {
  var block=new TextBlock { Text=text,FontSize=size,Foreground=Brush(color??"#EBF0F8"),FontWeight=bold?FontWeights.Bold:FontWeights.Medium,TextWrapping=TextWrapping.Wrap };
  if(!bold)block.SetResourceReference(TextBlock.FontWeightProperty,"AppBodyFontWeight");
  return block;
 }
 private Button Button(string label,RoutedEventHandler click,bool primary=false) { var b=new Button { Content=label,Style=(Style)FindResource(primary?"PrimaryButton":"QuietButton"),HorizontalContentAlignment=HorizontalAlignment.Center }; b.Click+=click; return b; }
 private static string Label(string kind) => kind switch { "Player"=>"선수","Staff"=>"스태프","Club"=>"구단","City"=>"도시",_=>"기타 인물" };
 private static string TraitGroup(string key) {
  if(!int.TryParse(key.Replace("trait_",""),out int id))return "기타";
  return id switch {
   1 or 2 or 3 or 28 or 29 or 30 or 31 or 33 or 35 or 36 or 40 or 41 or 47 or 52 or 53 or 54 or 58 or 59 or 61 or 64=>"드리블·볼 처리",
   7 or 8 or 20 or 21 or 23 or 25 or 26 or 27 or 44 or 45 or 48 or 60=>"패스·연계",
   9 or 10 or 11 or 12 or 13 or 15 or 22 or 24 or 37 or 43 or 46 or 57=>"슛·마무리",
   4 or 5 or 6 or 14 or 19 or 32 or 34 or 55=>"움직임",
   16 or 38 or 39=>"수비",
   _=>"성향·기타"};
 }
 private static T? Decode<T>(JsonElement json) => json.Deserialize<T>(JsonDefaults.Options);
 private void Status(string text) { StatusText.Text=text; }
 private void SetBusy(bool busy,string text="불러오는 중…") { _busy=busy||_liveReconnectBusy; BusyText.Text=text; BusyLayer.Visibility=_busy?Visibility.Visible:Visibility.Collapsed; ConnectButton.IsEnabled=!_busy; }
 private async Task Guard(Func<Task> work,bool busy=false,string title="불러오는 중…") { if(busy&&_busy)return;try { if(busy)SetBusy(true,title); await work(); } catch(Exception e) { App.Log(e); Status(e.Message); if(_qaRunning) throw; AppDialog.Show(this,e.Message,"작업을 완료하지 못했습니다",MessageBoxButton.OK,MessageBoxImage.Information); } finally { if(busy) SetBusy(false); } }
 private string EnginePath()
 {
  return PackagePaths.ResolveEngine(AppContext.BaseDirectory, App.Arg("--engine"));
 }
 private async void OnLoaded(object sender,RoutedEventArgs e)
 {
  _qaRunning=_qaOutput!=null;
  if(App.Arg("--kind") is string startingKind && startingKind is "Player" or "Staff" or "Club" or "Person" or "City")_kind=startingKind;
  try {
   SetSummary(await EnsureEngineReadyAsync()); SetNavigation();
   if(App.Arg("--width") is string width && double.TryParse(width,out double w))Width=w;
   if(App.Arg("--height") is string height && double.TryParse(height,out double h))Height=h;
   if(App.Arg("--snapshot") is string path) await Guard(async()=> { SetSummary(await _bridge!.CallAsync("load_snapshot",new{path=System.IO.Path.GetFullPath(path)})); await SearchAsync(); await OpenInitialEntityAsync(); },true,"저장된 선수단 불러오는 중…");
   if(App.Arg("--connect")=="readonly") {EnableLiveWatch();SetBusy(true,"FM 데이터베이스 연결 중…");try {SetSummary(await _bridge!.CallAsync("connect"));await SearchAsync();await OpenInitialEntityAsync();}catch(Exception ex){App.Log(ex);if(_qaRunning)throw;await ShowConnectFailureAsync(ex);}finally{SetBusy(false);}}
   if(_qaRunning) { await RunUiCheck(); await CloseSafely(); Application.Current.Shutdown(); }
   else if(_dataLoaded) {
    if(App.Arg("--entity") is string startId&&uint.TryParse(startId,out uint parsedId)) {
     await Guard(async()=> { await OpenEntity(_kind,parsedId); if(App.Arg("--tab") is string startTab){_tab=startTab;await RenderDetail();} },true);
    } else await ShowSearchAsync(_kind);
   }
  } catch(Exception ex) { App.Log(ex); Status(ex.Message); if(_qaOutput!=null) { Directory.CreateDirectory(System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(_qaOutput))!); File.WriteAllText(_qaOutput+".error.txt",ex.ToString()); Environment.ExitCode=1; await CloseSafely(); Application.Current.Shutdown(1); } else ShowConnectionProblem(_ready?"error":"engine_error", ex.Message); }
 }
 private void SetSummary(JsonElement data,bool newHistorySession=false)
 {
  _growthAvailability=ClubGrowthAvailability.FromSummary(data);
  _dataLoaded=data.TryGetProperty("connected",out var connected)&&connected.ValueKind==JsonValueKind.True;SearchScreenButton.IsEnabled=_dataLoaded;
  _recentHistory.UpdateConnection(_dataLoaded,newHistorySession);
  _live=data.TryGetProperty("live",out var live)&&live.GetBoolean();
  GameEditionText.Text=GameEditionPresentation.FromSummary(data);
  GameEditionText.ToolTip=_dataLoaded&&data.TryGetProperty("profile",out var profile)&&profile.ValueKind==JsonValueKind.String?profile.GetString():"게임에 연결하면 버전을 표시합니다.";
  if(_live)EnableLiveWatch();else if(_dataLoaded)DisableLiveWatch();
  _ownClub=data.TryGetProperty("own_club",out var c)&&c.ValueKind==JsonValueKind.Object?Decode<EntityRecord>(c):null;
  var date=data.TryGetProperty("game_date",out var d)?d.GetString():null;
  OwnClubLabel.Text=_ownClub?.Name??"소속 구단 없음";
  ConnectionLabel.Text=_live?"●  FM 실시간 연결":_dataLoaded?"○  스냅샷 · 읽기 전용":"○  연결 없음";
  ConnectionLabel.Foreground=ResourceBrush(_live?"AccentBrush":"GoldBrush");
  ConnectButton.Content=_live?"FM 다시 연결":"FM 연결";
  GameDateLabel.Text=$"{date??"—"}  ·  {(_live?"LIVE":_dataLoaded?"SNAPSHOT":"OFFLINE")}";
  Status(_live?"현재 세이브 연결됨 · 수정은 변경 검토 후 적용됩니다.":_dataLoaded?"저장 당시 정보입니다. 미리보기는 가능하며 게임에는 적용되지 않습니다.":"FM에 다시 연결하거나 스냅샷을 열어주세요.");
 }
 private bool CanLeave()
 {
  if(_relationsView?.IsBusy==true){Status("인물 관계 작업이 끝난 뒤 이동하세요.");return false;}
  if(_detail?.Fields.Any(f=>f.IsDirty)!=true&&_relationsView?.HasPendingChanges!=true)return true;
  return AppDialog.Show(this,"아직 적용하지 않은 변경 내용이 있습니다. 버리고 이동할까요?","변경 내용",MessageBoxButton.YesNo,MessageBoxImage.Question)==MessageBoxResult.Yes;
 }
 private void ClearDetail() { _abilityDisplayRefresh.Clear(); _relationsView?.Invalidate(); _staffReviewView?.Invalidate(); _rosterView?.Invalidate(); _renderVersion++; _scoreVersion++; _scoresHost=null; if(_detail!=null)foreach(var f in _detail.Fields)f.PropertyChanged-=FieldChanged; _detail=null; DirtyBar.Visibility=Visibility.Collapsed; ReviewButton.IsEnabled=false; }
 private async void Connect_Click(object sender,RoutedEventArgs e)
 {
  if(_busy||_livePollBusy||!CanLeave())return;
  SetBusy(true,"FM 데이터베이스 연결 중 · 최초 연결에는 시간이 걸릴 수 있습니다");
  try {
   // Cancelling a workbench close must keep the current connection and view.
   try {await CloseWorkbenches();}catch(OperationCanceledException){Status("연결 변경을 취소했습니다.");return;}
   DiagnosticsLog.Current?.Write("connection","connect_attempt");
   EnableLiveWatch();_reloadContext=null;
   await EnsureEngineReadyAsync();
   await DisposeSearchView(); CloseTools(); await StopDetailViewsAsync(); ClearDetail();
   SetSummary(await _bridge!.CallAsync("connect"),true); _routes.Clear();_routeIndex=-1;_offset=0; await ShowSearchAsync(_kind);
   DiagnosticsLog.Current?.Write("connection","connect_succeeded");
  } catch(Exception ex) {
   App.Log(ex); if(_qaRunning)throw;
   await ShowConnectFailureAsync(ex);
  } finally {SetBusy(false);}
 }
 private async void LoadSnapshot_Click(object sender,RoutedEventArgs e)
 {
  if(_busy||_livePollBusy||_bridge==null||!CanLeave())return;
  var dialog=new OpenFileDialog {Title="읽기 전용 스냅샷 열기",Filter="Touchline 데이터 (*.json)|*.json"};
  _liveWatchTimer.Stop();
  if(dialog.ShowDialog(this)!=true){if(_liveReload.Enabled)_liveWatchTimer.Start();return;}
  DisableLiveWatch();_reloadContext=null;
  await Guard(async()=> { await CloseWorkbenches(); await DisposeSearchView(); CloseTools(); await StopDetailViewsAsync(); ClearDetail(); SetSummary(await _bridge.CallAsync("load_snapshot",new{path=dialog.FileName}),true);_routes.Clear();_routeIndex=-1;_offset=0;await ShowSearchAsync(_kind); },true);
 }
 private void SetNavigation()
 {
  foreach(var b in NavigationButtons.Children.OfType<Button>()) { bool selected=(string)b.Tag==(LeagueScreen.IsVisible?"League":_kind); b.Background=selected?Brush("#233B38"):Brush("Transparent");b.Foreground=selected?ResourceBrush("AccentBrush"):ResourceBrush("MutedBrush"); }
  ListTitle.Text=Label(_kind);Breadcrumb.Text="데이터베이스  /  "+Label(_kind);
  OwnOnly.IsEnabled=_kind is "Player" or "Staff" or "Person";
  SortBox.IsEnabled=_kind is "Player" or "Staff";
  GroupBox.Visibility=_kind=="Player"?Visibility.Visible:Visibility.Collapsed;
  FootballBox.Visibility=_kind is "Player" or "Staff" or "Club"?Visibility.Visible:Visibility.Collapsed;
 }
 private async void Navigate_Click(object sender,RoutedEventArgs e)
 {
  if(!_ready||_busy||!CanLeave())return;
  _kind=(string)((Button)sender).Tag;_offset=0;SearchBox.Text="";_searchTimer.Stop();SetNavigation();await Guard(()=>ShowSearchAsync(_kind),true,"검색 필터 불러오는 중…");
 }
 private async Task SearchAsync()
 {
  if(!_ready||_bridge==null)return;
  int generation=++_searchVersion;
  string listKind=_kind;
  var args=new Dictionary<string,object?> { ["kind"]=_kind,["query"]=SearchBox.Text.Trim(),["offset"]=_offset,["limit"]=50,["sort"]=(SortBox.SelectedItem as ComboBoxItem)?.Tag?.ToString()??"name" };
  if(AbilityVisibility.Current.IsHidden(args["sort"]?.ToString()))args["sort"]="name";
  args["descending"]=args["sort"]?.ToString() is "ca" or "pa";
  if(_kind is not ("Player" or "Staff")) {args["sort"]="name";args["descending"]=false;}
  if(OwnOnly.IsChecked==true&&OwnOnly.IsEnabled&&_ownClub!=null)args["club_id"]=_ownClub.Id;
  var group=(GroupBox.SelectedItem as ComboBoxItem)?.Tag?.ToString();
  if(_kind=="Player"&&!string.IsNullOrWhiteSpace(group))args["group"]=group;
  if(_kind is "Player" or "Staff" or "Club") {
   string? category=(FootballBox.SelectedItem as ComboBoxItem)?.Tag?.ToString();
   args["filter"]=new {football_category=string.IsNullOrEmpty(category)?null:category};
  }
  string? requestedSort=_listRequestedSort=args["sort"]?.ToString();
  var recentSearch=CaptureQuickSearch(listKind,args);
  try { var data=await _bridge.CallAsync("search",args);if(generation!=_searchVersion||AbilityVisibility.Current.IsHidden(requestedSort))return;var records=Decode<List<EntityRecord>>(data.GetProperty("items"))??[];_total=data.GetProperty("total").GetInt32();_listKind=listKind;_listSort=requestedSort;EntityGrid.ItemsSource=records;ResultCount.Text=$"{_total:N0}{(_kind is "Club" or "City"?"개":"명")}";PageLabel.Text=$"{_offset/50+1} / {Math.Max(1,(_total+49)/50)}";PrevPage.IsEnabled=_offset>0;NextPage.IsEnabled=_offset+50<_total; RecordQuickSearch(recentSearch); } catch(Exception ex) { Status(ex.Message);if(_qaRunning)throw; }
 }
 private async Task OpenFirstAsync() { if(EntityGrid.ItemsSource is List<EntityRecord> list&&list.Count>0)await OpenEntity(list[0].Kind,list[0].Id);else {ClearDetail(); DetailHost.Children.Clear();DetailHost.Children.Add(Text("검색 결과가 없습니다.",22));} }
 private async Task OpenInitialEntityAsync()
 {
  if(App.Arg("--entity") is string id && uint.TryParse(id,out uint parsedId))await OpenEntity(_kind,parsedId);
  else await OpenFirstAsync();
 }
 private void Search_Changed(object sender,TextChangedEventArgs e) { if(!_ready)return;_offset=0;_searchTimer.Stop();_searchTimer.Start(); }
 private void Filter_Changed(object sender,RoutedEventArgs e) { if(!_ready)return;_offset=0;_searchTimer.Stop();_searchTimer.Start(); }
 private void Sort_Changed(object sender,SelectionChangedEventArgs e)=>Filter_Changed(sender,new());
 private void Group_Changed(object sender,SelectionChangedEventArgs e)=>Filter_Changed(sender,new());
 private void Football_Changed(object sender,SelectionChangedEventArgs e)=>Filter_Changed(sender,new());
 private async void PrevPage_Click(object sender,RoutedEventArgs e) { _offset=Math.Max(0,_offset-50);await SearchAsync(); }
 private async void NextPage_Click(object sender,RoutedEventArgs e) { _offset+=50;await SearchAsync(); }
 private async void Entity_Selected(object sender,SelectionChangedEventArgs e) { if(!_ready||_busy||_syncSelection||EntityGrid.SelectedItem is not EntityRecord item)return;if(!CanLeave()){SyncSelection();return;}await Guard(()=>OpenEntity(item.Kind,item.Id),true); }
 private void SyncSelection() { _syncSelection=true;EntityGrid.SelectedItem=EntityGrid.Items.OfType<EntityRecord>().FirstOrDefault(x=>x.Id==_detail?.Entity.Id&&x.Kind==_detail?.Entity.Kind);_syncSelection=false; }
 private async Task OpenEntity(string kind,uint id,bool addRoute=true,RecentEntityVisit? expectedVisit=null)
 {
  if(_bridge==null)return;
  long historyGeneration=_recentHistory.SessionGeneration,viewEpoch=_liveViewEpoch;
  await StopDetailViewsAsync();
  _scoreTimer.Stop();
  var data=await _bridge.CallAsync("detail",new{kind,id});
  var next=Decode<DetailRecord>(data)??throw new InvalidDataException("상세 정보를 읽을 수 없습니다.");next.InitializeFields();
  if (!_dataLoaded || viewEpoch!=_liveViewEpoch || historyGeneration!=_recentHistory.SessionGeneration) return;
  if (expectedVisit!=null && (!_recentHistory.CanReopen(expectedVisit) || !expectedVisit.Matches(next.Entity)))
   throw new InvalidOperationException("기록과 현재 대상이 다릅니다. 최근 기록에서 이름으로 다시 검색하세요.");
  ClearDetail();_detail=next;_tab="개요";ShowProfile();
  if(_kind!=kind||_listKind!=kind) {_kind=kind;_offset=0;SearchBox.Text="";_searchTimer.Stop();SetNavigation();await SearchAsync();}
  SyncSelection();
  foreach(var f in next.Fields) { f.ShowPrecision=_precision;f.PropertyChanged+=FieldChanged; }
  if(addRoute) { if(_routeIndex<_routes.Count-1)_routes.RemoveRange(_routeIndex+1,_routes.Count-_routeIndex-1);if(_routes.Count==0||_routes[^1]!=(kind,id))_routes.Add((kind,id));_routeIndex=_routes.Count-1; }
  BackButton.IsEnabled=_routeIndex>0;ForwardButton.IsEnabled=_routeIndex<_routes.Count-1;
  Breadcrumb.Text=$"{Label(kind)}  /  {next.Entity.Name}";
  if(next.Entity.ClubId==_ownClub?.Id&&next.BadgePath!=null)OwnBadge.Source=LoadImage(next.BadgePath,100);
  if(kind=="Club"&&id==_ownClub?.Id)OwnBadge.Source=LoadImage(next.ImagePath,100);
  await RenderDetail();DetailScroll.ScrollToTop();Status($"{next.Entity.Name}  ·  ID {id}  ·  {(_live?"현재 게임 정보":"저장된 정보 · 읽기 전용")}");
  if (historyGeneration==_recentHistory.SessionGeneration && _dataLoaded) { _recentHistory.Visit(next.Entity); SaveRecentHistory(); }
 }
 private static BitmapImage? LoadImage(string? path,int width=300)
 {
  if(string.IsNullOrEmpty(path)||!File.Exists(path))return null;
  try { using var stream=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.ReadWrite|FileShare.Delete);if(stream.Length>32*1024*1024)return null;var b=new BitmapImage();b.BeginInit();b.CacheOption=BitmapCacheOption.OnLoad;b.DecodePixelWidth=width;b.StreamSource=stream;b.EndInit();b.Freeze();return b; } catch {return null;}
 }
 private UIElement Hero(DetailRecord d)
 {
  var panel=new Grid { Margin=new Thickness(0,0,0,22) };panel.ColumnDefinitions.Add(new(){Width=new GridLength(d.Entity.Kind=="Player"?154:108)});panel.ColumnDefinitions.Add(new(){Width=new GridLength(1,GridUnitType.Star)});panel.ColumnDefinitions.Add(new(){Width=GridLength.Auto});
  var photo=LoadImage(d.ImagePath);
  var portrait=new Border { Background=Brush("#202D3A"),CornerRadius=new(9),Height=d.Entity.Kind=="Player"?134:88,Margin=new(0,0,20,0),VerticalAlignment=VerticalAlignment.Top,ClipToBounds=true };
  portrait.Child=photo!=null?new Image {Source=photo,Stretch=Stretch.Uniform,Margin=d.Entity.Kind=="Club"?new(12):new(0)}:new TextBlock {Text=d.Entity.Kind=="Club"?"FC":d.Entity.Kind=="City"?"⌖":d.Entity.Name[..Math.Min(1,d.Entity.Name.Length)],FontSize=44,Foreground=Brush("#527064"),VerticalAlignment=VerticalAlignment.Center,HorizontalAlignment=HorizontalAlignment.Center};panel.Children.Add(portrait);
  var info=new StackPanel {Margin=new(0,4,12,0)};Grid.SetColumn(info,1);panel.Children.Add(info);
  info.Children.Add(Text(d.Entity.Kind=="Player"?$"{d.Entity.Role}  /  {d.Entity.Group}":d.Entity.Role,11,"#75DFC4",true));
  var title=Text(d.Entity.DisplayName,27,null,true);title.Margin=new(0,8,0,4);info.Children.Add(title);
  if(d.Entity.EnglishName!=d.Entity.Name)info.Children.Add(Text(d.Entity.EnglishName,12,"#929FB3"));
  info.Children.Add(NameCopy.CreateButtons(d.Entity,Status));
  if(d.Entity.ClubId is uint clubId&&d.Entity.Kind!="Club") {var club=Button(d.Entity.Club,async(_,_)=> {if(CanLeave())await Guard(()=>OpenEntity("Club",clubId),true);});club.HorizontalAlignment=HorizontalAlignment.Left;club.Padding=new(0,9,0,8);club.Foreground=ResourceBrush("GoldBrush");info.Children.Add(club);}
  var baseFields=d.Fields.Where(f=>f.Section=="기본"&&f.Key.Contains("height")).Select(f=>$"{f.Value:0} cm").ToList();if(d.Entity.Age.HasValue)baseFields.Insert(0,d.Entity.Age+"세");baseFields.Add("ID "+d.Entity.Id);info.Children.Add(Text(string.Join("   ·   ",baseFields),11,"#929FB3"));
  if(d.Entity.Kind=="Staff") { var overlay=Button("FM 코치 오버레이",(_,_)=>ShowTools("overlay",staff:d.Entity));overlay.HorizontalAlignment=HorizontalAlignment.Left;overlay.Margin=new(0,10,0,0);overlay.Padding=new(0,5,0,5);info.Children.Add(overlay); }
  if(d.Entity.Kind=="Club") {var tools=new WrapPanel {Margin=new(0,10,0,0)};tools.Children.Add(Button("이벤트 만들기",(_,_)=>ShowTools("events",d.Entity.Id)));if(d.CityId is uint city)tools.Children.Add(Button("연고 도시 ↗",async(_,_)=> {if(CanLeave())await Guard(()=>OpenEntity("City",city),true);}));info.Children.Add(tools);}
  if(d.Entity.Ca.HasValue||d.Entity.Pa.HasValue) {
   var ability=new StackPanel {Width=112,Margin=new(6,6,0,0)};Grid.SetColumn(ability,2);panel.Children.Add(ability);
   foreach(var v in new[]{("현재 능력",d.Entity.Ca,"#75DFC4","ca"),("잠재 능력",d.Entity.Pa,"#E9C882","pa")}) {var c=new Border {Background=Brush("#1B2532"),CornerRadius=new(7),Padding=new(12,9,12,10),Margin=new(0,0,0,8)};var s=new StackPanel();s.Children.Add(Text(v.Item1,10,"#929FB3"));var value=Text("",25,v.Item3,true);value.Margin=new(0,3,0,0);s.Children.Add(value);var bar=new ProgressBar {Minimum=0,Maximum=200,Height=3,Margin=new(0,8,0,0),Foreground=Brush(v.Item3),Background=Brush("#33404F"),BorderThickness=new(0)};s.Children.Add(bar);c.Child=s;ability.Children.Add(c);TrackAbilityDisplay(()=>{bool hidden=AbilityVisibility.Current.IsHidden(v.Item4);value.Text=AbilityVisibility.Current.Format(v.Item4,v.Item2);value.FontSize=hidden?18:25;bar.Value=hidden?0:v.Item2??0;bar.Visibility=hidden?Visibility.Hidden:Visibility.Visible;});}
  }
  return panel;
 }
 private async Task RenderDetail()
 {
  if(_detail==null)return;await StopDetailViewsAsync();var d=_detail;int renderVersion=++_renderVersion;_scoresHost=null;_abilityDisplayRefresh.Clear();DetailHost.Children.Clear();DetailHost.Children.Add(d.Entity.Kind=="Club"&&(_tab is "스태프" or "선수단")?StaffClubHeader(d):Hero(d));
  var tabs=new WrapPanel {Margin=new(0,0,0,18)};
  string[] names=d.Entity.Kind switch {"Player"=>["개요","성격·히든","포지션","선수 특성","계약·신상","커리어 계획","선호·관계","국적·이름","전체 능력치"],"Staff"=>["개요","직무 점수","성격·히든","계약·신상","커리어 계획","선호·관계","국적·이름","전체 능력치"],"Person"=>["개요","커리어 계획","선호·관계","국적·이름","전체 능력치"],"Club"=>["개요","선수단","스태프","재정","후원·구장"],_=>["개요","전체 능력치"]};
  foreach(var name in names) {var b=Button(name,async(_,_)=>await ChangeDetailTabAsync(name));b.Margin=new(0,0,6,0);b.Padding=new(11,8,11,8);if(name==_tab){b.Background=Brush("#263D39");b.Foreground=ResourceBrush("AccentBrush");}tabs.Children.Add(b);}
  DetailHost.Children.Add(tabs);
  if(DetailPresentation.BasicPersonOnly(d)) {
   var information=new StackPanel();information.Children.Add(Text("기본 정보 조회",15,null,true));
   foreach(var message in DetailPresentation.BasicPersonMessages(d)){var note=Text(message,12,"#929FB3");note.Margin=new(0,10,0,0);information.Children.Add(note);}
   DetailHost.Children.Add(new Border {Background=ResourceBrush("PanelBrush"),BorderBrush=ResourceBrush("BorderBrush"),BorderThickness=new(1),CornerRadius=new(8),Padding=new(18),Child=information});
   return;
  }
  if(d.Entity.Kind is "Player" or "Staff" or "Person" && IsRelationsTab(_tab)){await RenderRelationsAsync(d,renderVersion);return;}
  if(d.Entity.Kind=="Club"&&_tab=="스태프") {await RenderStaffReviewAsync(d,renderVersion);return;}
  if(d.Entity.Kind=="Club"&&_tab=="선수단") {await RenderClubRosterAsync(d,renderVersion);return;}
  if(d.Entity.Kind=="Club"&&_tab=="후원·구장") {await RenderSponsors(d,renderVersion);return;}
  var controlLine=new DockPanel { Margin=new(0,0,0,12) };var precision=new CheckBox {Content="세부값",Foreground=ResourceBrush("MutedBrush"),IsChecked=_precision,FontSize=11,VerticalAlignment=VerticalAlignment.Center};DockPanel.SetDock(precision,Dock.Right);controlLine.Children.Add(precision);precision.Checked+=PrecisionChanged;precision.Unchecked+=PrecisionChanged;controlLine.Children.Add(Text(_tab=="직무 점수"?"사용자 계산식 · 변경 초안 즉시 반영":"능력치 · 숫자를 선택하여 수정",11,"#929FB3"));DetailHost.Children.Add(controlLine);
  IEnumerable<IGrouping<string,EditableField>> groups;
  if(d.Entity.Kind=="Staff"&&_tab=="직무 점수") { _scoresHost=new StackPanel();DetailHost.Children.Add(_scoresHost);RenderScores(d.Scores);return; }
  if(_tab=="성격·히든") groups=d.Fields.Where(f=>f.Section is "성격" or "경기 히든").GroupBy(f=>f.Section);
  else if(_tab=="선수 특성") groups=d.Fields.Where(f=>f.Section=="선수 특성").GroupBy(f=>TraitGroup(f.Key));
  else if(_tab=="계약·신상") groups=d.Fields.Where(f=>f.Key.StartsWith("contract.")||f.Key.StartsWith("person.")||f.Key.StartsWith("player.")||f.Key.Contains("reputation")).GroupBy(f=>f.Section);
  else if(_tab=="포지션") {DetailHost.Children.Add(BuildPitch(d));groups=d.Fields.Where(f=>f.Section=="포지션").GroupBy(f=>f.Section);}
  else if(_tab=="재정")groups=d.Fields.Where(f=>f.Section=="재정").GroupBy(f=>f.Section);
  else if(_tab=="전체 능력치")groups=d.Fields.GroupBy(f=>f.Section);
  else if(d.Entity.Kind=="Player")groups=d.Fields.Where(f=>f.Section is "기술" or "정신" or "신체" || (d.Entity.Role.Contains("GK")&&f.Section=="골키퍼")).GroupBy(f=>f.Section);
  else if(d.Entity.Kind=="Staff")groups=d.Fields.Where(f=>f.Section!="성격").GroupBy(f=>f.Section);
  else groups=d.Fields.GroupBy(f=>f.Section);
  var groupList=groups.ToList();
  if(d.Entity.Kind=="Player"&&_tab=="개요")groupList=groupList.OrderBy(g=>g.Key switch {"기술"=>0,"정신"=>1,"신체"=>2,_=>3}).ToList();
  var columns=new AdaptiveColumnsPanel {MinColumnWidth=_tab=="계약·신상"?290:210,MaxColumns=3,ColumnGap=14};
  foreach(var group in groupList)columns.Children.Add(FieldCard(group.Key,group));
  DetailHost.Children.Add(columns);
  if(d.Entity.Kind=="Player"&&_tab=="개요") {
   var lower=new AdaptiveColumnsPanel {Margin=new(0,14,0,0),MinColumnWidth=250,MaxColumns=2,ColumnGap=14};
   var basics=FieldCard("선수 정보",d.Fields.Where(f=>f.Section=="기본"));lower.Children.Add(basics);
   var overview=new Border {CornerRadius=new(8),Background=ResourceBrush("PanelBrush"),Padding=new(15),Margin=new(7,0,0,0)};Grid.SetColumn(overview,1);var note=new StackPanel();note.Children.Add(Text("선수 관리",13,null,true));note.Children.Add(Text("성격·히든과 포지션은 위 탭에서 조정할 수 있습니다.",12,"#929FB3"));note.Children.Add(Text("개별 능력치를 바꿔도 CA는 자동 계산되지 않습니다.",11,"#BDAB8A"));foreach(var t in note.Children.OfType<TextBlock>())t.Margin=new(0,0,0,10);overview.Child=note;lower.Children.Add(overview);DetailHost.Children.Add(lower);
  }
  if(d.Entity.Kind=="Staff"&&_tab=="개요") { _scoresHost=new StackPanel {Margin=new(0,16,0,0)};DetailHost.Children.Add(_scoresHost);RenderScores(d.Scores,true); }
  if(d.Notes.Count>0) {var notes=new StackPanel {Margin=new(0,16,0,0)};foreach(var n in d.Notes){var text=Text(n,11,"#8190A5");text.Margin=new(0,0,0,5);notes.Children.Add(text);}DetailHost.Children.Add(notes);}
 }
 private Border FieldCard(string title,IEnumerable<EditableField> fields)
 {
  var stack=new StackPanel();var head=new DockPanel {Margin=new(0,0,0,10)};head.Children.Add(Text(title,14,null,true));stack.Children.Add(head);
  int index=0;foreach(var f in fields) {
   if(f.Key.StartsWith("trait_")) {var check=new CheckBox {Content=new TextBlock{Text=f.Label,TextWrapping=TextWrapping.Wrap,FontSize=12},DataContext=f,ToolTip=f.Hint,IsEnabled=f.Editable,Margin=new(3,6,3,6)};check.SetBinding(CheckBox.IsCheckedProperty,new Binding("EditText"){Mode=BindingMode.TwoWay,UpdateSourceTrigger=UpdateSourceTrigger.PropertyChanged,Converter=new BitTextConverter()});stack.Children.Add(check);continue;}
   if(f.Key is "person.dob" or "contract.expires") {var dateRow=new DockPanel{Margin=new(0,5,0,5)};var picker=new DatePicker{DataContext=f,Width=142,IsEnabled=f.Editable,ToolTip=f.Hint,SelectedDateFormat=DatePickerFormat.Short};picker.SetBinding(DatePicker.SelectedDateProperty,new Binding("EditText"){Mode=BindingMode.TwoWay,Converter=new DateTextConverter()});DockPanel.SetDock(picker,Dock.Right);dateRow.Children.Add(picker);dateRow.Children.Add(Text(f.Label,12,"#BFC9D9"));stack.Children.Add(dateRow);continue;}
   var row=new Grid {Height=29,DataContext=f,ToolTip=f.Hint};var rowStyle=new Style(typeof(Grid));rowStyle.Setters.Add(new Setter(BackgroundProperty,index++%2==0?Brush("#1D2633"):Brush("Transparent")));var dirtyTrigger=new DataTrigger {Binding=new Binding("IsDirty"),Value=true};dirtyTrigger.Setters.Add(new Setter(BackgroundProperty,Brush("#344137")));rowStyle.Triggers.Add(dirtyTrigger);row.Style=rowStyle;row.ColumnDefinitions.Add(new(){Width=new(1,GridUnitType.Star)});row.ColumnDefinitions.Add(new(){Width=new(f.Max>100000?115:f.Max>200?75:46)});
   var label=Text(f.Label,12,"#BFC9D9");label.Margin=new(7,0,6,0);label.TextWrapping=TextWrapping.NoWrap;label.TextTrimming=TextTrimming.CharacterEllipsis;row.Children.Add(label);
   var edit=new TextBox {DataContext=f,Padding=new(3,2,6,2),BorderThickness=new(0),Background=Brush("Transparent"),FontSize=13,FontWeight=FontWeights.Bold,HorizontalContentAlignment=HorizontalAlignment.Right,VerticalContentAlignment=VerticalAlignment.Center,IsReadOnly=!f.Editable,ToolTip=$"{f.Label} · {f.Min:G}–{f.Max:G}\n{f.Hint}"};Grid.SetColumn(edit,1);edit.SetBinding(TextBox.TextProperty,new Binding("EditText") {Mode=BindingMode.TwoWay,UpdateSourceTrigger=UpdateSourceTrigger.PropertyChanged,ValidatesOnDataErrors=true});edit.SetBinding(TextBox.ForegroundProperty,new Binding("ColorHex"));
   edit.GotKeyboardFocus+=(_,_)=>edit.SelectAll();edit.PreviewKeyDown+=(_,e)=> { if(e.Key is Key.Up or Key.Down){f.Adjust(e.Key==Key.Up?1:-1);e.Handled=true;} };
   row.Children.Add(edit);
   if(AbilityVisibility.AbilityKey(f.Key)!=null) {
    var hidden=Text("숨김",12,"#929FB3");hidden.HorizontalAlignment=HorizontalAlignment.Right;hidden.Margin=new(0,0,6,0);Grid.SetColumn(hidden,1);row.Children.Add(hidden);
    TrackAbilityDisplay(()=>{bool mask=AbilityVisibility.Current.IsHidden(f.Key);edit.Visibility=mask?Visibility.Collapsed:Visibility.Visible;hidden.Visibility=mask?Visibility.Visible:Visibility.Collapsed;row.ToolTip=mask?"CA·PA 표시 설정에서 숨겨진 항목입니다.":f.Hint;});
   }
   stack.Children.Add(row);
  }
  return new Border {Background=ResourceBrush("PanelBrush"),BorderBrush=ResourceBrush("BorderBrush"),BorderThickness=new(1),CornerRadius=new(8),Padding=new(13,14,13,12),Margin=new(0,0,0,12),Child=stack};
 }
 private void PrecisionChanged(object sender,RoutedEventArgs e) { _precision=((CheckBox)sender).IsChecked==true;if(_detail!=null)foreach(var f in _detail.Fields)f.ShowPrecision=_precision; }
 private void FieldChanged(object? sender,PropertyChangedEventArgs e)
 {
  if(e.PropertyName!=nameof(EditableField.IsDirty)||_detail==null)return;
  int dirty=_detail.Fields.Count(f=>f.IsDirty);bool valid=_detail.Fields.All(f=>f.IsValid);DirtyBar.Visibility=dirty>0?Visibility.Visible:Visibility.Collapsed;DirtyLabel.Text=valid?$"{dirty}개 항목 변경됨  ·  아직 게임에 적용되지 않았습니다.":"입력값의 허용 범위를 확인하세요.";ReviewButton.IsEnabled=dirty>0&&valid;
  RefreshAbilityReviewAvailability();
  if(_detail.Entity.Kind=="Staff") {_scoreTimer.Stop();_scoreTimer.Start();}
 }
 private async Task RefreshDraftScores()
 {
  if(_bridge==null||_detail?.Entity.Kind!="Staff"||_detail.Fields.Any(f=>!f.IsValid))return;
  var d=_detail;int version=++_scoreVersion;
  try {var p=await _bridge.CallAsync("preview",new{kind=d.Entity.Kind,id=d.Entity.Id,edits=d.Fields.Where(f=>f.IsDirty).Select(f=>new{key=f.Key,value=f.Value}).ToArray()});if(version!=_scoreVersion||_detail!=d)return;if(p.TryGetProperty("scores",out var scores)){d.Scores=scores.Clone();RenderScores(scores,_tab=="개요");}}catch(Exception ex){Status(ex.Message);}
 }
 private void RenderScores(JsonElement scores,bool trainingOnly=false)
 {
  if(_scoresHost==null||scores.ValueKind!=JsonValueKind.Array)return;_scoresHost.Children.Clear();var fields=Decode<List<EditableField>>(scores)??[];
  if(trainingOnly)fields=fields.Where(f=>DetailPresentation.IsTrainingScore(f.Key)).ToList();
  var header=Text(trainingOnly?"훈련 분야별 점수":"직무 적합도",15,null,true);header.Margin=new(0,0,0,12);_scoresHost.Children.Add(header);
  var grid=new AdaptiveColumnsPanel {MinColumnWidth=190,MaxColumns=trainingOnly?2:3,ColumnGap=8};
  foreach(var f in fields) {var row=new DockPanel {Margin=new(0,0,0,2)};var v=Text(f.Value.ToString("0.#",CultureInfo.InvariantCulture),16,"#75DFC4",true);v.Margin=new(10,0,0,0);DockPanel.SetDock(v,Dock.Right);row.Children.Add(v);row.Children.Add(Text(f.Label,12,"#C8D4E4"));grid.Children.Add(new Border {Background=ResourceBrush("PanelBrush"),CornerRadius=new(6),Padding=new(12),Margin=new(0,0,8,8),Child=row});}_scoresHost.Children.Add(grid);
 }
 private UIElement BuildPitch(DetailRecord d)
 {
  var canvas=new Canvas {Width=550,Height=240,Background=Brush("#153D35"),ClipToBounds=true};canvas.Children.Add(new Rectangle {Width=510,Height=210,Stroke=Brush("#51796E"),StrokeThickness=1,Margin=new(20,15,0,0)});canvas.Children.Add(new Rectangle {Width=1,Height=210,Fill=Brush("#51796E"),Margin=new(275,15,0,0)});canvas.Children.Add(new Ellipse {Width=80,Height=80,Stroke=Brush("#51796E"),Margin=new(235,80,0,0)});
  var coordinates=new Dictionary<string,(string Label,double X,double Y)> { ["gk"]=("GK",25,108),["sw"]=("SW",90,108),["dl"]=("DL",155,32),["dc"]=("DC",155,108),["dr"]=("DR",155,184),["dm"]=("DM",220,108),["wbl"]=("WBL",220,32),["wbr"]=("WBR",220,184),["ml"]=("ML",285,32),["mc"]=("MC",285,108),["mr"]=("MR",285,184),["aml"]=("AML",350,32),["amc"]=("AMC",350,108),["amr"]=("AMR",350,184),["st"]=("ST",440,108) };
  foreach(var f in d.Fields.Where(f=>f.Section=="포지션")) {if(!coordinates.TryGetValue(f.Key.Replace("position_",""),out var p))continue;var box=new Border {Width=51,Height=38,CornerRadius=new(6),Background=f.Value>=15?Brush("#65C6A7"):Brush("#284F45"),ToolTip=f.Label,Child=new TextBlock {Text=$"{p.Label}\n{f.Value:0}",Foreground=Brush(f.Value>=15?"#102D25":"#B0CCC1"),TextAlignment=TextAlignment.Center,VerticalAlignment=VerticalAlignment.Center,FontSize=10}};Canvas.SetLeft(box,p.X);Canvas.SetTop(box,p.Y);canvas.Children.Add(box);}
  return new Border {CornerRadius=new(8),Child=canvas,HorizontalAlignment=HorizontalAlignment.Center,Margin=new(0,0,0,18)};
 }
 private async Task RenderSponsors(DetailRecord d,int version)
 {
  if(_bridge==null)return;
  DetailHost.Children.Add(FieldCard("시설·구장",d.Fields.Where(f=>f.Section!="재정")));
  try {var context=await _bridge.CallAsync("event_context",new{club_id=d.Entity.Id});if(version!=_renderVersion)return;var sponsors=context.GetProperty("sponsors");var title=Text("현재 후원 계약",16,null,true);title.Margin=new(0,12,0,12);DetailHost.Children.Add(title);
   foreach(var s in sponsors.EnumerateArray()) {var row=new StackPanel();row.Children.Add(Text($"후원 계약 #{s.GetProperty("slot")} · 연간 £{s.GetProperty("annual_gbp").GetDouble():N0}",14));row.Children.Add(Text($"{s.GetProperty("start")} → {s.GetProperty("end")}  ·  총액 £{s.GetProperty("total_gbp").GetDouble():N0}",12,"#929FB3"));DetailHost.Children.Add(new Border {Background=ResourceBrush("PanelBrush"),CornerRadius=new(8),Padding=new(16),Margin=new(0,0,0,8),Child=row});}
   DetailHost.Children.Add(Button("후원·지역 개발 이벤트 만들기",(_,_)=>ShowTools("events",d.Entity.Id),true));
  } catch(Exception ex) {DetailHost.Children.Add(Text(ex.Message,12,"#E9C882"));}
 }
 private async void Review_Click(object sender,RoutedEventArgs e)
 {
  if(_busy||_bridge==null||_detail==null||!ReviewButton.IsEnabled)return;
  await Guard(async()=> {var d=_detail;var p=await _bridge.CallAsync("preview",new {kind=d.Entity.Kind,id=d.Entity.Id,edits=d.Fields.Where(f=>f.IsDirty).Select(f=>new{key=f.Key,value=f.Value}).ToArray()});await ShowReview(p);},true,"변경 내용을 검증하는 중…");
 }
 private async Task ShowReview(JsonElement preview)
 {
  SetBusy(false);long viewEpoch=_liveViewEpoch;bool apply=false;var dialog=new Window {Owner=this,Title="변경 검토",Width=660,Height=560,WindowStartupLocation=WindowStartupLocation.CenterOwner,ResizeMode=ResizeMode.CanResize};_scalarReviewWindow=dialog;var dock=new DockPanel {Margin=new(24)};
  var buttons=new StackPanel {Orientation=Orientation.Horizontal,HorizontalAlignment=HorizontalAlignment.Right,Margin=new(0,20,0,0)};DockPanel.SetDock(buttons,Dock.Bottom);dock.Children.Add(buttons);buttons.Children.Add(Button("돌아가기",(_,_)=>dialog.Close()));var save=Button("백업 후 게임에 적용",(_,_)=>{apply=true;dialog.Close();},true);save.Margin=new(10,0,0,0);save.IsEnabled=preview.GetProperty("can_apply").GetBoolean()&&!preview.GetProperty("changes").EnumerateArray().Any(c=>AbilityVisibility.Current.IsHidden(c.TryGetProperty("key",out var key)?key.GetString():null));buttons.Children.Add(save);
  var top=new StackPanel {Margin=new(0,0,0,20)};DockPanel.SetDock(top,Dock.Top);dock.Children.Add(top);top.Children.Add(Text("변경 내용 확인",24,null,true));var applyGuidance=Text(_live?EditApplicationGuidance.BeforeApply:"스냅샷 모드입니다. 실제 적용하려면 FM을 연결해야 합니다.",12,_live?"#929FB3":"#E9C882");if(_live)applyGuidance.ToolTip=EditApplicationGuidance.Detail;top.Children.Add(applyGuidance);
  var rows=new StackPanel();foreach(var c in preview.GetProperty("changes").EnumerateArray()) {var line=new DockPanel();string? key=c.TryGetProperty("key",out var fieldKey)?fieldKey.GetString():null;var change=Text($"{AbilityVisibility.Current.Format(key,c.GetProperty("before"))}  →  {AbilityVisibility.Current.Format(key,c.GetProperty("after"))}",14,"#75DFC4",true);DockPanel.SetDock(change,Dock.Right);line.Children.Add(change);line.Children.Add(Text(c.GetProperty("label").GetString()??""));rows.Children.Add(new Border {Background=ResourceBrush("PanelBrush"),Padding=new(14),Margin=new(0,0,0,5),CornerRadius=new(6),Child=line});}dock.Children.Add(new ScrollViewer {Content=rows,VerticalScrollBarVisibility=ScrollBarVisibility.Auto});dialog.Content=dock;
  if(_qaRunning&&!_live)dialog.Loaded+=async(_,_)=>{await Task.Delay(100);CaptureWindow(dialog,System.IO.Path.Combine(System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(_qaOutput!))!,"wpf-review.png"));dialog.Close();};
  try {dialog.ShowDialog();}finally{if(_scalarReviewWindow==dialog)_scalarReviewWindow=null;}
  if(apply&&viewEpoch==_liveViewEpoch&&_live&&_bridge!=null&&_detail!=null) {SetBusy(true,"원본 백업 · 적용 · 재검증 중…");var identity=(_detail.Entity.Kind,_detail.Entity.Id);var receipt=await _bridge.CallAsync("apply",new{token=preview.GetProperty("token").GetString()});await OpenEntity(identity.Kind,identity.Id,false);Status(EditApplicationGuidance.Applied);}
 }
 private async void ResetEdits_Click(object sender,RoutedEventArgs e) {if(_detail==null)return;foreach(var f in _detail.Fields)f.Reset();await RefreshDraftScores();}
 private async void Refresh_Click(object sender,RoutedEventArgs e) {if(_bridge==null||!CanLeave()||_busy)return;await Guard(RefreshCurrentAsync,true);}
 private async Task RefreshCurrentAsync()
 {
  if(_bridge==null)return;
  if(LeagueScreen.IsVisible||_returnToLeague){await RefreshLeagueContextAsync();return;}
  var current=_detail?.Entity;bool searchVisible=SearchScreen.IsVisible;
  await CloseWorkbenches();await DisposeSearchView();CloseTools();await StopDetailViewsAsync();ClearDetail();
  SetSummary(await _bridge.CallAsync("refresh"),true);
  if(searchVisible)await ShowSearchAsync(_kind);else{await SearchAsync();if(current!=null)await OpenEntity(current.Kind,current.Id,false);}
 }
 private async void OwnClub_Click(object sender,RoutedEventArgs e) {if(!_busy&&_ownClub!=null&&CanLeave())await Guard(()=>OpenEntity("Club",_ownClub.Id),true);}
 private async void Back_Click(object sender,RoutedEventArgs e) {if(_busy||_routeIndex<=0||!CanLeave())return;var p=_routes[--_routeIndex];await Guard(()=>OpenEntity(p.Kind,p.Id,false),true);}
 private async void Forward_Click(object sender,RoutedEventArgs e) {if(_busy||_routeIndex>=_routes.Count-1||!CanLeave())return;var p=_routes[++_routeIndex];await Guard(()=>OpenEntity(p.Kind,p.Id,false),true);}
 private void ShowTools(string mode,uint? club=null,EntityRecord? staff=null) {if(_busy||_bridge==null)return;var window=new ToolsWindow(_bridge,mode,club??_ownClub?.Id,staff,_live) {Owner=this,BeforeUndo=BeforeHistoryUndoAsync,AfterUndo=AfterHistoryUndoAsync};_toolWindows.Add(window);window.Closed+=(_,_)=>_toolWindows.Remove(window);window.Show();}
 private void Events_Click(object sender,RoutedEventArgs e)=>ShowTools("events",_detail?.Entity.Kind=="Club"?_detail.Entity.Id:null);
 private void History_Click(object sender,RoutedEventArgs e)=>ShowTools("history");
 private async void Export_Click(object sender,RoutedEventArgs e) {if(_busy||_bridge==null||!ConfirmAbilityExport())return;var d=new SaveFileDialog {Title="현재 목록 스냅샷 저장",Filter="JSON 스냅샷|*.json",FileName="touchline-snapshot.json"};if(d.ShowDialog(this)==true)await Guard(async()=> {await _bridge.CallAsync("export_snapshot",new{path=d.FileName});Status("스냅샷 저장 완료: "+d.FileName);},true);}
 private void CloseTools() {foreach(var w in _toolWindows.ToArray())w.Close();}
 private void Minimize_Click(object sender,RoutedEventArgs e)=>WindowState=WindowState.Minimized;
 private void Maximize_Click(object sender,RoutedEventArgs e)=>WindowState=WindowState==WindowState.Maximized?WindowState.Normal:WindowState.Maximized;
 private void Close_Click(object sender,RoutedEventArgs e)=>Close();
 private async void OnClosing(object? sender,CancelEventArgs e) {if(_closing)return;e.Cancel=true;if((_busy||_livePollBusy)&&!_qaRunning){Status("진행 중인 작업이 완료된 뒤 닫아주세요.");return;}if(!CanLeave())return;await CloseSafely();}
 private async Task CloseSafely() {if(_closing)return;try{await CloseWorkbenches();}catch(OperationCanceledException){return;}DisableLiveWatch();await StopDetailViewsAsync();await DisposeSearchView();_closing=true;_ready=false;_searchTimer.Stop();_scoreTimer.Stop();CloseTools();if(_bridge!=null)await _bridge.DisposeAsync();SaveDesktopWindow();LaunchPendingUpdate();Close();}
 private void Updates_Click(object sender,RoutedEventArgs e) => OpenUpdateSettings();

 private async Task RunUiCheck()
 {
  if(_bridge==null||_qaOutput==null)return;
  var dir=System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(_qaOutput))!;Directory.CreateDirectory(dir);
  if(App.Arg("--entity") is string uid&&uint.TryParse(uid,out var id))await OpenEntity(App.Arg("--kind")??"Player",id);
  if(App.Arg("--tab") is string tab){_tab=tab;await RenderDetail();}
  await Task.Delay(400);UpdateLayout();Capture(_qaOutput);
  await CaptureLayoutEvidenceAsync(_qaOutput);
  _qaChecks.Add("native_wpf_rendered");
  RunUsabilityQa(dir);
  if(App.Arg("--detailed-search-check")=="true") await RunDetailedSearchQaAsync(dir);
  if(App.Arg("--connection-ui-check")=="true") await RunConnectionPresentationQaAsync(dir);
  if(App.Arg("--ability-visibility-check")=="true") await RunAbilityVisibilityQaAsync(dir);
  if(App.Arg("--smoke")=="all") {
   var original=_detail?.Entity;
   if(_detail!=null) {var field=_detail.Fields.First(f=>f.Editable&&f.Max>f.Value);var before=field.Original;field.Adjust(1);if(!field.IsDirty)throw new Exception("edit did not mark dirty");var p=await _bridge.CallAsync("preview",new{kind=_detail.Entity.Kind,id=_detail.Entity.Id,edits=new[]{new{key=field.Key,value=field.Value}}});if(p.GetProperty("can_apply").GetBoolean())throw new Exception("offline preview unexpectedly writable");await ShowReview(p);field.Reset();if(field.IsDirty||field.Value!=before)throw new Exception("reset lost precise original");_qaChecks.Add("edit_preview_and_reset_preserve_original");}
   if(_ownClub!=null) {await OpenEntity("Club",_ownClub.Id);_tab="선수단";await RenderDetail();UpdateLayout();Capture(System.IO.Path.Combine(dir,"wpf-club.png"));_qaChecks.Add("club_roster_links");
    var staff=await _bridge.CallAsync("search",new{kind="Staff",club_id=_ownClub.Id,limit=1,sort="ca",descending=true});var first=staff.GetProperty("items").EnumerateArray().First();await OpenEntity("Staff",first.GetProperty("id").GetUInt32());_tab="직무 점수";await RenderDetail();UpdateLayout();Capture(System.IO.Path.Combine(dir,"wpf-staff.png"));_qaChecks.Add("staff_27_scores");
    var overlayData=await _bridge.CallAsync("overlay_detail",new{id=_detail!.Entity.Id});var overlayReport=CoachOverlayWindow.VerifyNativeShell(_bridge,_detail.Entity,overlayData,System.IO.Path.Combine(dir,"wpf-overlay.png"));File.WriteAllText(System.IO.Path.Combine(dir,"wpf-overlay.json"),JsonSerializer.Serialize(overlayReport,new JsonSerializerOptions{WriteIndented=true}));_qaChecks.Add("native_overlay_styles_and_geometry");
    var events=new ToolsWindow(_bridge,"events",_ownClub.Id,null,false){Owner=this};events.Show();var eventReport=await events.QaPreviewAsync();events.UpdateLayout();CaptureWindow(events,System.IO.Path.Combine(dir,"wpf-events.png"));File.WriteAllText(System.IO.Path.Combine(dir,"wpf-events.json"),JsonSerializer.Serialize(eventReport,new JsonSerializerOptions{WriteIndented=true}));events.Close();_qaChecks.Add("event_preview_invalidation_and_offline_apply_guard");
    var journal=new ToolsWindow(_bridge,"history",null,null,false){Owner=this};journal.Show();var historyReport=await journal.QaHistoryAsync();File.WriteAllText(System.IO.Path.Combine(dir,"wpf-history.json"),JsonSerializer.Serialize(historyReport,new JsonSerializerOptions{WriteIndented=true}));journal.Close();_qaChecks.Add("journal_offline_undo_guard");
   }
   if(original!=null)await OpenEntity(original.Kind,original.Id);
  }
  if(App.Arg("--workbench-check")=="true") {
   var workbench=new WorkbenchWindow(_bridge,_live,_detail?.Entity,_ownClub,_=>{}){Owner=this};
   workbench.Show();
   try {var report=await workbench.QaAsync();CaptureWindow(workbench,System.IO.Path.Combine(dir,"wpf-workbench.png"));File.WriteAllText(System.IO.Path.Combine(dir,"wpf-workbench.json"),JsonSerializer.Serialize(report,new JsonSerializerOptions{WriteIndented=true}));_qaChecks.Add("workbench_snapshot_search_compare_preview");}
   finally {await workbench.CloseAsync();}
  }
  if(App.Arg("--search-check")=="true")await RunSearchQaAsync(dir);
  if(App.Arg("--search-columns-check")=="true")await RunSearchColumnsQaAsync(dir);
  if(App.Arg("--nationality-search-check")=="true")await RunNationalitySearchQaAsync(dir);
  if(App.Arg("--league-browser-check")=="true")await RunLeagueBrowserQaAsync(dir);
  if(App.Arg("--football-search-check")=="true")await RunFootballSearchQaAsync(dir);
  if(App.Arg("--position-sort-check")=="true")await RunPositionSortQaAsync(dir);
  if(App.Arg("--relations-check")=="true")await RunRelationsQaAsync(dir);
  if(App.Arg("--staff-review-check")=="true")await RunStaffReviewQaAsync(dir);
  if(App.Arg("--club-roster-check")=="true")await RunClubRosterQaAsync(dir);
  if(_live&&App.Arg("--overlay-live-check")=="true"&&_ownClub!=null) {var result=await _bridge.CallAsync("search",new{kind="Staff",club_id=_ownClub.Id,limit=1});var staff=Decode<EntityRecord>(result.GetProperty("items")[0])!;var report=await CoachOverlayWindow.VerifyLiveAsync(_bridge,staff);File.WriteAllText(System.IO.Path.Combine(dir,"wpf-overlay-live.json"),JsonSerializer.Serialize(report,new JsonSerializerOptions{WriteIndented=true}));_qaChecks.Add("readonly_live_overlay_poll");}
  File.WriteAllText(_qaOutput+".json",JsonSerializer.Serialize(new{success=true,framework="WPF + Rust",browser=false,live=_live,game_writes=false,checks=_qaChecks,width=ActualWidth,height=ActualHeight},new JsonSerializerOptions{WriteIndented=true}));
 }
 private void Capture(string path)=>CaptureWindow(this,path);
 private static void CaptureWindow(Window window,string path) {window.UpdateLayout();var source=new RenderTargetBitmap((int)window.ActualWidth,(int)window.ActualHeight,96,96,PixelFormats.Pbgra32);source.Render(window);var encoder=new PngBitmapEncoder();encoder.Frames.Add(BitmapFrame.Create(source));using var f=File.Create(path);encoder.Save(f);}
}
