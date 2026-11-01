using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace Touchline.Wpf;

/// <summary>Keeps review actions outside the scrolling changes at every supported UI scale.</summary>
public sealed class ScalarReviewWindow : Window
{
 private readonly Grid _surface = new();
 public FrameworkElement Surface => _surface;
 public Button ApplyButton { get; }
 public Button ReturnButton { get; }
 public ScrollViewer ChangesScroll { get; }
 public bool ApplyRequested { get; private set; }

 public ScalarReviewWindow(JsonElement preview, bool live)
 {
  Title = "변경 검토"; Width = 660; Height = 560; MinWidth = 360; MinHeight = 260;
  WindowStartupLocation = WindowStartupLocation.CenterOwner; ResizeMode = ResizeMode.CanResize;
  Background = Brush("BaseBrush"); Foreground = Brush("InkBrush"); Typography.Initialize(this);
  // Keep padding inside the bounded surface: an outer margin would enlarge the
  // scaled scroll extent and put the right/bottom actions beyond the viewport.
  var layout = new Grid { Margin = new Thickness(20) };
  _surface.Children.Add(layout);
  layout.RowDefinitions.Add(new RowDefinition());
  layout.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

  var body = new StackPanel();
  body.Children.Add(Text("변경 내용 확인", 24, true));
  var guidance = Text(live ? EditApplicationGuidance.BeforeApply : "스냅샷 모드입니다. 실제 적용하려면 FM을 연결해야 합니다.", 12);
  guidance.Margin = new Thickness(0, 4, 0, 18);
  if (live) guidance.ToolTip = EditApplicationGuidance.Detail;
  body.Children.Add(guidance);
  foreach (var change in preview.GetProperty("changes").EnumerateArray())
  {
   string? key = change.TryGetProperty("key", out var fieldKey) ? fieldKey.GetString() : null;
   var row = new StackPanel();
   row.Children.Add(Text(change.GetProperty("label").GetString() ?? "", 13));
   var values = Text($"{AbilityVisibility.Current.Format(key, change.GetProperty("before"))}  →  {AbilityVisibility.Current.Format(key, change.GetProperty("after"))}", 14, true);
   values.Foreground = Brush("AccentBrush"); row.Children.Add(values);
   body.Children.Add(new Border { Background = Brush("PanelBrush"), Padding = new Thickness(12), Margin = new Thickness(0, 0, 0, 6), CornerRadius = new CornerRadius(6), Child = row });
  }
  ChangesScroll = new ScrollViewer { Content = body, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, CanContentScroll = false };
  layout.Children.Add(ChangesScroll);

  var actions = new WrapPanel { HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 12, 0, 0) };
  ReturnButton = Action("돌아가기", false); ReturnButton.Click += (_, _) => Close();
  ApplyButton = Action("백업 후 게임에 적용", true);
  ApplyButton.IsEnabled = live && preview.GetProperty("can_apply").GetBoolean()
   && !preview.GetProperty("changes").EnumerateArray().Any(change => AbilityVisibility.Current.IsHidden(change.TryGetProperty("key", out var key) ? key.GetString() : null));
  ApplyButton.Click += (_, _) => { if (ApplyButton.IsEnabled) { ApplyRequested = true; Close(); } };
  actions.Children.Add(ReturnButton); actions.Children.Add(ApplyButton);
  Grid.SetRow(actions, 1); layout.Children.Add(actions); Content = _surface;
 }

 private static Brush Brush(string name) => Application.Current.TryFindResource(name) as Brush ?? Brushes.White;
 private static TextBlock Text(string text, double size, bool bold = false)
 {
  var block = new TextBlock { Text = text, FontSize = size, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 0, 4) };
  if (bold) block.FontWeight = FontWeights.SemiBold;
  else block.SetResourceReference(TextBlock.FontWeightProperty, "AppBodyFontWeight");
  return block;
 }
 private Button Action(string label, bool primary) => new()
 {
  Content = new TextBlock { Text = label, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center },
  Style = TryFindResource(primary ? "PrimaryButton" : "QuietButton") as Style,
  HorizontalContentAlignment = HorizontalAlignment.Center,
  MinWidth = primary ? 170 : 90,
  Margin = new Thickness(4, 3, 0, 3)
 };
}
