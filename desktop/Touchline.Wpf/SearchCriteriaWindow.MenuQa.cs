using System.IO;
using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Automation.Provider;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

public sealed partial class SearchCriteriaWindow
{
 private async Task<object> QaConditionMenusAsync(string directory)
 {
  var saved = ReadDetailedRules();
  var menu = BuildConditionMenu();
  MenuItem? category = null;
  try {
   menu.PlacementTarget = (FrameworkElement)FindName("AddDetailedCondition");
   menu.Placement = PlacementMode.Bottom;
   menu.IsOpen = true;
   await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
   menu.ApplyTemplate(); menu.UpdateLayout();
   var root = menu.Template.FindName("MenuBorder", menu) as Border ?? throw new InvalidOperationException("조건 메뉴 배경 템플릿이 없습니다.");
   if (menu.Items.Count == 0 || root.ActualWidth < 100 || root.ActualHeight < 50) throw new InvalidOperationException("조건 메뉴가 실제로 표시되지 않았습니다.");
   var rootColors = MenuQaContrast(menu.Foreground, root.Background, "기본 조건 메뉴");
   CaptureMenuQa(menu, Path.Combine(directory, "condition-menu-root.png"));

   category = menu.Items.OfType<MenuItem>().FirstOrDefault(item => item.Header?.ToString() == "기본 정보") ?? menu.Items.OfType<MenuItem>().First();
   category.ApplyTemplate();
   var peer = new MenuItemAutomationPeer(category);
   var expand = peer.GetPattern(PatternInterface.ExpandCollapse) as IExpandCollapseProvider ?? throw new InvalidOperationException("조건 분류의 하위 메뉴 열기 동작이 없습니다.");
   expand.Expand();
   await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
   category.UpdateLayout();
   var popup = category.Template.FindName("PART_Popup", category) as Popup ?? throw new InvalidOperationException("조건 하위 메뉴 팝업이 없습니다.");
   var submenu = category.Template.FindName("SubmenuBorder", category) as Border ?? throw new InvalidOperationException("조건 하위 메뉴 배경이 없습니다.");
   if (!category.IsSubmenuOpen || !popup.IsOpen || popup.Child is not FrameworkElement popupChild) throw new InvalidOperationException("조건 하위 메뉴가 열리지 않았습니다.");
   popupChild.UpdateLayout();
   if (submenu.ActualWidth < 100 || submenu.ActualHeight < 50) throw new InvalidOperationException("조건 하위 메뉴가 실제로 표시되지 않았습니다.");
   var leaf = category.Items.OfType<MenuItem>().First();
   leaf.ApplyTemplate(); leaf.UpdateLayout();
   var childColors = MenuQaContrast(leaf.Foreground, submenu.Background, "조건 하위 메뉴");
   var header = FindMenuQaPresenter(leaf) ?? throw new InvalidOperationException("조건 메뉴의 제목이 표시되지 않았습니다.");
   var inheritedColors = MenuQaContrast((Brush)header.GetValue(TextElement.ForegroundProperty), submenu.Background, "실제 제목 글자");
   CaptureMenuQa(popupChild, Path.Combine(directory, "condition-menu-submenu.png"));

   // Opening a submenu highlights its owning native MenuItem without global
   // mouse/keyboard input. Inspect the actual template's highlighted surface.
   var highlightBorder = FindMenuQaHighlight(category);
   var highlightedColors = MenuQaContrast(category.Foreground, highlightBorder.Background, "열린 분류 강조");
   CaptureMenuQa(menu, Path.Combine(directory, "condition-menu-open-highlight.png"));
   if (KeyboardNavigation.GetDirectionalNavigation(menu) != KeyboardNavigationMode.Cycle)
    throw new InvalidOperationException("조건 메뉴의 방향키 순환 설정이 없습니다.");

   int count = _ruleEditors.Count;
   var expected = _ruleCatalog.First(d => d.Category == category.Header?.ToString() && d.Label == leaf.Header?.ToString());
   var invoke = new MenuItemAutomationPeer(leaf).GetPattern(PatternInterface.Invoke) as IInvokeProvider ?? throw new InvalidOperationException("조건 메뉴 실행 동작이 없습니다.");
   invoke.Invoke();
   await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
   if (_ruleEditors.Count != count + 1 || _ruleEditors[^1].Rule.Key != expected.Key)
    throw new InvalidOperationException("하위 메뉴에서 선택한 검색 조건이 추가되지 않았습니다.");
   return new {
    success = true, actualPopupRendered = true, submenuOpens = true,
    automationInvokesCondition = true, directionalNavigation = "Cycle",
    root = rootColors, child = childColors, renderedHeader = inheritedColors, highlighted = highlightedColors,
    rootScreenshot = Path.Combine(directory, "condition-menu-root.png"),
    submenuScreenshot = Path.Combine(directory, "condition-menu-submenu.png"),
    highlightedScreenshot = Path.Combine(directory, "condition-menu-open-highlight.png")
   };
  } finally {
   if (category != null) category.IsSubmenuOpen = false;
   menu.IsOpen = false;
   LoadDetailedRules(saved);
  }
 }

 private static Border FindMenuQaHighlight(MenuItem item)
 {
  foreach (var border in MenuQaDescendants(item).OfType<Border>()) {
   if (border.Background is SolidColorBrush brush && brush.Color.A == 255)
    return border;
  }
  throw new InvalidOperationException("강조된 조건 메뉴의 배경색이 없습니다.");
 }

 private static ContentPresenter? FindMenuQaPresenter(DependencyObject root) => MenuQaDescendants(root).OfType<ContentPresenter>().FirstOrDefault(p => p.ContentSource == "Header" || p.Content is string);

 private static IEnumerable<DependencyObject> MenuQaDescendants(DependencyObject root)
 {
  for (int index = 0; index < VisualTreeHelper.GetChildrenCount(root); index++) {
   var child = VisualTreeHelper.GetChild(root, index); yield return child;
   foreach (var descendant in MenuQaDescendants(child)) yield return descendant;
  }
 }

 private static object MenuQaContrast(Brush foreground, Brush background, string label)
 {
  if (foreground is not SolidColorBrush fg || background is not SolidColorBrush bg || fg.Color.A != 255 || bg.Color.A != 255)
   throw new InvalidOperationException($"{label}: 명시적인 불투명 글자색과 배경색이 필요합니다.");
  static double Channel(byte value) { double n = value / 255.0; return n <= 0.04045 ? n / 12.92 : Math.Pow((n + 0.055) / 1.055, 2.4); }
  static double Luminance(Color color) => 0.2126 * Channel(color.R) + 0.7152 * Channel(color.G) + 0.0722 * Channel(color.B);
  double one = Luminance(fg.Color), two = Luminance(bg.Color);
  double contrast = (Math.Max(one, two) + 0.05) / (Math.Min(one, two) + 0.05);
  if (contrast < 4.5) throw new InvalidOperationException($"{label}: 글자 대비가 부족합니다 ({contrast:F2}:1).");
  return new { foreground = fg.Color.ToString(), background = bg.Color.ToString(), contrastRatio = Math.Round(contrast, 2) };
 }

 private static void CaptureMenuQa(FrameworkElement visual, string path)
 {
  visual.UpdateLayout();
  var bitmap = new RenderTargetBitmap((int)Math.Ceiling(visual.ActualWidth), (int)Math.Ceiling(visual.ActualHeight), 96, 96, PixelFormats.Pbgra32);
  bitmap.Render(visual);
  var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap));
  using var stream = File.Create(path); encoder.Save(stream);
 }
}
