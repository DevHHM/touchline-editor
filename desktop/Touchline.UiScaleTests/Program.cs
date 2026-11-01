using System.IO;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using Touchline.Wpf;

internal static class Program
{
 [STAThread]
 private static int Main(string[] args)
 {
  try { return Run(args); }
  catch (Exception error) { Console.Error.WriteLine(error); return 1; }
 }

 private static int Run(string[] args)
 {
  // No Window.Show, dispatcher loop, engine, or live desktop automation.
  // Exercise real WPF measure/arrange and menu handlers in an unshown visual tree.
  var app = new App();
  typeof(App).GetProperty(nameof(App.Arguments))!.SetValue(null, new[] { "--ui-check", "offline-layout" });
  app.InitializeComponent();
  UiScale.Initialize();
  var window = new MainWindow();
  var root = (FrameworkElement)window.Content;
  T Named<T>(string name) => (T)window.FindName(name);
  var viewport = Named<UiScaleViewport>("WorkspaceViewport");
  var shell = Named<Grid>("WorkspaceShell");
  var search = Named<TextBox>("SearchBox");
  var chooser = Named<Button>("UiScaleButton");
  var options = Named<MenuItem>("UiScaleOptions");
  var results = new List<object>();
  var failures = new List<string>();
  string? preferenceBefore = File.Exists(UiScale.SettingsPath) ? File.ReadAllText(UiScale.SettingsPath) : null;
  static void Layout(FrameworkElement content, double width, double height)
  {
   content.Measure(new Size(width, height)); content.Arrange(new Rect(0, 0, width, height)); content.UpdateLayout();
  }
  void Check(bool value, string name) { if (!value) failures.Add(name); }
  foreach (var (width, height) in new[] { (640, 400), (800, 540), (1120, 740), (1510, 990), (2560, 1440) })
  foreach (int percent in UiScalePreferences.Presets)
  {
   options.Items.OfType<MenuItem>().Single(item => Equals(item.Tag, percent)).RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
   Layout(root, width, height);
   typeof(MainWindow).GetMethod("ApplyResponsiveShell", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, null);
   Layout(root, width, height);
   viewport.ScrollToHorizontalOffset(0); viewport.ScrollToVerticalOffset(0); Layout(root, width, height);
   double scale = percent / 100d;
   Rect Bounds(FrameworkElement element) => element.TransformToAncestor(root).TransformBounds(new Rect(element.RenderSize));
   bool chooserReachable = Bounds(chooser).Left >= 0 && Bounds(chooser).Right <= width && Bounds(chooser).Bottom <= height;
   bool controlsScaled = Math.Abs(Bounds(search).Height - search.ActualHeight * scale) < 1;
   bool menuSelected = options.Items.OfType<MenuItem>().Count(item => item.IsChecked) == 1 && chooser.Content.ToString() == $"화면 {percent}%";
   bool extentCorrect = Math.Abs(viewport.ExtentWidth - shell.ActualWidth * scale) < 2 && Math.Abs(viewport.ExtentHeight - shell.ActualHeight * scale) < 2;
   bool scrollbarsAvailable = (viewport.ScrollableWidth < 1 || viewport.ComputedHorizontalScrollBarVisibility == Visibility.Visible)
    && (viewport.ScrollableHeight < 1 || viewport.ComputedVerticalScrollBarVisibility == Visibility.Visible);
   viewport.ScrollToHorizontalOffset(viewport.ScrollableWidth); viewport.ScrollToVerticalOffset(viewport.ScrollableHeight); Layout(root, width, height);
   bool endReachable = Math.Abs(viewport.HorizontalOffset - viewport.ScrollableWidth) < 1 && Math.Abs(viewport.VerticalOffset - viewport.ScrollableHeight) < 1;
   bool headerSeparated = Bounds(Named<TextBlock>("Breadcrumb")).Right <= Bounds(Named<TextBlock>("GameDateLabel")).Left + 1;
   bool success = chooserReachable && controlsScaled && menuSelected && extentCorrect && scrollbarsAvailable && endReachable && headerSeparated;
   Check(success, $"main_{width}x{height}_{percent}");
   results.Add(new { width, height, percent, success, chooserReachable, controlsScaled, menuSelected, extentCorrect, scrollbarsAvailable, endReachable, headerSeparated, logicalWidth = shell.ActualWidth, logicalHeight = shell.ActualHeight, viewport.ExtentWidth, viewport.ExtentHeight, viewport.ScrollableWidth, viewport.ScrollableHeight });
  }
  var dialog = new SearchCriteriaWindow("Player", [], [], [], null, new SearchCriteria());
  typeof(UiScale).GetMethod("ConfigureWindow", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, [dialog]);
  var dialogRoot = (FrameworkElement)dialog.Content;
  UiScale.Set(150, false); Layout(dialogRoot, 800, 500);
  Check(dialogRoot is UiScaleViewport { Scale: 1.5 } dialogViewport && dialogViewport.ScrollableWidth > 0, "search_dialog_scales_and_scrolls");
  UiScale.Set(100, false); Layout(dialogRoot, 800, 500);
  Check(dialogRoot is UiScaleViewport { Scale: 1 }, "existing_dialog_follows_reset");
  // Detached layout only: Loaded never fires, so no bridge or FM is accessed.
  var shortlist = new ShortlistWindow(null!, [], []);
  typeof(UiScale).GetMethod("ConfigureWindow", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, [shortlist]);
  var shortlistRoot = (UiScaleViewport)shortlist.Content;
  var shortlistGrid = (DataGrid)typeof(ShortlistWindow).GetField("_grid", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(shortlist)!;
  foreach (var (width, height) in new[] { (640, 400), (980, 660), (1510, 990) })
  foreach (int percent in new[] { 80, 100, 125, 150 })
  {
   UiScale.Set(percent, false); Layout(shortlistRoot, width, height);
   bool hasTable = shortlistGrid.ActualHeight >= 70;
   bool scrollReachable = (shortlistRoot.ScrollableWidth < 1 || shortlistRoot.ComputedHorizontalScrollBarVisibility == Visibility.Visible)
    && (shortlistRoot.ScrollableHeight < 1 || shortlistRoot.ComputedVerticalScrollBarVisibility == Visibility.Visible);
   Check(hasTable && scrollReachable, $"shortlist_{width}x{height}_{percent}");
   results.Add(new { scenario = "shortlist", width, height, percent, hasTable, scrollReachable, tableHeight = shortlistGrid.ActualHeight });
  }
  var fixedSizeGrid = new Grid { MinWidth = 640, MinHeight = 480 };
  var autoHeight = new UiScaleViewport { MinimumContentWidth = 640, MinimumContentHeight = 200, Content = fixedSizeGrid };
  var autoWindow = new Window { Content = autoHeight, SizeToContent = SizeToContent.Height };
  UiScale.Set(150, false); autoHeight.Measure(new Size(620, double.PositiveInfinity));
  results.Add(new { scenario = "auto_height_dialog", autoHeight.Scale, desiredWidth = autoHeight.DesiredSize.Width, desiredHeight = autoHeight.DesiredSize.Height, contentDesiredWidth = fixedSizeGrid.DesiredSize.Width, contentDesiredHeight = fixedSizeGrid.DesiredSize.Height });
  Check(double.IsFinite(autoHeight.DesiredSize.Height) && autoHeight.DesiredSize.Height >= 720, "auto_height_dialog_remains_finite_and_readable");
  Check((File.Exists(UiScale.SettingsPath) ? File.ReadAllText(UiScale.SettingsPath) : null) == preferenceBefore, "qa_does_not_change_user_scale_preference");
  var strongText = Named<MenuItem>("StrongTextOption");
  var weightProbe = new TextBlock { Text = "선수 검색 · 현재 능력 180 · 한글 가독성" };
  weightProbe.SetResourceReference(TextBlock.FontWeightProperty, "AppBodyFontWeight");
  shell.Children.Add(weightProbe);
  strongText.IsChecked = true;
  strongText.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
  Layout(root, 1510, 990);
  Check(weightProbe.FontWeight == FontWeights.SemiBold, "readability_strong_applies_without_restart");
  strongText.IsChecked = false;
  strongText.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
  Layout(root, 1510, 990);
  Check(weightProbe.FontWeight == FontWeights.Medium, "readability_default_can_be_restored");
  var captionProbe = new TextBlock { Style = (Style)app.FindResource("Caption") };
  Check(captionProbe.FontSize >= 12, "caption_minimum_readable_size");
  Check(EditApplicationGuidance.Applied.Contains("재시작 없이") && EditApplicationGuidance.Detail.Contains("한글화"), "apply_guidance_distinguishes_live_and_file_edits");
  results.Add(new { scenario = "readability_and_apply_guidance", cases = 4, fontWeight = weightProbe.FontWeight.ToString(), resourceWeight = app.Resources["AppBodyFontWeight"].ToString() });
  shell.Children.Remove(weightProbe);
  Check(!(bool)typeof(MainWindow).GetProperty("TutorialAllowed", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!, "tutorial_qa_window_blocks_onboarding");
  typeof(MainWindow).GetMethod("ShowTutorial", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, null);
  Check(typeof(MainWindow).GetField("_tutorialWindow", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window) == null, "tutorial_qa_replay_does_not_open_a_window");
  string? tutorialPreview = args.Length > 1 ? args[1] : args.Length > 0 ? Path.Combine(Path.GetDirectoryName(Path.GetFullPath(args[0]))!, "tutorial-preview.png") : null;
  RunTutorialChecks(results, Check, tutorialPreview);
  FeedbackUiChecks.Run(window, results, Check);
  string report = JsonSerializer.Serialize(new { success = failures.Count == 0, shownWindows = 0, capturedScreens = 0, engineStarted = false, gameWrites = false, cases = results, failures }, new JsonSerializerOptions { WriteIndented = true });
  if (args.Length > 0) { Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(args[0]))!); File.WriteAllText(args[0], report); }
  Console.WriteLine(report);
  return failures.Count == 0 ? 0 : 1;
 }

 private static void RunTutorialChecks(List<object> results, Action<bool, string> check, string? previewPath)
 {
  static T Field<T>(TutorialWindow window, string name) => (T)typeof(TutorialWindow).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
  static void Layout(FrameworkElement surface, double width, double height)
  { surface.Measure(new Size(width, height)); surface.Arrange(new Rect(0, 0, width, height)); surface.UpdateLayout(); }
  static Button Skip(TutorialWindow window) => ((Grid)((Border)window.Surface).Child).Children.OfType<StackPanel>().Single()
   .Children.OfType<DockPanel>().Single().Children.OfType<Button>().Single();
  static void Click(Button button) => button.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));

  object originalWeight = Application.Current.Resources["AppBodyFontWeight"];
  var typography = new TutorialWindow();
  try
  {
   Application.Current.Resources["AppBodyFontWeight"] = FontWeights.SemiBold;
   check(Field<TextBlock>(typography, "_body").FontWeight == FontWeights.SemiBold && Field<TextBlock>(typography, "_hint").FontWeight == FontWeights.SemiBold,
    "tutorial_honors_strong_text_preference");
   Application.Current.Resources["AppBodyFontWeight"] = FontWeights.Medium;
   check(Field<TextBlock>(typography, "_body").FontWeight == FontWeights.Medium, "tutorial_text_weight_updates_without_reopening");
  }
  finally { Application.Current.Resources["AppBodyFontWeight"] = originalWeight; typography.Close(); }
  var help = new DiagnosticsWindow(() => { });
  var helpPanel = (StackPanel)((ScrollViewer)((Border)help.Surface).Child).Content;
  check(helpPanel.Children.OfType<Button>().Any(button => Equals(button.Content, "처음 시작하기 · 안내 다시 보기")), "tutorial_replay_is_discoverable_in_help");
  help.Close();

  var navigation = new TutorialWindow();
  string initialHeading = Field<TextBlock>(navigation, "_heading").Text;
  check(navigation.ExitAction == TutorialExitAction.None && !Field<Button>(navigation, "_previous").IsEnabled && Field<TextBlock>(navigation, "_progress").Text == "1 / 4", "tutorial_starts_on_first_page_without_dismissal");
  Click(Field<Button>(navigation, "_next"));
  check(Field<TextBlock>(navigation, "_heading").Text != initialHeading && Field<Button>(navigation, "_previous").IsEnabled && navigation.ExitAction == TutorialExitAction.None, "tutorial_next_changes_page_without_dismissal");
  Click(Field<Button>(navigation, "_previous"));
  check(Field<TextBlock>(navigation, "_heading").Text == initialHeading && !Field<Button>(navigation, "_previous").IsEnabled, "tutorial_previous_returns_to_first_page");
  for (int index = 0; index < 3; ++index) Click(Field<Button>(navigation, "_next"));
  check(Field<TextBlock>(navigation, "_progress").Text == "4 / 4" && Equals(Field<Button>(navigation, "_next").Content, "완료") && navigation.ExitAction == TutorialExitAction.None, "tutorial_reaching_last_page_requires_explicit_finish");
  Click(Field<Button>(navigation, "_previous")); Click(Field<Button>(navigation, "_next")); Click(Field<Button>(navigation, "_next"));
  check(navigation.ExitAction == TutorialExitAction.Completed && !navigation.IsVisible, "tutorial_finish_click_records_completion_without_showing_window");
  var skipped = new TutorialWindow(); Click(Field<Button>(skipped, "_next")); Click(Skip(skipped));
  check(skipped.ExitAction == TutorialExitAction.Skipped && !skipped.IsVisible, "tutorial_skip_click_records_explicit_dismissal");
  var closed = new TutorialWindow(); Click(Field<Button>(closed, "_next")); closed.Close();
  check(closed.ExitAction == TutorialExitAction.None && !closed.IsVisible, "tutorial_window_close_does_not_consume_first_launch");
  results.Add(new { scenario = "tutorial_navigation", pages = 4, nextBackTested = true, finishRequiresClick = true, skipTested = true, closePreservesFirstLaunch = true, shownWindows = 0 });

  foreach (var (width, height) in new[] { (440, 400), (640, 500) })
  foreach (int percent in new[] { 80, 100, 150 })
  {
   UiScale.Set(percent, false);
   var tutorial = new TutorialWindow();
   try
   {
    typeof(UiScale).GetMethod("ConfigureWindow", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, [tutorial]);
    var viewport = (UiScaleViewport)tutorial.Content;
    var next = Field<Button>(tutorial, "_next"); var previous = Field<Button>(tutorial, "_previous"); var skip = Skip(tutorial);
    var pageScroll = Field<ScrollViewer>(tutorial, "_pageScroll");
    Rect Bounds(FrameworkElement element) => element.TransformToAncestor(viewport).TransformBounds(new Rect(element.RenderSize));
    bool Reachable(FrameworkElement element)
    {
     var bounds = Bounds(element);
     return bounds.Width > 0 && bounds.Height > 0 && bounds.Left >= -1 && bounds.Top >= -1 && bounds.Right <= viewport.ViewportWidth + 1 && bounds.Bottom <= viewport.ViewportHeight + 1;
    }
    for (int page = 0; page < 4; ++page)
    {
     Layout(viewport, width, height);
     bool scaled = Math.Abs(viewport.Scale - percent / 100d) < .001;
     bool pageReadable = Field<TextBlock>(tutorial, "_heading").ActualHeight > 0 && Field<TextBlock>(tutorial, "_body").ActualWidth > 150 && pageScroll.ViewportHeight >= 45;
     bool navigationCorrect = previous.IsEnabled == (page > 0) && Equals(next.Content, page == 3 ? "완료" : "다음") && tutorial.ExitAction == TutorialExitAction.None;
     bool scrollbarsAvailable = (viewport.ScrollableWidth < 1 || viewport.ComputedHorizontalScrollBarVisibility == Visibility.Visible)
      && (viewport.ScrollableHeight < 1 || viewport.ComputedVerticalScrollBarVisibility == Visibility.Visible);
     pageScroll.ScrollToVerticalOffset(pageScroll.ScrollableHeight); Layout(viewport, width, height);
     bool bodyEndReachable = Math.Abs(pageScroll.VerticalOffset - pageScroll.ScrollableHeight) < 1;
     viewport.ScrollToHorizontalOffset(0); viewport.ScrollToVerticalOffset(viewport.ScrollableHeight); Layout(viewport, width, height);
     bool skipReachable = Reachable(skip);
     viewport.ScrollToHorizontalOffset(viewport.ScrollableWidth); viewport.ScrollToVerticalOffset(viewport.ScrollableHeight); Layout(viewport, width, height);
     bool footerReachable = Reachable(next) && Reachable(previous) && Math.Abs(viewport.VerticalOffset - viewport.ScrollableHeight) < 1;
     bool highScaleScroll = percent != 150 || width != 440 || (viewport.ScrollableWidth > 0 && viewport.ScrollableHeight > 0);
     bool success = scaled && pageReadable && navigationCorrect && scrollbarsAvailable && bodyEndReachable && skipReachable && footerReachable && highScaleScroll;
     check(success, $"tutorial_page_{page + 1}_{width}x{height}_{percent}");
     results.Add(new { scenario = "tutorial", page = page + 1, width, height, percent, success, scaled, pageReadable, navigationCorrect, scrollbarsAvailable, bodyEndReachable, skipReachable, footerReachable, highScaleScroll, viewport.ScrollableWidth, viewport.ScrollableHeight, pageViewportHeight = pageScroll.ViewportHeight });
     if (page < 3) Click(next);
    }
   }
   finally { tutorial.Close(); }
  }
  UiScale.Set(100, false);
  if (previewPath != null)
  {
   TutorialWindow.RenderPreview(previewPath);
   using var previewReport = JsonDocument.Parse(File.ReadAllText(Path.ChangeExtension(previewPath, ".json")));
   bool previewValid = previewReport.RootElement.GetProperty("success").GetBoolean() && !previewReport.RootElement.GetProperty("shown").GetBoolean()
    && !previewReport.RootElement.GetProperty("preferencesAccessed").GetBoolean() && previewReport.RootElement.GetProperty("pages").GetArrayLength() == 4
    && previewReport.RootElement.GetProperty("pages").EnumerateArray().All(page => page.GetProperty("valid").GetBoolean() && File.Exists(page.GetProperty("output").GetString()));
   check(previewValid, "tutorial_all_four_detached_previews_render_without_windows_or_preferences");
   results.Add(new { scenario = "tutorial_previews", path = Path.GetFullPath(previewPath), pages = 4, success = previewValid, shownWindows = 0, preferencesAccessed = false });
  }
 }
}
