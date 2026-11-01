using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private async Task RunTutorialTourQaAsync(string directory)
 {
  if (_live || _qaOutput == null) throw new InvalidOperationException("시작 안내 QA는 오프라인 실행에서만 지원합니다.");
  var originalSize = new Size(Width, Height); int originalScale = UiScale.Percent;
  string? originalDraft = _detail == null ? null : JsonSerializer.Serialize(_detail.Fields.Select(f => new { f.Key, f.Value, f.IsDirty }));
  bool connectEnabled = ConnectButton.IsEnabled, searchEnabled = SearchScreenButton.IsEnabled, reviewEnabled = ReviewButton.IsEnabled;
  var reports = new List<object>();
  try
  {
   foreach (var (width, height, scale) in new[] { (1510, 990, 100), (1000, 700, 125), (800, 540, 200), (1920, 1080, 150) })
   {
    Width = width; Height = height; UiScale.Set(scale, persist: false);
    await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded);
    var guide = ShowTutorialOverlay(persistDismissal: false);
    try
    {
     for (int index = 0; index < TutorialTour.Steps.Count; index++)
     {
      guide.GoToStep(index, animate: false);
      await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded);
      await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
      var target = ResolveTutorialTarget(TutorialTour.Steps[index].TargetKey)!;
      if (!guide.TargetAvailable || guide.TargetBounds.Width <= 0 || guide.TargetBounds.Height <= 0)
       throw new InvalidOperationException($"안내 대상이 보이지 않습니다: {width}x{height}/{scale}/{index}.");
      Rect targetRect = guide.TargetBounds, card = guide.CardBounds;
      var viewport = new Rect(0, 0, guide.ActualWidth, guide.ActualHeight);
      if (!viewport.Contains(card) || card.Width < 260 || card.Height < 100)
       throw new InvalidOperationException($"안내가 창을 벗어났습니다: {width}x{height}/{scale}/{index}: {card}, {viewport}.");
      Rect visibleTarget = target.TransformToVisual(guide).TransformBounds(new Rect(target.RenderSize));
      visibleTarget.Intersect(viewport);
      if (visibleTarget.IsEmpty || !targetRect.IntersectsWith(visibleTarget))
       throw new InvalidOperationException("강조 영역이 실제 버튼과 일치하지 않습니다.");
      // Even the transparent spotlight must consume input, instead of clicking FM 연결/변경 검토 underneath.
      var hit = VisualTreeHelper.HitTest(guide, new Point(targetRect.Left + targetRect.Width / 2, targetRect.Top + targetRect.Height / 2));
      if (hit == null) throw new InvalidOperationException("강조 영역에서 실제 버튼으로 입력이 통과합니다.");
      string image = $"tutorial-{width}-{scale}-{index + 1}.png";
      Capture(Path.Combine(directory, image));
      reports.Add(new { width, height, scale, step = index + 1, target = TutorialTour.Steps[index].TargetKey,
       actualControl = target.Name, targetDisabled = !target.IsEnabled, targetRect = RectEvidence(targetRect), card = RectEvidence(card), image });
     }
     guide.GoToStep(0, animate: false);
     SendGuideKey(guide, Key.Enter);
     if (guide.StepIndex != 1) throw new InvalidOperationException("Enter 키가 다음 안내로 이동하지 않았습니다.");
     SendGuideKey(guide, Key.Left);
     if (guide.StepIndex != 0 || !IsTutorialOpen) throw new InvalidOperationException("이전 단계 이동으로 안내가 종료되었습니다.");
     SendGuideKey(guide, Key.F5);
     if (guide.StepIndex != 0 || !IsTutorialOpen) throw new InvalidOperationException("안내 중 편집 단축키가 전달되었습니다.");
     SendGuideKey(guide, Key.Escape);
     if (IsTutorialOpen || !guide.IsStopped || TutorialRoot.Children.Contains(guide))
      throw new InvalidOperationException("시작 안내 종료 시 애니메이션/화면이 정리되지 않았습니다.");
    }
    finally { RemoveTutorialOverlay(restoreFocus: false); }
   }

   // Exercise real WPF animation clocks while the QA window remains off-screen and inactive.
   Width = 1510; Height = 990; UiScale.Set(100, persist: false);
   await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded);
   var animated = ShowTutorialOverlay(persistDismissal: false);
   animated.GoToStep(0, animate: false);
   await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
   var before = animated.TargetBounds;
   animated.GoToStep(1, animate: true);
   bool moved = false, animationClockObserved = false;
   var frames = new List<object>(); var frameBounds = new List<Rect>();
   for (int frame = 0; frame < 9; frame++)
   {
    await Task.Delay(45);
    await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
    Capture(Path.Combine(directory, $"tutorial-animation-{frame:00}.png"));
    moved |= animated.TargetBounds != before;
    animationClockObserved |= animated.IsAnimating;
    frameBounds.Add(animated.TargetBounds);
    frames.Add(new { frame, targetRect = RectEvidence(animated.TargetBounds), card = RectEvidence(animated.CardBounds), animating = animated.IsAnimating });
   }
   if (!moved || animated.StepIndex != 1) throw new InvalidOperationException("단계 이동 강조 애니메이션이 움직이지 않았습니다.");
   bool intermediateFrame = frameBounds.Any(rect => rect != before && rect != animated.TargetBounds);
   if (animated.MotionEnabled && (!animationClockObserved || !intermediateFrame))
    throw new InvalidOperationException("애니메이션 설정이 켜져 있는데 중간 이동 프레임을 확인하지 못했습니다.");
   bool motionEnabled = animated.MotionEnabled;
   animated.GoToStep(animated.StepCount - 1, animate: false);
   SendGuideKey(animated, Key.Right);
   if (IsTutorialOpen || !animated.IsStopped) throw new InvalidOperationException("완료 후 시작 안내가 남았습니다.");
   var skipped = ShowTutorialOverlay(persistDismissal: false);
   Descendants<Button>(skipped).Single(button => button.Content?.ToString() == "건너뛰기")
    .RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Primitives.ButtonBase.ClickEvent));
   if (IsTutorialOpen || !skipped.IsStopped) throw new InvalidOperationException("건너뛰기 후 시작 안내가 남았습니다.");

   var missing = new GuidedTutorialOverlay([TutorialTour.Steps[0]], _ => null);
   Grid.SetRowSpan(missing, 3); Panel.SetZIndex(missing, 1000); TutorialRoot.Children.Add(missing);
   try
   {
    UpdateLayout(); missing.Start(); missing.GoToStep(0, animate: false);
    await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
    if (missing.TargetAvailable) throw new InvalidOperationException("없는 안내 대상에 가짜 강조를 표시했습니다.");
    Capture(Path.Combine(directory, "tutorial-missing-target.png"));
   }
   finally { missing.Stop(); TutorialRoot.Children.Remove(missing); }

   string? afterDraft = _detail == null ? null : JsonSerializer.Serialize(_detail.Fields.Select(f => new { f.Key, f.Value, f.IsDirty }));
   if (originalDraft != afterDraft || connectEnabled != ConnectButton.IsEnabled || searchEnabled != SearchScreenButton.IsEnabled || reviewEnabled != ReviewButton.IsEnabled)
    throw new InvalidOperationException("시작 안내가 초안이나 실제 버튼의 활성화 상태를 바꿨습니다.");
   File.WriteAllText(Path.Combine(directory, "tutorial-tour.json"), JsonSerializer.Serialize(new
   {
    success = true, realTargets = true, steps = TutorialTour.Steps.Count, layoutCases = reports,
    animatedTransition = moved, motionEnabled, animationClockObserved, intermediateFrame, frames,
    keyboardAndPointerGuidanceOnly = true, disabledTargetsRemainDisabled = true,
    closePreviousCompleteSkipCleanup = true, missingTargetFallback = true,
    preferencesAccessed = false, liveGameAccess = false, gameWrites = false
   }, new JsonSerializerOptions { WriteIndented = true }));
   _qaChecks.Add("animated_tour_real_targets_zoom_resize_input_guard_and_cleanup_without_game_or_preferences_writes");
  }
  finally
  {
   RemoveTutorialOverlay(restoreFocus: false);
   Width = originalSize.Width; Height = originalSize.Height; UiScale.Set(originalScale, persist: false);
   await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded);
  }
 }

 private static object RectEvidence(Rect rect) => new { x = rect.X, y = rect.Y, width = rect.Width, height = rect.Height };

 private void SendGuideKey(GuidedTutorialOverlay guide, Key key)
 {
  var source = PresentationSource.FromVisual(this) ?? throw new InvalidOperationException("안내 키보드 검사에 필요한 네이티브 화면이 없습니다.");
  // In-process routed WPF events; no SendKeys, desktop input, focus stealing, or game input.
  var args = new KeyEventArgs(Keyboard.PrimaryDevice, source, Environment.TickCount, key) { RoutedEvent = Keyboard.PreviewKeyDownEvent };
  guide.RaiseEvent(args);
  if (!args.Handled) throw new InvalidOperationException("시작 안내가 키보드 입력을 처리하지 않았습니다: " + key);
 }
}
