using System.Windows;
namespace Touchline.Wpf;
public partial class MainWindow {
 private readonly List<WorkbenchWindow> _workbenches=[];
 private void Workbench_Click(object sender,RoutedEventArgs e) {
  if(_busy||_bridge==null)return;
  if(_workbenches.FirstOrDefault(window=>!window.IsConnectionInvalidated) is {} existing) {if(existing.WindowState==WindowState.Minimized)existing.WindowState=WindowState.Normal;existing.Activate();return;}
  var window=new WorkbenchWindow(_bridge,_live,_detail?.Entity,_ownClub,async entity=>{
   if(CanLeave())await Guard(()=>OpenEntity(entity.Kind,entity.Id),true);
  }){Owner=this};
  _workbenches.Add(window);window.Closed+=(_,_)=>_workbenches.Remove(window);window.Show();
 }
 private async Task CloseWorkbenches() {
  foreach(var w in _workbenches.ToArray()) { if(!await w.CloseAsync())throw new OperationCanceledException("작업 창의 저장하지 않은 내용을 확인한 뒤 다시 시도하세요."); }
  if(_bridge!=null)try{await _bridge.CallAsync("freeze_stop");}catch(Exception ex){App.Log(ex);}
 }
}
