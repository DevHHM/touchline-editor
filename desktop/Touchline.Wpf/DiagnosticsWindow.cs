using System.Diagnostics;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Microsoft.Win32;

namespace Touchline.Wpf;

public sealed class DiagnosticsWindow : Window
{
 private readonly TextBlock _remoteStatus;
 public FrameworkElement Surface { get; }
 public DiagnosticsWindow()
 {
  Title="Touchline 도움말 · 진단";Width=700;Height=700;MinWidth=560;MinHeight=500;WindowStartupLocation=WindowStartupLocation.CenterOwner;
  Background=new SolidColorBrush(Color.FromRgb(20,27,38));Foreground=new SolidColorBrush(Color.FromRgb(231,238,247));FontSize=14;Typography.Initialize(this);
  var panel=new StackPanel{Margin=new Thickness(28)};
  var scroll=new ScrollViewer{Content=panel,VerticalScrollBarVisibility=ScrollBarVisibility.Auto};
  Surface=new Border{Background=Background,Child=scroll};Content=Surface;
  panel.Children.Add(Label("도움말 · 진단",24,true));
  panel.Children.Add(Label("Touchline Editor "+(DiagnosticsLog.Current?.Version??typeof(App).Assembly.GetName().Version?.ToString(3)),12));
  panel.Children.Add(Label("로컬 진단 로그",18,true,22));
  panel.Children.Add(Label(LocalStatus(),13,false,10));
  panel.Children.Add(Label("연결 상태와 오류 원인을 이 PC에 기록합니다. 요청 인자·선수 정보·세이브·응답 데이터는 기록하지 않으며, 주소·개인 경로·토큰을 가립니다. 로그는 순환 보관됩니다.",13,false,12));
  var buttons=new WrapPanel{Margin=new Thickness(0,16,0,0)};
  buttons.Children.Add(Action("로그 폴더 열기",()=>OpenFolder(this)));
  buttons.Children.Add(Action("진단 로그 내보내기",()=>ExportLogs(this)));panel.Children.Add(buttons);
  panel.Children.Add(Label("ZIP에는 검증한 진단 로그와 버전·실행 환경만 담습니다. 내보내기는 업로드하지 않습니다.",12,false,8));
  panel.Children.Add(Label("오류별 공유 동의",18,true,24));
  panel.Children.Add(Label("오류가 발생하면 해당 보고서를 공개 GitHub에 공유할지 먼저 묻습니다. 기본 선택은 ‘공유 안 함’이며, ‘이 오류 공유’를 눌러야 그 보고서만 전송합니다. 앞으로 발생할 오류까지 자동으로 동의하는 설정은 없습니다.",13,false,12));
  _remoteStatus=Label(DiagnosticSharing.Configured?DiagnosticSharing.Status:"오류 전송 서버가 아직 설정되지 않았습니다. 로컬 로그는 계속 저장됩니다.",13,false,10);panel.Children.Add(_remoteStatus);
  panel.Children.Add(Label("동의한 경우에만 오류 코드·에디터 버전 등 제한된 진단 정보가 공개 GitHub 이슈로 전달되어 누구나 볼 수 있습니다. 원본 로그·오류 원문·요청 인자·세이브·사용자 경로는 전송하지 않습니다.",12,false,8));
  panel.Children.Add(Label("같은 오류의 동의창은 실행 중 한 번만 표시합니다. 다른 앱을 사용하는 중에는 에디터로 돌아올 때까지 기다립니다. 전송 주소·개인정보 안내·공개 범위가 바뀌면 다시 동의해야 합니다. 이미 공개된 보고서는 자동 삭제되지 않습니다.",12,false,8));
  if(DiagnosticSharing.Configured) {
   var send=Action("승인한 대기 보고 재전송",async()=>{try{await DiagnosticSharing.SendPendingAsync();}catch(Exception error){AppDialog.Show(this,DiagnosticsLog.Sanitize(error.Message),"진단 전송");}});
   send.HorizontalAlignment=HorizontalAlignment.Left;send.Margin=new Thickness(0,12,0,0);panel.Children.Add(send);
   var cancel=Action("이번 실행 공유 중지 · 대기 보고 삭제",()=>{try{DiagnosticSharing.CancelPending();}catch(Exception error){AppDialog.Show(this,DiagnosticsLog.Sanitize(error.Message),"오류 공유 중지");}});
   cancel.HorizontalAlignment=HorizontalAlignment.Left;panel.Children.Add(cancel);
  }
  panel.Children.Add(Label("단축키",18,true,24));panel.Children.Add(Label(EditorShortcuts.HelpText,13,false,12));
  Loaded+=(_,_)=>DiagnosticSharing.StatusChanged+=RefreshRemoteStatus;
  Closed+=(_,_)=>DiagnosticSharing.StatusChanged-=RefreshRemoteStatus;
 }
 private void RefreshRemoteStatus() { if(!Dispatcher.HasShutdownStarted)Dispatcher.BeginInvoke(()=>_remoteStatus.Text=DiagnosticSharing.Status); }
 private static TextBlock Label(string text,double size=14,bool bold=false,double top=4)=>new(){Text=text,FontSize=size,FontWeight=bold?FontWeights.SemiBold:FontWeights.Normal,TextWrapping=TextWrapping.Wrap,Margin=new Thickness(0,top,0,0)};
 private static Button Action(string label,Action run){var button=new Button{Content=label,Padding=new Thickness(16,9,16,9),Margin=new Thickness(0,0,10,8)};button.Click+=(_,_)=>run();return button;}
 public static string LocalStatus()
 {
  var log=DiagnosticsLog.Current;
  return log?.IsAvailable==true?"진단 로그 저장 중 · %LOCALAPPDATA%\\TouchlineEditor\\Logs": "진단 로그를 저장하지 못했습니다 · "+(log?.LastFailure??"로깅이 초기화되지 않았습니다.");
 }
 public static void OpenFolder(Window owner)
 {
  try {
   var log=DiagnosticsLog.Current??throw new IOException("진단 로그가 초기화되지 않았습니다.");
   DiagnosticsLog.RejectReparsePath(log.DirectoryPath);
   if(!Directory.Exists(log.DirectoryPath))throw new IOException(LocalStatus());
   Process.Start(new ProcessStartInfo(log.DirectoryPath){UseShellExecute=true});
  }catch(Exception error){AppDialog.Show(owner,DiagnosticsLog.Sanitize(error.Message),"로그 폴더 열기 실패",MessageBoxButton.OK,MessageBoxImage.Error);}
 }
 public static void ExportLogs(Window owner)
 {
  try {
   var log=DiagnosticsLog.Current??throw new IOException("진단 로그가 초기화되지 않았습니다.");
   var picker=new SaveFileDialog{Title="진단 로그 ZIP 저장",Filter="ZIP 파일 (*.zip)|*.zip",FileName="Touchline-Diagnostics-"+DateTime.Now.ToString("yyyyMMdd-HHmmss-fff")+".zip",OverwritePrompt=false,AddExtension=true};
   if(picker.ShowDialog(owner)!=true)return;
   string path=log.Export(picker.FileName);
   AppDialog.Show(owner,"진단 ZIP을 저장했습니다.\n"+path+"\n\n자동 업로드되지 않았습니다.","진단 로그 내보내기",MessageBoxButton.OK,MessageBoxImage.Information);
  }catch(Exception error){AppDialog.Show(owner,DiagnosticsLog.Sanitize(error.Message),"진단 로그 내보내기 실패",MessageBoxButton.OK,MessageBoxImage.Error);}
 }
 public static void RenderPreview(string path)
 {
  var window=new DiagnosticsWindow();
  try {
   var surface=window.Surface;surface.Measure(new Size(700,700));surface.Arrange(new Rect(0,0,700,700));surface.UpdateLayout();
   var bitmap=new RenderTargetBitmap(700,700,96,96,PixelFormats.Pbgra32);bitmap.Render(surface);
   var png=new PngBitmapEncoder();png.Frames.Add(BitmapFrame.Create(bitmap));using var output=File.Create(path);png.Save(output);
  }finally{window.Close();}
 }
}
