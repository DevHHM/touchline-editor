using System.IO;
using System.Text.Json;
using System.Windows;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private SearchView? _searchView;
 private bool _dataLoaded;
 private string? _lastSearchKind;
 private async Task ShowSearchAsync(string kind)
 {
  if(_bridge==null||!_dataLoaded){Status("FM에 연결하거나 스냅샷을 먼저 열어주세요.");return;}
  _searchTimer.Stop();
  await StopDetailViewsAsync();
  ++_searchVersion;
  ClearDetail();
  LeagueScreen.Visibility=Visibility.Collapsed; _returnToLeague=false;
  if(_searchView==null)
  {
   _searchView=new SearchView(_bridge,_live,_ownClub,async entity=>
   {
    if(_busy||!CanLeave())return;
    await Guard(async()=>{await _searchView!.StopAsync();await OpenEntity(entity.Kind,entity.Id);},true);
   });
   long historyGeneration = _recentHistory.SessionGeneration;
   _searchView.SearchExecuted += state =>
   {
    if (!_dataLoaded || historyGeneration != _recentHistory.SessionGeneration) return;
    _recentHistory.Search(state); SaveRecentHistory();
   };
   SearchScreen.Children.Add(_searchView);
  }
  ContentColumns.Visibility=Visibility.Collapsed;
  SearchScreen.Visibility=Visibility.Visible;
  _kind=kind;_lastSearchKind=kind;SetNavigation();
  Breadcrumb.Text=Label(kind)+" 검색 · 필터";
  SearchScreenButton.Content="검색·필터";
  await _searchView.SetKindAsync(kind);
 }
 private void ShowProfile()
 {
  if(LeagueScreen.IsVisible)_returnToLeague=true;
  LeagueScreen.Visibility=Visibility.Collapsed;
  SearchScreen.Visibility=Visibility.Collapsed;
  ContentColumns.Visibility=Visibility.Visible;
  SearchScreenButton.Content=_returnToLeague?"리그 목록으로":"검색 결과로";
  SetNavigation();
 }
 private async void SearchScreen_Click(object sender,RoutedEventArgs e)
 {
  if(_busy||!CanLeave())return;
  if(_returnToLeague||LeagueScreen.IsVisible){await Guard(()=>ShowLeagueBrowserAsync(),true,"리그 목록 불러오는 중…");return;}
  await Guard(()=>ShowSearchAsync(_lastSearchKind??_kind),true,"검색 필터 불러오는 중…");
 }
 private async Task DisposeSearchView()
 {
  DisposeLeagueBrowser();
  if(_searchView!=null)await _searchView.StopAsync();
  _searchView=null;_lastSearchKind=null;SearchScreen.Children.Clear();SearchScreen.Visibility=Visibility.Collapsed;
  ContentColumns.Visibility=Visibility.Visible;
 }
 private async Task RunSearchQaAsync(string directory)
 {
  await ShowSearchAsync("Staff");
  var result=await _searchView!.QaAsync(directory);
  await ShowSearchAsync("Staff");
  await Dispatcher.InvokeAsync(UpdateLayout,System.Windows.Threading.DispatcherPriority.Render);
  await System.Windows.Threading.Dispatcher.Yield(System.Windows.Threading.DispatcherPriority.ApplicationIdle);
  Capture(Path.Combine(directory,"main-staff-search.png"));
  double originalWidth=Width,originalHeight=Height;
  try {
   Width=1120;Height=740;
   await Dispatcher.InvokeAsync(UpdateLayout,System.Windows.Threading.DispatcherPriority.Render);
   await System.Windows.Threading.Dispatcher.Yield(System.Windows.Threading.DispatcherPriority.ApplicationIdle);
   Capture(Path.Combine(directory,"main-staff-search-1120.png"));
  }finally{Width=originalWidth;Height=originalHeight;}
  File.WriteAllText(Path.Combine(directory,"main-search.json"),JsonSerializer.Serialize(result,new JsonSerializerOptions{WriteIndented=true}));
  _qaChecks.Add("visible_main_staff_search_filters");
 }
 private async Task RunSearchColumnsQaAsync(string directory)
 {
  await ShowSearchAsync("Player");
  var result=await _searchView!.QaColumnsAsync(directory);
  File.WriteAllText(Path.Combine(directory,"search-columns-ui.json"),JsonSerializer.Serialize(result,new JsonSerializerOptions{WriteIndented=true}));
  Capture(Path.Combine(directory,"main-search-columns.png"));
  _qaChecks.Add("search_columns_nationality_picker_persistence");
 }
 private async Task RunDetailedSearchQaAsync(string directory)
 {
  await ShowSearchAsync("Player");
  var result=await _searchView!.QaDetailedSearchAsync(directory);
  File.WriteAllText(Path.Combine(directory,"detailed-search-ui.json"),JsonSerializer.Serialize(result,new JsonSerializerOptions{WriteIndented=true}));
  _qaChecks.Add("typed_detailed_conditions_and_or_position_catalog");
  OpenUpdateSettings();
  try {
   await Task.Delay(180);_updateWindow!.UpdateLayout();CaptureWindow(_updateWindow,Path.Combine(directory,"update-settings.png"));
   _updateWindow.Width=520;_updateWindow.Height=300;await Task.Delay(120);_updateWindow.UpdateLayout();CaptureWindow(_updateWindow,Path.Combine(directory,"update-settings-520x300.png"));
   if(_updateWindow.Content is not System.Windows.Controls.ScrollViewer updateScroll||updateScroll.ScrollableHeight<=0)throw new InvalidOperationException("작은 업데이트 창의 세로 스크롤 검증 실패");
   _qaChecks.Add("update_settings_native_scroll_layout");
  }finally{_updateWindow?.Close();}
 }
 private async Task RunNationalitySearchQaAsync(string directory)
 {
  await ShowSearchAsync("Player");
  var result=await _searchView!.QaNationalityAsync(directory);
  File.WriteAllText(Path.Combine(directory,"nationality-search-ui.json"),JsonSerializer.Serialize(result,new JsonSerializerOptions{WriteIndented=true}));
  var workbench=new WorkbenchWindow(_bridge!,false,null,_ownClub,_=>{}) {Owner=this};workbench.Show();
  try {
   var workbenchResult=await workbench.QaNationalityAsync(directory);
   File.WriteAllText(Path.Combine(directory,"nationality-workbench-ui.json"),JsonSerializer.Serialize(workbenchResult,new JsonSerializerOptions{WriteIndented=true}));
  } finally {await workbench.CloseAsync();}
  _qaChecks.Add("primary_nationality_search_filters");
 }
 private async Task RunPositionSortQaAsync(string directory)
 {
  if(_live)throw new InvalidOperationException("포지션 정렬 QA는 스냅샷으로 실행합니다.");
  var fixturePath=Path.GetDirectoryName(Path.GetFullPath(App.Arg("--snapshot")!))!;
  using var expected=JsonDocument.Parse(File.ReadAllText(Path.Combine(fixturePath,"expected.json")));
  uint[] Ids(string key)=>expected.RootElement.GetProperty(key).EnumerateArray().Select(item=>item.GetUInt32()).ToArray();
  await ShowSearchAsync("Player");
  var search=await _searchView!.QaPositionOrderAsync(directory);
  await OpenEntity("Club",10);_tab="선수단";await RenderDetail();
  var roster=await _rosterView!.QaPositionOrderAsync(directory,Ids("club_player_ascending_ids"),Ids("club_player_descending_ids"));
  var workbench=new WorkbenchWindow(_bridge!,false,null,_ownClub,_=>{}) {Owner=this};workbench.Show();
  object advanced;
  try {advanced=await workbench.QaPositionOrderAsync(directory,Ids("player_ascending_ids"),Ids("player_descending_ids"));}
  finally {await workbench.CloseAsync();}
  File.WriteAllText(Path.Combine(directory,"position-sort-ui.json"),JsonSerializer.Serialize(new {success=true,game_writes=false,search,roster,advanced},new JsonSerializerOptions{WriteIndented=true}));
  _qaChecks.Add("position_sort_tactical_order");
 }

 private async Task RunFootballSearchQaAsync(string directory)
 {
  if(_live)throw new InvalidOperationException("축구 구분 QA는 스냅샷으로 실행합니다.");
  await ShowSearchAsync("Staff");
  var result=await _searchView!.QaFootballCategoryAsync(directory);
  File.WriteAllText(Path.Combine(directory,"football-search-ui.json"),JsonSerializer.Serialize(result,new JsonSerializerOptions{WriteIndented=true}));
  var workbench=new WorkbenchWindow(_bridge!,false,null,_ownClub,_=>{}) {Owner=this};
  workbench.Show();
  try {
   var workbenchResult=await workbench.QaFootballCategoryAsync(directory);
   File.WriteAllText(Path.Combine(directory,"football-workbench-ui.json"),JsonSerializer.Serialize(workbenchResult,new JsonSerializerOptions{WriteIndented=true}));
  } finally {await workbench.CloseAsync();}
  _qaChecks.Add("football_category_search_filters");
 }
}
