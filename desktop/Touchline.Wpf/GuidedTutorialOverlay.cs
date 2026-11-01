using System.ComponentModel;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Shapes;
using System.Windows.Threading;

namespace Touchline.Wpf;

/// <summary>
/// An in-place, read-only tour: the hole reveals the real control but never forwards
/// clicks to it. Targets are measured in this overlay's coordinates, including zoom,
/// scrolling and DPI transforms. No tutorial operation invokes an editor command.
/// </summary>
public sealed class GuidedTutorialOverlay : Grid
{
 private readonly IReadOnlyList<TutorialStep> _steps;
 private readonly Func<string, FrameworkElement?> _resolveTarget;
 private readonly SpotlightVisual _spotlight = new();
 private readonly Canvas _canvas = new();
 private readonly Border _card;
 private readonly Grid _cardLayout;
 private readonly TextBlock _progress, _heading, _body, _hint, _location;
 private readonly ScrollViewer _copyScroll;
 private readonly Button _previous, _next, _skip, _close;
 private readonly Border[] _dots;
 private Window? _owner;
 private DispatcherOperation? _pendingPlacement;
 private Rect _lastTarget = Rect.Empty;
 private Size _lastSize;
 private bool _running, _finished, _contentChanged = true;
 private int _placementGeneration;

 public event Action<TutorialExitAction>? Finished;
 public int StepIndex { get; private set; }
 public int StepCount => _steps.Count;
 public string TargetKey => _steps[StepIndex].TargetKey;
 public bool TargetAvailable { get; private set; }
 public Rect TargetBounds => _spotlight.TargetBounds;
 public Rect CardBounds => new(Canvas.GetLeft(_card), Canvas.GetTop(_card), _card.ActualWidth, _card.ActualHeight);
 public bool MotionEnabled => SystemParameters.ClientAreaAnimation && !SystemParameters.HighContrast;
 public bool IsAnimating => _spotlight.HasAnimatedProperties || _card.HasAnimatedProperties;
 public bool IsStopped => !_running;

 public GuidedTutorialOverlay(IReadOnlyList<TutorialStep> steps, Func<string, FrameworkElement?> resolveTarget)
 {
  if (steps.Count == 0) throw new ArgumentException("A tour needs at least one step.", nameof(steps));
  _steps = steps; _resolveTarget = resolveTarget;
  Background = Brushes.Transparent; ClipToBounds = true; Focusable = true;
  SetResourceReference(TextElement.FontFamilyProperty, "AppFont");
  KeyboardNavigation.SetTabNavigation(this, KeyboardNavigationMode.Cycle);
  KeyboardNavigation.SetControlTabNavigation(this, KeyboardNavigationMode.Cycle);
  AutomationProperties.SetName(this, "처음 시작하기 · 실제 버튼 위치 안내");
  Children.Add(_spotlight); Children.Add(_canvas);
  _spotlight.IsHitTestVisible = false;

  _cardLayout = new Grid { Margin = new Thickness(24, 20, 24, 20) };
  for (int i = 0; i < 5; i++) _cardLayout.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
  // Only the explanation scrolls on unusually short windows. Navigation stays visible.
  _cardLayout.RowDefinitions[2].Height = new GridLength(1, GridUnitType.Star);
  _card = new Border
  {
   Background = Resource("PanelBrush", "#181E29"), BorderBrush = Resource("BorderBrush", "#354052"),
   BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(14), Child = _cardLayout,
   UseLayoutRounding = true, SnapsToDevicePixels = true
  };
  Canvas.SetLeft(_card, 12); Canvas.SetTop(_card, 12); _canvas.Children.Add(_card);
  KeyboardNavigation.SetTabNavigation(_card, KeyboardNavigationMode.Cycle);

  var header = new Grid { Margin = new Thickness(0, 0, 0, 16) };
  header.ColumnDefinitions.Add(new ColumnDefinition()); header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
  header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
  var identity = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
  identity.Children.Add(new Border { Background = Resource("AccentBrush", "#75DFC4"), Width = 6, Height = 18, CornerRadius = new CornerRadius(3), Margin = new Thickness(0, 0, 9, 0) });
  identity.Children.Add(Text("처음 시작하기", 12, true)); header.Children.Add(identity);
  _progress = Text("", 12); _progress.Foreground = Resource("MutedBrush", "#A8B3C4"); Grid.SetColumn(_progress, 1); header.Children.Add(_progress);
  _close = Button("✕", () => Dismiss(TutorialExitAction.None), false, 28); _close.Padding = new Thickness(5); _close.Margin = new Thickness(10, -5, -5, -5);
  _close.ToolTip = "안내 닫기 (Esc)"; AutomationProperties.SetName(_close, "시작 안내 닫기"); Grid.SetColumn(_close, 2); header.Children.Add(_close);
  _cardLayout.Children.Add(header);

  _heading = Text("", 21, true); _heading.LineHeight = 29; _heading.Margin = new Thickness(0, 0, 0, 12);
  AutomationProperties.SetLiveSetting(_heading, AutomationLiveSetting.Polite);
  Grid.SetRow(_heading, 1); _cardLayout.Children.Add(_heading);

  var copy = new StackPanel();
  _body = Text("", 14); _body.LineHeight = 23; copy.Children.Add(_body);
  _hint = Text("", 12); _hint.LineHeight = 20; _hint.Foreground = Resource("MutedBrush", "#A8B3C4"); _hint.Margin = new Thickness(0, 12, 0, 0); copy.Children.Add(_hint);
  _copyScroll = new ScrollViewer { Content = copy, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, Focusable = false, CanContentScroll = false };
  Grid.SetRow(_copyScroll, 2); _cardLayout.Children.Add(_copyScroll);

  var guidance = new StackPanel { Margin = new Thickness(0, 16, 0, 16) };
  _location = Text("강조된 버튼의 위치를 확인하세요", 11); _location.Foreground = Resource("AccentBrush", "#75DFC4"); _location.LineHeight = 18;
  guidance.Children.Add(_location);
  var dots = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 12, 0, 0) };
  _dots = steps.Select(_ => new Border { Width = 20, Height = 3, Margin = new Thickness(0, 0, 5, 0), CornerRadius = new CornerRadius(1.5) }).ToArray();
  foreach (var dot in _dots) dots.Children.Add(dot);
  guidance.Children.Add(dots); Grid.SetRow(guidance, 3); _cardLayout.Children.Add(guidance);

  var navigation = new Grid();
  navigation.ColumnDefinitions.Add(new ColumnDefinition()); navigation.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto }); navigation.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
  _skip = Button("건너뛰기", () => Dismiss(TutorialExitAction.Skipped), false, 70); _skip.HorizontalAlignment = HorizontalAlignment.Left;
  _skip.ToolTip = "다음 실행부터 자동 표시하지 않기"; navigation.Children.Add(_skip);
  _previous = Button("이전", () => GoToStep(StepIndex - 1), false, 64); _previous.Margin = new Thickness(0, 0, 8, 0); Grid.SetColumn(_previous, 1); navigation.Children.Add(_previous);
  _next = Button("다음  →", Next, true, 88); Grid.SetColumn(_next, 2); navigation.Children.Add(_next);
  Grid.SetRow(navigation, 4); _cardLayout.Children.Add(navigation);
  PreviewKeyDown += OnPreviewKeyDown;
  // Consuming input on the opaque hit surface prevents accidental game actions even
  // in the transparent spotlight. The visual hole is intentionally not interactive.
  PreviewMouseDown += (_, args) => { if (!IsWithinCard(args.OriginalSource as DependencyObject)) { args.Handled = true; _next.Focus(); } };
  PreviewMouseWheel += (_, args) => { if (!IsWithinCard(args.OriginalSource as DependencyObject)) args.Handled = true; };
  Unloaded += (_, _) => Stop();
 }

 public void Start()
 {
  if (_running || _finished) return;
  _running = true; _owner = Window.GetWindow(this);
  SizeChanged += OnSizeChanged; LayoutUpdated += OnLayoutUpdated;
  SystemParameters.StaticPropertyChanged += OnSystemPreferencesChanged;
  if (_owner != null) { _owner.Activated += OnOwnerActivation; _owner.Deactivated += OnOwnerActivation; _owner.StateChanged += OnOwnerActivation; }
  GoToStep(0, false);
  _next.Focus();
 }

 public void GoToStep(int index, bool animate = true)
 {
  if (_finished) return;
  StepIndex = Math.Clamp(index, 0, _steps.Count - 1);
  var step = _steps[StepIndex]; _progress.Text = $"{StepIndex + 1} / {_steps.Count}";
  _heading.Text = step.Title; _body.Text = step.Body; _hint.Text = step.Hint;
  _previous.IsEnabled = StepIndex > 0; _next.Content = StepIndex == _steps.Count - 1 ? "시작하기  ✓" : "다음  →";
  AutomationProperties.SetName(_next, StepIndex == _steps.Count - 1 ? "시작 안내 완료" : "다음 단계");
  AutomationProperties.SetName(_heading, $"{StepIndex + 1}/{_steps.Count} · {step.Label} · {step.Title}");
  for (int i = 0; i < _dots.Length; i++) _dots[i].Background = i <= StepIndex ? Resource("AccentBrush", "#75DFC4") : Resource("BorderBrush", "#354052");
  _copyScroll.ScrollToTop(); _contentChanged = true;
  var target = _resolveTarget(step.TargetKey);
  // BringIntoView is presentation-only and exposes a real target at large UI zoom.
  // MainWindow restores the user's scroll offsets after the tour ends.
  if (target is { IsVisible: true }) target.BringIntoView();
  Position(animate);
  int generation = ++_placementGeneration;
  _pendingPlacement?.Abort();
  _pendingPlacement = Dispatcher.BeginInvoke(DispatcherPriority.Loaded, () =>
  {
   _pendingPlacement = null;
   if (!_running || generation != _placementGeneration) return;
   Position(animate);
   _next.Focus();
   if (UIElementAutomationPeer.CreatePeerForElement(_heading) is AutomationPeer peer) peer.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged);
  });
 }

 private void Next() { if (StepIndex == _steps.Count - 1) Dismiss(TutorialExitAction.Completed); else GoToStep(StepIndex + 1); }

 public void Dismiss(TutorialExitAction action)
 {
  if (_finished) return;
  _finished = true; Stop(); Finished?.Invoke(action);
 }

 public void Stop()
 {
  _running = false; _placementGeneration++;
  _pendingPlacement?.Abort(); _pendingPlacement = null;
  SizeChanged -= OnSizeChanged; LayoutUpdated -= OnLayoutUpdated;
  SystemParameters.StaticPropertyChanged -= OnSystemPreferencesChanged;
  if (_owner != null) { _owner.Activated -= OnOwnerActivation; _owner.Deactivated -= OnOwnerActivation; _owner.StateChanged -= OnOwnerActivation; _owner = null; }
  _spotlight.StopAnimations();
  _card.BeginAnimation(Canvas.LeftProperty, null); _card.BeginAnimation(Canvas.TopProperty, null); _card.BeginAnimation(OpacityProperty, null);
 }

 private void OnSizeChanged(object sender, SizeChangedEventArgs args) { if (_running) Position(false); }
 private void OnLayoutUpdated(object? sender, EventArgs args) { if (_running) Position(false); }
 private void OnOwnerActivation(object? sender, EventArgs args) => UpdatePulse();
 private void OnSystemPreferencesChanged(object? sender, PropertyChangedEventArgs args)
 {
  if (args.PropertyName is not (nameof(SystemParameters.ClientAreaAnimation) or nameof(SystemParameters.HighContrast))) return;
  if (!MotionEnabled)
  {
   _spotlight.StopAnimations();
   _card.BeginAnimation(Canvas.LeftProperty, null); _card.BeginAnimation(Canvas.TopProperty, null);
  }
  UpdatePulse();
 }
 private void UpdatePulse() => _spotlight.SetPulse(_running && TargetAvailable && MotionEnabled && _owner is { IsActive: true, WindowState: not WindowState.Minimized });

 private void Position(bool animate)
 {
  if (!_running || ActualWidth <= 0 || ActualHeight <= 0) return;
  var viewport = new Rect(0, 0, ActualWidth, ActualHeight);
  var target = _resolveTarget(TargetKey);
  Rect targetBounds = VisibleBounds(target, viewport);
  bool available = !targetBounds.IsEmpty && targetBounds.Width >= 2 && targetBounds.Height >= 2;
  if (!_contentChanged && _lastSize == RenderSize && SameRect(_lastTarget, targetBounds)) return;
  _contentChanged = false; _lastSize = RenderSize; _lastTarget = targetBounds;
  TargetAvailable = available;
  _location.Text = available ? "강조된 위치를 확인하고 ‘다음’을 누르세요 · 버튼은 실행되지 않습니다" : "현재 화면에 버튼이 보이지 않습니다 · 안내만 읽고 다음으로 이동할 수 있습니다";
  double width = Math.Min(390, Math.Max(0, ActualWidth - 24));
  double maxHeight = Math.Max(0, ActualHeight - 24);
  _card.Width = width; _card.Height = double.NaN; _card.MaxHeight = maxHeight;
  _copyScroll.MaxHeight = Math.Max(40, maxHeight - 216);
  _card.Measure(new Size(width, maxHeight));
  double height = Math.Min(maxHeight, _card.DesiredSize.Height);
  _card.Height = height;
  var preferred = TutorialTourPlacement.Place(new(0, 0, ActualWidth, ActualHeight), available ? ToTourRect(targetBounds) : new(ActualWidth / 2, ActualHeight / 2, 0, 0), width, height);
  var cardBounds = new Rect(preferred.X, preferred.Y, preferred.Width, preferred.Height);
  _card.Width = preferred.Width; _card.Height = preferred.Height;
  bool motion = animate && MotionEnabled && !_spotlight.TargetBounds.IsEmpty;
  _spotlight.MoveTo(targetBounds, cardBounds, available, motion);
  MoveCard(cardBounds, motion);
  UpdatePulse();
 }

 private void MoveCard(Rect bounds, bool animate)
 {
  double fromX = Canvas.GetLeft(_card), fromY = Canvas.GetTop(_card);
  _card.BeginAnimation(Canvas.LeftProperty, null); _card.BeginAnimation(Canvas.TopProperty, null);
  Canvas.SetLeft(_card, bounds.X); Canvas.SetTop(_card, bounds.Y);
  if (!animate) return;
  var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
  var duration = TimeSpan.FromMilliseconds(260);
  _card.BeginAnimation(Canvas.LeftProperty, new DoubleAnimation(fromX, bounds.X, duration) { EasingFunction = ease, FillBehavior = FillBehavior.Stop });
  _card.BeginAnimation(Canvas.TopProperty, new DoubleAnimation(fromY, bounds.Y, duration) { EasingFunction = ease, FillBehavior = FillBehavior.Stop });
 }

 private Rect VisibleBounds(FrameworkElement? target, Rect viewport)
 {
  if (target is not { IsVisible: true } || target.ActualWidth <= 0 || target.ActualHeight <= 0) return Rect.Empty;
  try
  {
   Rect bounds = target.TransformToVisual(this).TransformBounds(new Rect(target.RenderSize));
   // An offscreen control must never yield a spotlight over an unrelated button.
   for (DependencyObject? ancestor = VisualTreeHelper.GetParent(target); ancestor != null && ancestor != this; ancestor = VisualTreeHelper.GetParent(ancestor))
   {
    if (ancestor is FrameworkElement element && (element.ClipToBounds || element is ScrollViewer or ScrollContentPresenter))
    {
     bounds.Intersect(element.TransformToVisual(this).TransformBounds(new Rect(element.RenderSize)));
     if (bounds.IsEmpty) return Rect.Empty;
    }
   }
   bounds.Intersect(viewport);
   if (bounds.IsEmpty || bounds.Width < 2 || bounds.Height < 2) return Rect.Empty;
   bounds.Inflate(6, 6); bounds.Intersect(viewport); return bounds;
  }
  catch (InvalidOperationException) { return Rect.Empty; }
 }

 private void OnPreviewKeyDown(object sender, KeyEventArgs args)
 {
  // Preserve the normal window-close chord; all editing/search commands stay blocked.
  if ((args.Key == Key.System ? args.SystemKey : args.Key) == Key.F4 && Keyboard.Modifiers.HasFlag(ModifierKeys.Alt)) return;
  if (args.Key == Key.Escape) { args.Handled = true; Dismiss(TutorialExitAction.None); return; }
  if (Keyboard.Modifiers == ModifierKeys.None && args.Key == Key.Enter)
  {
   args.Handled = true;
   if (Keyboard.FocusedElement == _close) Dismiss(TutorialExitAction.None);
   else if (Keyboard.FocusedElement == _skip) Dismiss(TutorialExitAction.Skipped);
   else if (Keyboard.FocusedElement == _previous) GoToStep(StepIndex - 1);
   else Next();
   return;
  }
  if (Keyboard.Modifiers == ModifierKeys.None && args.Key == Key.Right) { args.Handled = true; Next(); return; }
  if (Keyboard.Modifiers == ModifierKeys.None && args.Key == Key.Left) { args.Handled = true; GoToStep(StepIndex - 1); return; }
  if (args.Key == Key.Tab)
  {
   args.Handled = true;
   var buttons = new[] { _close, _skip, _previous, _next }.Where(button => button.IsEnabled).ToArray();
   int current = Array.IndexOf(buttons, Keyboard.FocusedElement as Button);
   int direction = Keyboard.Modifiers.HasFlag(ModifierKeys.Shift) ? -1 : 1;
   if (current < 0 && direction < 0) current = 0;
   buttons[(current + direction + buttons.Length) % buttons.Length].Focus(); return;
  }
  if (Keyboard.Modifiers.HasFlag(ModifierKeys.Control) || Keyboard.Modifiers.HasFlag(ModifierKeys.Alt) || args.Key is >= Key.F1 and <= Key.F24) args.Handled = true;
 }

 private bool IsWithinCard(DependencyObject? element)
 {
  for (; element != null; element = element is Visual or System.Windows.Media.Media3D.Visual3D ? VisualTreeHelper.GetParent(element) : LogicalTreeHelper.GetParent(element)) if (ReferenceEquals(element, _card)) return true;
  return false;
 }
 private static bool SameRect(Rect left, Rect right) => left.IsEmpty && right.IsEmpty || !left.IsEmpty && !right.IsEmpty && Math.Abs(left.X - right.X) < .4 && Math.Abs(left.Y - right.Y) < .4 && Math.Abs(left.Width - right.Width) < .4 && Math.Abs(left.Height - right.Height) < .4;
 private static TutorialRect ToTourRect(Rect rect) => new(rect.X, rect.Y, rect.Width, rect.Height);
 private static TextBlock Text(string value, double size, bool bold = false) => new() { Text = value, FontSize = size, FontWeight = bold ? FontWeights.SemiBold : FontWeights.Medium, Foreground = Resource("InkBrush", "#EBF0F8"), TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
 private static Button Button(string label, Action action, bool primary, double minWidth)
 {
  var button = new Button { Content = label, MinWidth = minWidth, MinHeight = 36, Padding = new Thickness(12, 8, 12, 8), HorizontalContentAlignment = HorizontalAlignment.Center, VerticalContentAlignment = VerticalAlignment.Center };
  if (Application.Current?.TryFindResource(primary ? "PrimaryButton" : "QuietButton") is Style style) button.Style = style;
  AutomationProperties.SetName(button, label); button.Click += (_, _) => action(); return button;
 }
 private static Brush Resource(string name, string fallback) => Application.Current?.TryFindResource(name) as Brush ?? new SolidColorBrush((Color)ColorConverter.ConvertFromString(fallback));

 /// <summary>Animation is render-only: no layout thrash from the pulsing outline.</summary>
 private sealed class SpotlightVisual : FrameworkElement
 {
  private static readonly DependencyProperty TargetBoundsProperty = DependencyProperty.Register(nameof(TargetBounds), typeof(Rect), typeof(SpotlightVisual), new FrameworkPropertyMetadata(Rect.Empty, FrameworkPropertyMetadataOptions.AffectsRender));
  private static readonly DependencyProperty CardBoundsProperty = DependencyProperty.Register(nameof(CardBounds), typeof(Rect), typeof(SpotlightVisual), new FrameworkPropertyMetadata(Rect.Empty, FrameworkPropertyMetadataOptions.AffectsRender));
  private static readonly DependencyProperty PulseProperty = DependencyProperty.Register(nameof(Pulse), typeof(double), typeof(SpotlightVisual), new FrameworkPropertyMetadata(0d, FrameworkPropertyMetadataOptions.AffectsRender));
  public Rect TargetBounds { get => (Rect)GetValue(TargetBoundsProperty); private set => SetValue(TargetBoundsProperty, value); }
  private Rect CardBounds { get => (Rect)GetValue(CardBoundsProperty); set => SetValue(CardBoundsProperty, value); }
  private double Pulse { get => (double)GetValue(PulseProperty); set => SetValue(PulseProperty, value); }
  private bool _showTarget, _pulsing;
  private readonly Brush _shade = new SolidColorBrush(Color.FromArgb(205, 6, 10, 17));
  private readonly Color _accent = Color.FromRgb(117, 223, 196);

  public void MoveTo(Rect target, Rect card, bool showTarget, bool animate)
  {
   Rect fromTarget = TargetBounds, fromCard = CardBounds;
   BeginAnimation(TargetBoundsProperty, null); BeginAnimation(CardBoundsProperty, null);
   _showTarget = showTarget; TargetBounds = target; CardBounds = card; InvalidateVisual();
   if (!animate || fromTarget.IsEmpty || target.IsEmpty || fromCard.IsEmpty) return;
   var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
   BeginAnimation(TargetBoundsProperty, new RectAnimation(fromTarget, target, TimeSpan.FromMilliseconds(260)) { EasingFunction = ease, FillBehavior = FillBehavior.Stop });
   BeginAnimation(CardBoundsProperty, new RectAnimation(fromCard, card, TimeSpan.FromMilliseconds(260)) { EasingFunction = ease, FillBehavior = FillBehavior.Stop });
  }
  public void SetPulse(bool enabled)
  {
   if (_pulsing == enabled) return;
   _pulsing = enabled; BeginAnimation(PulseProperty, null); Pulse = 0;
   if (enabled) BeginAnimation(PulseProperty, new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(1250)) { AutoReverse = true, RepeatBehavior = RepeatBehavior.Forever, EasingFunction = new SineEase { EasingMode = EasingMode.EaseInOut } });
  }
  public void StopAnimations()
  {
   SetPulse(false); BeginAnimation(TargetBoundsProperty, null); BeginAnimation(CardBoundsProperty, null);
  }
  protected override void OnRender(DrawingContext drawing)
  {
   base.OnRender(drawing);
   if (ActualWidth <= 0 || ActualHeight <= 0) return;
   var shade = new GeometryGroup { FillRule = FillRule.EvenOdd };
   shade.Children.Add(new RectangleGeometry(new Rect(RenderSize)));
   Rect bounds = TargetBounds;
   if (_showTarget && !bounds.IsEmpty) shade.Children.Add(new RectangleGeometry(bounds, 7, 7));
   drawing.DrawGeometry(_shade, null, shade);
   if (!_showTarget || bounds.IsEmpty) return;
   var ring = new SolidColorBrush(_accent);
   drawing.DrawRoundedRectangle(null, new Pen(ring, 2), bounds, 7, 7);
   var outer = bounds; outer.Inflate(3 + Pulse * 3, 3 + Pulse * 3);
   drawing.DrawRoundedRectangle(null, new Pen(new SolidColorBrush(Color.FromArgb((byte)(72 * (1 - Pulse * .55)), _accent.R, _accent.G, _accent.B)), 3), outer, 10, 10);
   if (CardBounds.IsEmpty || bounds.IntersectsWith(CardBounds)) return;
   Rect card = CardBounds;
   if (card.Width < 40 || card.Height < 40) return;
   Point target, origin;
   if (card.Left >= bounds.Right) { target = new(bounds.Right + 3, bounds.Top + bounds.Height / 2); origin = new(card.Left, Math.Clamp(target.Y, card.Top + 18, card.Bottom - 18)); }
   else if (card.Right <= bounds.Left) { target = new(bounds.Left - 3, bounds.Top + bounds.Height / 2); origin = new(card.Right, Math.Clamp(target.Y, card.Top + 18, card.Bottom - 18)); }
   else if (card.Top >= bounds.Bottom) { target = new(bounds.Left + bounds.Width / 2, bounds.Bottom + 3); origin = new(Math.Clamp(target.X, card.Left + 18, card.Right - 18), card.Top); }
   else { target = new(bounds.Left + bounds.Width / 2, bounds.Top - 3); origin = new(Math.Clamp(target.X, card.Left + 18, card.Right - 18), card.Bottom); }
   drawing.DrawLine(new Pen(new SolidColorBrush(Color.FromArgb(170, _accent.R, _accent.G, _accent.B)), 1.5), origin, target);
   drawing.DrawEllipse(ring, null, target, 3, 3);
  }
 }
}
