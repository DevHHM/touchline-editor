using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace Touchline.Wpf;

/// <summary>A read-only introduction; navigation never connects to or changes FM.</summary>
public sealed class TutorialWindow : Window
{
 private sealed record Step(string Label, string Title, string Body, string Hint);
 private static readonly Step[] Steps =
 [
  new("연결", "FM 세이브를 불러오고 연결하세요", "FM에서 세이브를 먼저 불러온 뒤, 에디터 왼쪽 아래의 ‘FM 연결’을 누르세요. 연결이 끝나면 현재 게임 정보를 볼 수 있습니다.", "세이브 불러오기와 저장은 FM에서 진행합니다. 스냅샷은 저장 당시 정보를 읽기 전용으로 볼 때 사용합니다."),
  new("검색", "찾고 싶은 대상을 열어보세요", "선수·스태프·구단 등을 고르고 이름, 영문 이름 또는 UID로 검색하세요. 목록에서 대상을 선택하면 오른쪽에 상세 정보가 열립니다.", "‘검색·필터’에서 조건을 더 좁힐 수 있습니다. 검색창으로 바로 이동하려면 Ctrl+F를 누르세요."),
  new("변경", "변경 검토 후 적용하세요", "상세 화면에서 편집 가능한 값을 바꾼 뒤 ‘변경 검토’를 누르세요. 대상과 변경 전·후 값을 확인하고 ‘백업 후 게임에 적용’을 눌러야 게임에 반영됩니다. 값 입력과 미리보기만으로는 게임이 바뀌지 않습니다.", EditorFeatureGuidance.Freeze),
  new("기록", "기록과 도움말을 활용하세요", "‘최근 기록’에서 열었던 프로필과 검색을 다시 찾고, ‘변경 이력’에서 적용한 작업을 확인하세요. 오른쪽 위 도움말에서 이 안내도 다시 볼 수 있습니다.", "오류 공유는 오류마다 직접 ‘이 오류 공유’를 눌러 동의한 경우에만 전송합니다. 도움말에서 로컬 진단 로그를 확인하거나 내보낼 수 있습니다.")
 ];
 private readonly TextBlock _progress, _heading, _body, _hint;
 private readonly Button _previous, _next, _openFreeze;
 private readonly Border[] _markers;
 private readonly ScrollViewer _pageScroll;
 private int _stepIndex;
 public FrameworkElement Surface { get; }
 public TutorialExitAction ExitAction { get; private set; }
 public event Action? FreezeNavigationRequested;

 public TutorialWindow()
 {
  Title = "처음 시작하기 · Touchline Editor";
  Width = 640; Height = 500; MinWidth = 440; MinHeight = 400;
  WindowStartupLocation = WindowStartupLocation.CenterOwner;
  Background = Resource("PanelBrush", "#181E29"); Foreground = Resource("InkBrush", "#EBF0F8");
  Typography.Initialize(this);
  var layout = new Grid { Margin = new Thickness(28) };
  layout.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
  layout.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
  layout.RowDefinitions.Add(new RowDefinition());
  layout.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
  Surface = new Border { Background = Background, Child = layout }; Content = Surface;

  var header = new Grid();
  header.ColumnDefinitions.Add(new ColumnDefinition()); header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
  var title = Label("처음 시작하기", 23, true); header.Children.Add(title);
  _progress = Label("", 13); _progress.Foreground = Resource("MutedBrush", "#A8B3C4"); Grid.SetColumn(_progress, 1); header.Children.Add(_progress);
  layout.Children.Add(header);
  var markers = new WrapPanel { Margin = new Thickness(0, 20, 0, 18) };
  _markers = Steps.Select((step, index) => new Border { CornerRadius = new CornerRadius(5), Padding = new Thickness(12, 6, 12, 6), Margin = new Thickness(0, 0, 8, 4), Child = Label($"{index + 1} {step.Label}", 12) }).ToArray();
  foreach (var marker in _markers) markers.Children.Add(marker);
  Grid.SetRow(markers, 1); layout.Children.Add(markers);

  var page = new StackPanel();
  _heading = Label("", 20, true); page.Children.Add(_heading);
  _body = Label("", 14); _body.LineHeight = 24; _body.Margin = new Thickness(0, 14, 0, 0); page.Children.Add(_body);
  _hint = Label("", 12); _hint.LineHeight = 21; _hint.Foreground = Resource("MutedBrush", "#A8B3C4");
  page.Children.Add(new Border { Background = Resource("RaisedBrush", "#202836"), CornerRadius = new CornerRadius(6), Padding = new Thickness(14), Margin = new Thickness(0, 18, 0, 0), Child = _hint });
  _openFreeze = ActionButton("능력치 고정 화면 열기", () => { Close(); FreezeNavigationRequested?.Invoke(); });
  _openFreeze.HorizontalAlignment = HorizontalAlignment.Left;
  _openFreeze.Margin = new Thickness(0, 12, 0, 0);
  page.Children.Add(_openFreeze);
  _pageScroll = new ScrollViewer { Content = page, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
  Grid.SetRow(_pageScroll, 2); layout.Children.Add(_pageScroll);

  var footer = new StackPanel { Margin = new Thickness(0, 20, 0, 0) };
  var note = Label("완료·건너뛰기 후에는 자동 표시하지 않습니다. 도움말에서 다시 볼 수 있습니다.", 11); note.Foreground = Resource("MutedBrush", "#A8B3C4"); note.Margin = new Thickness(0, 0, 0, 14); footer.Children.Add(note);
  var actions = new DockPanel { LastChildFill = false };
  var skip = ActionButton("건너뛰기", () => Finish(TutorialExitAction.Skipped), "QuietButton"); DockPanel.SetDock(skip, Dock.Left); actions.Children.Add(skip);
  var navigation = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right };
  _previous = ActionButton("이전", () => ShowStep(_stepIndex - 1)); _previous.Margin = new Thickness(8, 0, 8, 0); navigation.Children.Add(_previous);
  _next = ActionButton("다음", Next, "PrimaryButton"); _next.IsDefault = true; navigation.Children.Add(_next);
  DockPanel.SetDock(navigation, Dock.Right); actions.Children.Add(navigation); footer.Children.Add(actions);
  Grid.SetRow(footer, 3); layout.Children.Add(footer);
  ShowStep(0);
  ContentRendered += (_, _) => _next.Focus();
  PreviewKeyDown += (_, args) => { if (args.Key == Key.Escape) { args.Handled = true; Close(); } };
 }

 private void Next() { if (_stepIndex == Steps.Length - 1) Finish(TutorialExitAction.Completed); else ShowStep(_stepIndex + 1); }
 private void Finish(TutorialExitAction action) { ExitAction = action; Close(); }
 private void ShowStep(int index)
 {
  _stepIndex = Math.Clamp(index, 0, Steps.Length - 1);
  var step = Steps[_stepIndex];
  _progress.Text = $"{_stepIndex + 1} / {Steps.Length}";
  _heading.Text = step.Title; _body.Text = step.Body; _hint.Text = step.Hint;
  _openFreeze.Visibility = _stepIndex == 2 ? Visibility.Visible : Visibility.Collapsed;
  _pageScroll.ScrollToTop();
  AutomationProperties.SetName(_heading, $"{_progress.Text} · {step.Title}");
  _previous.IsEnabled = _stepIndex > 0; _next.Content = _stepIndex == Steps.Length - 1 ? "완료" : "다음";
  for (int i = 0; i < _markers.Length; ++i)
  {
   _markers[i].Background = Resource(i == _stepIndex ? "AccentBrush" : "RaisedBrush", i == _stepIndex ? "#75DFC4" : "#202836");
   ((TextBlock)_markers[i].Child).Foreground = i == _stepIndex ? ColorBrush("#112A25") : Resource("MutedBrush", "#A8B3C4");
  }
 }
 private static TextBlock Label(string text, double size, bool bold = false)
 {
  var label = new TextBlock { Text = text, FontSize = size, FontWeight = FontWeights.SemiBold, TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
  if (!bold) label.SetResourceReference(TextBlock.FontWeightProperty, "AppBodyFontWeight");
  return label;
 }
 private static Button ActionButton(string text, Action action, string? style = null)
 {
  var button = new Button { Content = text, MinWidth = 88, Padding = new Thickness(16, 9, 16, 9), HorizontalContentAlignment = HorizontalAlignment.Center };
  if (style != null && Application.Current?.TryFindResource(style) is Style resource) button.Style = resource;
  AutomationProperties.SetName(button, text); button.Click += (_, _) => action(); return button;
 }
 private static Brush Resource(string name, string fallback) => Application.Current?.TryFindResource(name) as Brush ?? ColorBrush(fallback);
 private static Brush ColorBrush(string value) => new SolidColorBrush((Color)ColorConverter.ConvertFromString(value));

 /// <summary>Renders all four actual pages without showing a window or reading preferences.</summary>
 public static void RenderPreview(string path)
 {
  Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
  var window = new TutorialWindow();
  var pages = new List<object>();
  try
  {
   for (int index = 0; index < Steps.Length; ++index)
   {
    window.ShowStep(index);
    window.Surface.Measure(new Size(640, 500)); window.Surface.Arrange(new Rect(0, 0, 640, 500)); window.Surface.UpdateLayout();
    var bitmap = new RenderTargetBitmap(640, 500, 96, 96, PixelFormats.Pbgra32); bitmap.Render(window.Surface);
    var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap));
    string output = index == 0 ? path : Path.Combine(Path.GetDirectoryName(path)!, Path.GetFileNameWithoutExtension(path) + $"-{index + 1}" + Path.GetExtension(path));
    using (var stream = File.Create(output)) encoder.Save(stream);
    bool valid = window._previous.IsEnabled == (index > 0) && window._next.Content.ToString() == (index == Steps.Length - 1 ? "완료" : "다음") && window.ExitAction == TutorialExitAction.None;
    pages.Add(new { step = index + 1, title = window._heading.Text, output, valid });
    if (!valid) throw new InvalidOperationException("Tutorial navigation state is invalid.");
   }
   window.ShowStep(0); window.Next(); window.ShowStep(window._stepIndex - 1);
   if (window._stepIndex != 0 || window.ExitAction != TutorialExitAction.None) throw new InvalidOperationException("Tutorial navigation dismissed the introduction.");
  }
  finally { window.Close(); }
  File.WriteAllText(Path.ChangeExtension(path, ".json"), JsonSerializer.Serialize(new { success = true, shown = false, preferencesAccessed = false, pages }, new JsonSerializerOptions { WriteIndented = true }));
 }
}
