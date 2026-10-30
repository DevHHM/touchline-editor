using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private FrameworkElement? _connectionProblemPanel;
 private string? _connectionProblemKey;

 private async Task<JsonElement> EnsureEngineReadyAsync()
 {
  _ready=false;
  var summary=await _bridgeSession.EnsureReadyAsync(EnginePath,message=>Dispatcher.BeginInvoke(()=>Status(message)));
  _ready=true;
  return summary;
 }

 private async Task ShowConnectFailureAsync(Exception error)
 {
  await InvalidateLoadedSessionAsync(error.Message);
  if(!_ready||_bridge==null){ShowConnectionProblem("engine_error",error.Message);return;}
  try {
   var observation=Decode<LiveConnectionObservation>(await _bridge.CallAsync("connection_poll"));
   if(observation!=null){
    _liveReload.Observe(observation);
    if(ConnectionPresentation.IsProblem(observation.State)){ShowConnectionProblem(observation.State,string.IsNullOrWhiteSpace(observation.Reason)?error.Message:observation.Reason);return;}
    if(observation.State is "waiting" or "loading" or "ready"){ShowLiveWait(observation);return;}
   }
  }catch(Exception probeError){App.Log(probeError);}
  ShowConnectionProblem("error",error.Message);
 }

 private void ShowConnectionProblem(string state,string reason)
 {
  LogConnectionState(state,reason);
  var presentation=ConnectionPresentation.For(state,reason);
  ConnectionLabel.Text="○  "+presentation.Label;
  ConnectionLabel.Foreground=ResourceBrush("GoldBrush");
  ConnectButton.Content="FM 다시 연결";
  Status(reason);
  if(_dataLoaded)return; // Preserve a still-valid snapshot while reporting a transport error.
  SearchScreen.Visibility=LeagueScreen.Visibility=Visibility.Collapsed;
  ContentColumns.Visibility=Visibility.Visible;
  string key=state+"\n"+reason;
  if(key==_connectionProblemKey&&_connectionProblemPanel!=null&&DetailHost.Children.Contains(_connectionProblemPanel))return;
  _connectionProblemKey=key;
  DetailHost.Children.Clear();
  var content=new StackPanel{MaxWidth=920,HorizontalAlignment=HorizontalAlignment.Left};
  content.Children.Add(Text(presentation.Title,23,bold:true));
  var guidance=Text(presentation.Guidance,14);guidance.Margin=new Thickness(0,16,0,16);content.Children.Add(guidance);
  content.Children.Add(new TextBox{Text=reason,IsReadOnly=true,AcceptsReturn=true,TextWrapping=TextWrapping.Wrap,VerticalScrollBarVisibility=ScrollBarVisibility.Auto,MinHeight=70,MaxHeight=260,Padding=new Thickness(12),Margin=new Thickness(0,0,0,16)});
  var actions=new WrapPanel();
  actions.Children.Add(Button("다시 연결",Connect_Click,true));
  var copy=Button("진단 내용 복사",(_,_)=>{
   try {Clipboard.SetText(ConnectionPresentation.Report(typeof(App).Assembly.GetName().Version?.ToString(3)??"unknown",state,reason));Status("연결 진단 내용을 복사했습니다.");}
   catch(Exception error){Status("클립보드에 복사하지 못했습니다 · "+error.Message);}
  });
  copy.Margin=new Thickness(10,0,0,8);actions.Children.Add(copy);
  AddDiagnosticActions(actions);
  content.Children.Add(actions);
  var logStatus=Text(DiagnosticsWindow.LocalStatus(),12);logStatus.Margin=new Thickness(0,10,0,0);content.Children.Add(logStatus);
  var version=Text("Touchline Editor "+typeof(App).Assembly.GetName().Version?.ToString(3)+" · 검증된 FM26/FM27 빌드만 연결합니다.",12);version.Margin=new Thickness(0,16,0,0);content.Children.Add(version);
  _connectionProblemPanel=new Border{Background=ResourceBrush("PanelBrush"),BorderBrush=ResourceBrush("BorderBrush"),BorderThickness=new Thickness(1),CornerRadius=new CornerRadius(8),Padding=new Thickness(24),Margin=new Thickness(0,20,0,0),Child=content};
  DetailHost.Children.Add(_connectionProblemPanel);
 }

 private static void LogConnectionState(string state,string reason) => DiagnosticsLog.Current?.Write("connection","state_changed",state+": "+reason,ConnectionPresentation.IsProblem(state)?"error":"info","connection_state");
 private void AddDiagnosticActions(Panel actions)
 {
  var folder=Button("로그 폴더 열기",(_,_)=>DiagnosticsWindow.OpenFolder(this));folder.Margin=new Thickness(10,0,0,8);actions.Children.Add(folder);
  var export=Button("진단 로그 내보내기",(_,_)=>DiagnosticsWindow.ExportLogs(this));export.Margin=new Thickness(10,0,0,8);actions.Children.Add(export);
 }

 private async Task RunConnectionPresentationQaAsync(string directory)
 {
  if(_live)throw new InvalidOperationException("연결 오류 화면 QA는 오프라인에서만 실행합니다.");
  bool loaded=_dataLoaded;
  try {
   _dataLoaded=false;
   foreach(var item in new[]{("error","FM 프로세스 접근 실패: 액세스가 거부되었습니다. (os error 5)"),("unsupported_build","지원하지 않는 FM 빌드입니다 (12345678/19E69000). 이 빌드의 주소 검증이 먼저 필요합니다"),("engine_error","배포본의 engine 폴더가 없습니다. ZIP 전체를 다시 압축 해제하세요.")}){
    ShowConnectionProblem(item.Item1,item.Item2);await Dispatcher.InvokeAsync(UpdateLayout,System.Windows.Threading.DispatcherPriority.Render);
    Capture(Path.Combine(directory,"connection-"+item.Item1+".png"));
    if(_connectionProblemPanel==null||_connectionProblemPanel.ActualHeight<180||ConnectionLabel.Text.Contains("로드 감지"))throw new InvalidOperationException("연결 실패 화면이 표시되지 않았습니다.");
   }
   ShowLiveWait(new(){State="waiting",Reason="Football Manager를 실행하고 세이브를 불러오세요."});
   if(_connectionProblemPanel!=null&&DetailHost.Children.Contains(_connectionProblemPanel))throw new InvalidOperationException("정상 대기 상태로 바뀌었는데 이전 오류 화면이 남았습니다.");
   await Dispatcher.InvokeAsync(UpdateLayout,System.Windows.Threading.DispatcherPriority.Render);Capture(Path.Combine(directory,"connection-waiting.png"));
   _qaChecks.Add("connection_errors_show_reason_retry_and_copy_without_game_or_clipboard_writes");
   DiagnosticsWindow.RenderPreview(Path.Combine(directory,"diagnostics-help.png"));
   DiagnosticsConsentPrompt.RenderPreview(Path.Combine(directory,"diagnostics-consent.png"));
   if(DiagnosticsLog.Current?.IsAvailable!=true)throw new IOException("오프라인 QA 진단 로그가 저장되지 않았습니다.");
   _qaChecks.Add("diagnostics_help_folder_export_controls_and_local_logging");
  }finally{_dataLoaded=loaded;if(_detail!=null)await RenderDetail();}
 }
}
