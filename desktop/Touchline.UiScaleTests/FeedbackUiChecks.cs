using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;
using Touchline.Wpf;

internal static class FeedbackUiChecks
{
 public static void Run(MainWindow main, List<object> results, Action<bool, string> check)
 {
  int originalScale = UiScale.Percent;
  bool hideCa = AbilityVisibility.Current.HideCa, hidePa = AbilityVisibility.Current.HidePa;
  try
  {
   AbilityVisibility.Current.Set(false, false);
   RunReviewLayouts(results, check);
   RunReviewGuards(results, check);
   RunFreezeEntry(main, results, check);
   RunTutorialFreezeNavigation(results, check);
  }
  finally
  {
   AbilityVisibility.Current.Set(hideCa, hidePa);
   UiScale.Set(originalScale, false);
  }
 }

 private static JsonElement Preview(bool canApply, int count = 30) => JsonSerializer.SerializeToElement(new
 {
  can_apply = canApply,
  changes = Enumerable.Range(0, count).Select(index => new
  {
   key = index == 0 ? "ca" : "pace",
   label = $"{index + 1}. 선수의 현재 능력치와 변경할 목표 값 확인 — 긴 설명도 스크롤해서 확인합니다",
   before = 100 + index,
   after = 101 + index
  }).ToArray()
 });

 private static void Layout(FrameworkElement surface, double width, double height)
 {
  surface.Measure(new Size(width, height));
  surface.Arrange(new Rect(0, 0, width, height));
  surface.UpdateLayout();
 }

 private static void Configure(Window window) => typeof(UiScale)
  .GetMethod("ConfigureWindow", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, [window]);

 private static Rect Bounds(FrameworkElement element, FrameworkElement root) => element
  .TransformToAncestor(root).TransformBounds(new Rect(element.RenderSize));

 private static bool Inside(Rect bounds, double width, double height) => bounds.Width > 0 && bounds.Height > 0
  && bounds.Left >= -0.5 && bounds.Top >= -0.5 && bounds.Right <= width + 0.5 && bounds.Bottom <= height + 0.5;

 private static IEnumerable<FrameworkElement> Descendants(DependencyObject parent)
 {
  for (int index = 0; index < VisualTreeHelper.GetChildrenCount(parent); ++index)
  {
   var child = VisualTreeHelper.GetChild(parent, index);
   if (child is FrameworkElement element) yield return element;
   foreach (var descendant in Descendants(child)) yield return descendant;
  }
 }

 private static bool LabelFits(Button button)
 {
  var labels = Descendants(button).Where(element => element is TextBlock or AccessText).ToArray();
  return labels.Length > 0 && labels.All(element => Inside(Bounds(element, button), button.ActualWidth, button.ActualHeight)
   && element.DesiredSize.Width <= element.ActualWidth + 0.5
   && element.DesiredSize.Height <= element.ActualHeight + 0.5);
 }

 private static void RunReviewLayouts(List<object> results, Action<bool, string> check)
 {
  var dialog = new ScalarReviewWindow(Preview(true), true);
  try
  {
   Configure(dialog);
   var root = (FrameworkElement)dialog.Content;
   foreach (var (width, height) in new[] { (344, 220), (480, 320), (640, 400), (800, 540), (980, 660) })
   foreach (int percent in UiScalePreferences.Presets)
   {
    UiScale.Set(percent, false);
    if (root is ScrollViewer outer) { outer.ScrollToHome(); outer.ScrollToLeftEnd(); }
    dialog.ChangesScroll.ScrollToHome();
    Layout(root, width, height);
    Rect applyBefore = Bounds(dialog.ApplyButton, root), backBefore = Bounds(dialog.ReturnButton, root);
    bool actionsInitiallyReachable = Inside(applyBefore, width, height) && Inside(backBefore, width, height);
    bool labelsVisible = LabelFits(dialog.ApplyButton) && LabelFits(dialog.ReturnButton);
    bool bodyScrollable = dialog.ChangesScroll.ActualHeight > 0 && dialog.ChangesScroll.ScrollableHeight > 0;
    dialog.ChangesScroll.ScrollToEnd();
    Layout(root, width, height);
    Rect applyAfter = Bounds(dialog.ApplyButton, root), backAfter = Bounds(dialog.ReturnButton, root);
    bool footerStable = Math.Abs(applyAfter.X - applyBefore.X) < 0.5 && Math.Abs(applyAfter.Y - applyBefore.Y) < 0.5
     && Math.Abs(backAfter.X - backBefore.X) < 0.5 && Math.Abs(backAfter.Y - backBefore.Y) < 0.5;
    bool rowsReachable = Math.Abs(dialog.ChangesScroll.VerticalOffset - dialog.ChangesScroll.ScrollableHeight) < 1;
    bool noOuterScrolling = root is not ScrollViewer viewport || viewport.ScrollableWidth < 1 && viewport.ScrollableHeight < 1;
    bool success = actionsInitiallyReachable && labelsVisible && bodyScrollable && footerStable && rowsReachable && noOuterScrolling;
    check(success, $"scalar_review_actions_{width}x{height}_{percent}");
    results.Add(new { scenario = "scalar_review_actions", width, height, percent, success, actionsInitiallyReachable,
     labelsVisible, bodyScrollable, footerStable, rowsReachable, noOuterScrolling,
     bodyHeight = dialog.ChangesScroll.ActualHeight, applyX = applyBefore.X, applyY = applyBefore.Y,
     applyWidth = applyBefore.Width, applyHeight = applyBefore.Height,
     outerScrollableWidth = (root as ScrollViewer)?.ScrollableWidth,
     outerScrollableHeight = (root as ScrollViewer)?.ScrollableHeight, shownWindows = 0 });
   }
  }
  finally { dialog.Close(); }
 }

 private static void RunReviewGuards(List<object> results, Action<bool, string> check)
 {
  var blocked = new ScalarReviewWindow(Preview(false, 1), false);
  check(!blocked.ApplyButton.IsEnabled && !blocked.ApplyRequested && !blocked.IsVisible, "scalar_review_snapshot_apply_stays_disabled");
  blocked.ReturnButton.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
  check(!blocked.ApplyRequested, "scalar_review_return_does_not_request_apply");

  var staleLivePreview = new ScalarReviewWindow(Preview(true, 1), false);
  check(!staleLivePreview.ApplyButton.IsEnabled, "scalar_review_snapshot_rejects_even_an_applyable_preview");
  staleLivePreview.Close();
  var unavailableLivePreview = new ScalarReviewWindow(Preview(false, 1), true);
  check(!unavailableLivePreview.ApplyButton.IsEnabled, "scalar_review_live_session_still_honors_backend_apply_block");
  unavailableLivePreview.Close();

  AbilityVisibility.Current.Set(true, false);
  var hidden = new ScalarReviewWindow(Preview(true, 1), true);
  check(!hidden.ApplyButton.IsEnabled && !hidden.ApplyRequested, "scalar_review_hidden_ca_cannot_apply");
  hidden.Close();
  AbilityVisibility.Current.Set(false, false);

  var reviewed = new ScalarReviewWindow(Preview(true, 1), true);
  check(reviewed.ApplyButton.IsEnabled && !reviewed.ApplyRequested && !reviewed.IsVisible, "scalar_review_live_preview_requires_explicit_action");
  reviewed.ApplyButton.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
  check(reviewed.ApplyRequested && !reviewed.IsVisible, "scalar_review_action_only_records_request_without_showing_or_writing");
  results.Add(new { scenario = "scalar_review_guards", cases = 7, shownWindows = 0, engineStarted = false, gameWrites = false });
 }

 private static void RunFreezeEntry(MainWindow main, List<object> results, Action<bool, string> check)
 {
  var navigation = main.FindName("AttributeFreezeButton") as Button;
  check(navigation is { Visibility: Visibility.Visible } && Equals(navigation.Content, "능력치 고정"), "freeze_has_named_discoverable_navigation");
  // Never raise the navigation Click: its production handler displays the window.
  // Detaching the real Workbench tree leaves Loaded unfired and the bridge unused.
  var workbench = new WorkbenchWindow(null!, false, null, null, _ => throw new InvalidOperationException("Detached freeze entry must not open a profile."));
  try
  {
   static T Field<T>(WorkbenchWindow window, string name) => (T)typeof(WorkbenchWindow)
    .GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
   var tabs = Field<TabControl>(workbench, "_tabs");
   int tabCount = tabs.Items.Count;
   workbench.OpenFreeze();
   workbench.OpenFreeze();
   bool actualFreezeTab = tabs.SelectedItem is TabItem selected && Equals(selected.Header, "수치 고정") && tabs.Items.Count == tabCount;
   bool requiresTargets = Field<TextBlock>(workbench, "_freezeSummary").Text.Contains("대상")
    && !Field<Button>(workbench, "_freezeEnable").IsEnabled;
   bool noOperation = !Field<DispatcherTimer>(workbench, "_freezeTimer").IsEnabled
    && typeof(WorkbenchWindow).GetField("_freezeToken", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(workbench) == null
    && !workbench.IsLoaded && !workbench.IsVisible;
   check(actualFreezeTab && requiresTargets && noOperation, "freeze_entry_selects_existing_tab_without_preview_or_engine_work");
   workbench.InvalidateConnection();
   check(!Field<Button>(workbench, "_freezeEnable").IsEnabled && !Field<DispatcherTimer>(workbench, "_freezeTimer").IsEnabled,
    "freeze_invalidated_session_remains_stopped");
   results.Add(new { scenario = "freeze_entry", actualFreezeTab, requiresTargets, noOperation, shownWindows = 0, engineStarted = false, gameWrites = false });
  }
  finally
  {
   workbench.InvalidateConnection();
   workbench.Close();
  }
 }

 private static void RunTutorialFreezeNavigation(List<object> results, Action<bool, string> check)
 {
  var tutorial = new TutorialWindow();
  var workbench = new WorkbenchWindow(null!, false, null, null, _ => throw new InvalidOperationException("Tutorial navigation must not open a profile."));
  int requests = 0;
  tutorial.FreezeNavigationRequested += () => { ++requests; workbench.OpenFreeze(); };
  try
  {
   static T Field<T>(object window, string name) => (T)window.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
   static void Click(Button button) => button.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
   var open = Field<Button>(tutorial, "_openFreeze");
   var next = Field<Button>(tutorial, "_next");
   check(open.Visibility == Visibility.Collapsed && requests == 0, "tutorial_freeze_navigation_is_not_an_automatic_startup_action");
   Click(next); Click(next);
   check(open.Visibility == Visibility.Visible && requests == 0, "tutorial_review_step_offers_explicit_freeze_navigation");
   Click(Field<Button>(tutorial, "_previous"));
   check(open.Visibility == Visibility.Collapsed && requests == 0, "tutorial_back_navigation_never_opens_freeze");
   Click(next); Click(open);
   var selected = Field<TabControl>(workbench, "_tabs").SelectedItem as TabItem;
   bool correctTab = Equals(selected?.Header, "수치 고정");
   bool stopped = !Field<DispatcherTimer>(workbench, "_freezeTimer").IsEnabled
    && !Field<Button>(workbench, "_freezeEnable").IsEnabled
    && typeof(WorkbenchWindow).GetField("_freezeToken", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(workbench) == null;
   check(requests == 1 && correctTab && stopped && !workbench.IsLoaded && !workbench.IsVisible,
    "tutorial_explicit_action_routes_to_existing_freeze_tab_without_preview_enable_tick_or_game_access");
   check(tutorial.ExitAction == TutorialExitAction.None && !tutorial.IsVisible,
    "tutorial_feature_navigation_does_not_mark_onboarding_completed_or_skipped");
   results.Add(new { scenario = "tutorial_freeze_navigation", requests, correctTab, stopped, shownWindows = 0, engineStarted = false, gameWrites = false });
  }
  finally { tutorial.Close(); workbench.InvalidateConnection(); workbench.Close(); }
 }
}
