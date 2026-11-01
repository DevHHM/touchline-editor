using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace Touchline.Wpf;

/// <summary>Public reporting is a decision for this error, never a background/default opt-in.</summary>
public static class DiagnosticsConsentPrompt
{
 public static async Task<bool> AskAsync(SharedDiagnosticReport report,DiagnosticCollector collector,CancellationToken token)
 {
  var app=Application.Current;if(app==null||app.Dispatcher.HasShutdownStarted)return false;
  return await app.Dispatcher.InvokeAsync(async()=>{
   // Do not activate the editor over FM/another app or stack over an existing modal error.
   while(!token.IsCancellationRequested&&!app.Dispatcher.HasShutdownStarted){
    if(app.MainWindow is{IsActive:true,IsEnabled:true,IsVisible:true,WindowState:not WindowState.Minimized} owner && owner is not MainWindow { IsTutorialOpen: true }){
     token.ThrowIfCancellationRequested();var dialog=new ConsentWindow(owner,report,collector);
     using var registration=token.Register(()=>{try{if(!dialog.Dispatcher.HasShutdownStarted)dialog.Dispatcher.BeginInvoke(()=>dialog.Close());}catch{}});
     try{dialog.ShowDialog();return dialog.Accepted&&!token.IsCancellationRequested;}
     finally{dialog.Close();}
    }
    await Task.Delay(300,token);
   }
   return false;
  }).Task.Unwrap();
 }

 private sealed class ConsentWindow:Window
 {
  public bool Accepted{get;private set;}
  public FrameworkElement Surface{get;}
  public Button Decline{get;}
  public Button Share{get;}
  public ConsentWindow(Window? owner,SharedDiagnosticReport report,DiagnosticCollector collector)
  {
   Title="이 오류를 공유할까요?";Owner=owner;ShowInTaskbar=false;WindowStartupLocation=WindowStartupLocation.CenterOwner;
   Width=640;MaxHeight=SystemParameters.WorkArea.Height;SizeToContent=SizeToContent.Height;ResizeMode=ResizeMode.NoResize;WindowStyle=WindowStyle.ToolWindow;
   Background=new SolidColorBrush(Color.FromRgb(20,27,38));Foreground=new SolidColorBrush(Color.FromRgb(231,238,247));FontSize=14;Typography.Initialize(this);
   var panel=new StackPanel{Margin=new Thickness(26)};Surface=new Border{Background=Background,Child=new ScrollViewer{Content=panel,VerticalScrollBarVisibility=ScrollBarVisibility.Auto}};Content=Surface;
   void Add(string text,int size=14,bool bold=false,int top=12)=>panel.Children.Add(new TextBlock{Text=text,FontSize=size,FontWeight=bold?FontWeights.SemiBold:FontWeights.Normal,TextWrapping=TextWrapping.Wrap,Margin=new Thickness(0,top,0,0)});
   Add("이 오류를 공유할까요?",22,true,0);
   Add(collector.Destination=="public"?"동의하면 아래 오류 요약이 공개 GitHub 이슈로 전달되어 누구나 볼 수 있습니다.":"동의하면 아래 오류 요약이 배포자가 설정한 비공개 수집 서버로 전달됩니다.",14,true);
   Add("전송할 전체 항목",14,true);
   panel.Children.Add(new TextBox{Text=report.PayloadPreview(),IsReadOnly=true,AcceptsReturn=true,TextWrapping=TextWrapping.Wrap,VerticalScrollBarVisibility=ScrollBarVisibility.Auto,MaxHeight=220,Margin=new Thickness(0,10,0,0)});
   Add(report.Schema==2?"미리보기의 오류 단계·예외 종류·빌드 출처·판매처는 정해진 목록의 값만 보냅니다. 확인되지 않은 정보는 unknown으로 표시합니다.":"현재 수집 서버와 호환되는 기본 요약입니다. 오류 단계·예외 종류·판매처는 보내지 않습니다.",13);
   if(report.Code=="engine_missing")Add("engine 폴더를 찾지 못한 경우 ZIP 전체를 새 폴더에 압축 해제하고 최상위 Touchline Editor.exe를 실행해 주세요. 개별 실행 파일만 옮기지 마세요.",13);
   Add("위 항목만 보냅니다. 원본 로그·오류 원문·사용자 이름·경로·선수 정보·세이브·검색 내용·요청 인자는 보내지 않습니다.",13);
   Add("수집 서버: "+new Uri(collector.IngestUrl).Host+"\n안내: "+collector.PrivacyUrl+(collector.Destination=="public"?"\n공개 위치: github.com/DevHHM/touchline-editor/issues":""),12);
   Add("이 보고서에만 동의합니다. 전송 실패 시 승인한 보고서만 재시도하며, 도움말에서 취소할 수 있습니다. 이미 공개된 보고서는 자동 삭제되지 않습니다.",13);
   var buttons=new StackPanel{Orientation=Orientation.Horizontal,HorizontalAlignment=HorizontalAlignment.Right,Margin=new Thickness(0,22,0,0)};
   Decline=new Button{Content="공유 안 함",IsDefault=true,IsCancel=true,Padding=new Thickness(18,10,18,10),Margin=new Thickness(0,0,12,0)};
   Share=new Button{Content="이 오류 공유",Padding=new Thickness(18,10,18,10)};
   Decline.Click+=(_,_)=>Close();Share.Click+=(_,_)=>{Accepted=true;Close();};buttons.Children.Add(Decline);buttons.Children.Add(Share);panel.Children.Add(buttons);
   ContentRendered+=(_,_)=>Decline.Focus();
  }
 }

 public static void RenderPreview(string path)
 {
  var report=new SharedDiagnosticReport(1,"touchline-editor",new string('0',32),"2026-10-31T00:00:00Z","0.14.0","client","connection","connection_failed","access_denied",null);
  var window=new ConsentWindow(null,report,new("https://diagnostics.example.test/v1/errors","https://diagnostics.example.test/privacy","public"));
  try{
   bool safe=!window.Accepted&&window.Decline.IsDefault&&window.Decline.IsCancel&&!window.Share.IsDefault&&!window.Share.IsCancel;
   if(!safe)throw new InvalidOperationException("Error sharing must default to No.");
   var surface=window.Surface;surface.Measure(new Size(640,double.PositiveInfinity));surface.Arrange(new Rect(0,0,640,surface.DesiredSize.Height));surface.UpdateLayout();
   var bitmap=new RenderTargetBitmap(640,(int)Math.Ceiling(surface.ActualHeight),96,96,PixelFormats.Pbgra32);bitmap.Render(surface);
   var png=new PngBitmapEncoder();png.Frames.Add(BitmapFrame.Create(bitmap));using(var output=File.Create(path))png.Save(output);
   File.WriteAllText(path+".json",JsonSerializer.Serialize(new{safe_default_no=safe,public_disclosure=true,per_report_only=true,network_accessed=false}));
  }finally{window.Close();}
 }
}
