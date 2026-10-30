using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private ClubStaffReviewView? _staffReviewView;
 private readonly Dictionary<uint,string> _staffSquadSelections = [];
 private UIElement StaffClubHeader(DetailRecord detail)
 {
  var header=new DockPanel{Margin=new Thickness(0,0,0,14)};
  var badge=new Border{Width=52,Height=52,Margin=new Thickness(0,0,14,0),Background=Brush("#202D3A"),CornerRadius=new CornerRadius(7)};
  var photo=LoadImage(detail.ImagePath,100);
  badge.Child=photo!=null?new Image{Source=photo,Stretch=Stretch.Uniform,Margin=new Thickness(6)}:new TextBlock{Text="FC",FontSize=22,FontWeight=FontWeights.Bold,HorizontalAlignment=HorizontalAlignment.Center,VerticalAlignment=VerticalAlignment.Center,Foreground=ResourceBrush("AccentBrush")};
  DockPanel.SetDock(badge,Dock.Left);header.Children.Add(badge);
  var info=new StackPanel{VerticalAlignment=VerticalAlignment.Center};
  info.Children.Add(Text(detail.Entity.Name,22,null,true));
  info.Children.Add(Text($"구단 · ID {detail.Entity.Id}",11,"#929FB3"));info.Children.Add(NameCopy.CreateButtons(detail.Entity,Status));header.Children.Add(info);return header;
 }
 private async Task StopStaffReviewAsync()
 {
  var view=_staffReviewView;_staffReviewView=null;
  if(view!=null)await view.StopAsync();
 }
 private async Task RenderStaffReviewAsync(DetailRecord detail,int renderVersion)
 {
  if(_bridge==null)return;
  var view=new ClubStaffReviewView(_bridge,detail.Entity,_live,async target=>
  {
   if(!_busy&&CanLeave())await Guard(()=>OpenEntity(target.Kind,target.Id),true);
  },_staffSquadSelections.GetValueOrDefault(detail.Entity.Id));
  view.SquadChanged += squad => _staffSquadSelections[detail.Entity.Id] = squad;
  _staffReviewView=view;DetailHost.Children.Add(view);
  await view.LoadAsync();
  if(renderVersion!=_renderVersion||_detail!=detail)view.Invalidate();
 }
 private async Task RunStaffReviewQaAsync(string directory)
 {
  if(_live)throw new InvalidOperationException("스태프 추천 화면 QA는 스냅샷에서 실행합니다.");
  if(_detail?.Entity.Kind!="Club")throw new InvalidOperationException("구단 상세가 필요합니다.");
  _tab="스태프";await RenderDetail();
  if(_staffReviewView==null)throw new InvalidOperationException("구단 스태프 추천 화면이 연결되지 않았습니다.");
  var report=await _staffReviewView.QaAsync(directory);
  await Dispatcher.InvokeAsync(UpdateLayout,DispatcherPriority.Render);
  await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
  Capture(Path.Combine(directory,"main-staff-review.png"));
  File.WriteAllText(Path.Combine(directory,"staff-review-ui.json"),JsonSerializer.Serialize(new{success=true,live=false,game_writes=false,report},new JsonSerializerOptions{WriteIndented=true}));
  _qaChecks.Add("club_staff_replacement_review");
 }
}
