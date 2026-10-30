using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private PersonRelationsView? _relationsView;
 private async Task<bool> BeforeHistoryUndoAsync()
 {
  if(_busy||!CanLeave())return false;
  SetBusy(true,"변경 기록 되돌리기 · 원본 확인 중…");
  try {
   if(_detail!=null)foreach(var field in _detail.Fields)field.Reset();
   await StopDetailViewsAsync();
   return true;
  } catch {SetBusy(false);throw;}
 }
 private async Task AfterHistoryUndoAsync()
 {
  try {
   if(_closing)return;
   if(!await CheckRelationConnectionAsync("관계 변경 결과를 확인하지 못해 연결이 해제되었습니다. 변경 기록을 확인하고 FM에 다시 연결하세요."))return;
   var current=_detail?.Entity;string selected=_tab;
   InvalidateRelationSearchResults();
   await SearchAsync();
   if(SearchScreen.IsVisible)await ShowSearchAsync(_kind);
   else if(current!=null){await OpenEntity(current.Kind,current.Id,false);_tab=selected;await RenderDetail();}
  } finally {SetBusy(false);}
 }
 private void InvalidateRelationSearchResults()
 {
  _searchView?.InvalidateResults();
  foreach(var workbench in _workbenches.ToArray())workbench.InvalidateResults();
 }
 private async Task<bool> CheckRelationConnectionAsync(string message)
 {
  if(_bridge==null)return false;
  var summary=await _bridge.CallAsync("summary");
  if(summary.TryGetProperty("connected",out var connected)&&connected.ValueKind==JsonValueKind.True)return true;
  _searchTimer.Stop();_scoreTimer.Stop();++_searchVersion;
  // Invalidation cannot depend on closing a window: an unsaved library may
  // legitimately remain open. Mark every view disconnected before awaiting it.
  SetSummary(summary);
  foreach(var workbench in _workbenches.ToArray())workbench.InvalidateConnection();
  foreach(var tool in _toolWindows.ToArray())tool.InvalidateConnection();
  _searchView?.InvalidateResults();
  await DisposeSearchView();await StopDetailViewsAsync();ClearDetail();
  _routes.Clear();_routeIndex=-1;_offset=0;_total=0;_listKind=null;
  EntityGrid.ItemsSource=null;ResultCount.Text="0명";PageLabel.Text="0 / 0";
  BackButton.IsEnabled=false;ForwardButton.IsEnabled=false;PrevPage.IsEnabled=false;NextPage.IsEnabled=false;
  OwnBadge.Source=null;Breadcrumb.Text="연결 해제";
  DetailHost.Children.Clear();DetailHost.Children.Add(Text(message,16));Status(message);
  return false;
 }
 private async Task RelationApplyFailedAsync(string message)
 {
  await CheckRelationConnectionAsync(message);
 }
 private static bool IsRelationsTab(string tab) => tab is "커리어 계획" or "선호·관계" or "국적·이름";
 private async Task StopDetailViewsAsync()
 {
  var view=_relationsView;_relationsView=null;
  if(view!=null)await view.StopAsync();
  await StopStaffReviewAsync();
  await StopRosterAsync();
 }
 private async Task ChangeDetailTabAsync(string name)
 {
  if(_busy||_relationsView?.IsBusy==true)return;
  if(_relationsView?.HasPendingChanges==true&&!CanLeave())return;
  if(IsRelationsTab(name)&&_detail?.Fields.Any(field=>field.IsDirty)==true)
  {
   if(!CanLeave())return;
   foreach(var field in _detail.Fields)field.Reset();
  }
  _tab=name;await Guard(RenderDetail,true);
 }
 private async Task RenderRelationsAsync(DetailRecord detail,int renderVersion)
 {
  if(_bridge==null)return;
  string section=_tab switch {"커리어 계획"=>"career","국적·이름"=>"identity",_=>"relations"};
  var view=new PersonRelationsView(_bridge,detail.Entity,_live,section,async target=>
  {
   if(!_busy&&CanLeave())await Guard(()=>OpenEntity(target.Kind,target.Id),true);
  },async()=>
  {
   if(_detail!=detail)return;
   string selected=_tab;
   await Guard(async()=>{InvalidateRelationSearchResults();await SearchAsync();await OpenEntity(detail.Entity.Kind,detail.Entity.Id,false);_tab=selected;await RenderDetail();Status("인물 관계 메모리 반영 확인 · 게임 처리 및 저장 후 유지 여부는 별도 확인이 필요합니다.");},true);
  },RelationApplyFailedAsync);
  _relationsView=view;DetailHost.Children.Add(view);
  await view.LoadAsync();
  if(renderVersion!=_renderVersion||_detail!=detail)view.Invalidate();
 }
 private async Task RunRelationsQaAsync(string directory)
 {
  if(_live)throw new InvalidOperationException("관계 편집 QA는 스냅샷에서만 실행합니다.");
  if(_detail?.Entity.Kind is not ("Player" or "Staff" or "Person"))throw new InvalidOperationException("인물 상세가 필요합니다.");
  var reports=new List<object>();
  foreach(string tab in new[]{"커리어 계획","선호·관계","국적·이름"})
  {
   _tab=tab;await RenderDetail();
   if(_relationsView==null)throw new InvalidOperationException("관계 편집 화면이 연결되지 않았습니다.");
   string section=tab=="커리어 계획"?"career":tab=="선호·관계"?"relations":"identity";
   reports.Add(new{section,report=await _relationsView.QaAsync(Path.Combine(directory,section))});
   await Dispatcher.InvokeAsync(UpdateLayout,DispatcherPriority.Render);await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
   Capture(Path.Combine(directory,$"main-{section}.png"));
  }
  File.WriteAllText(Path.Combine(directory,"relations-ui.json"),JsonSerializer.Serialize(new{success=true,live=false,game_writes=false,reports},new JsonSerializerOptions{WriteIndented=true}));
  _qaChecks.Add("person_relations_career_identity_tabs");
 }
}
