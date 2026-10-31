using System.IO;
using System.Windows;
using System.Windows.Input;

namespace Touchline.Wpf;

public static class UiScale
{
 public static int Percent { get; private set; } = 100;
 public static string SettingsPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "ui-scale.json");
 public static event Action? Changed;

 public static void Initialize()
 {
  Set(App.Arg("--ui-check") == null ? UiScalePreferences.Load(SettingsPath).Percent : 100, false);
  EventManager.RegisterClassHandler(typeof(Window), FrameworkElement.LoadedEvent, new RoutedEventHandler((sender, args) =>
  {
   if (sender is Window window && ReferenceEquals(args.OriginalSource, window)) ConfigureWindow(window);
  }));
  EventManager.RegisterClassHandler(typeof(Window), Keyboard.PreviewKeyDownEvent, new KeyEventHandler((sender, args) =>
  {
   if (sender is CoachOverlayWindow) return;
   var modifiers = Keyboard.Modifiers;
   if (modifiers != ModifierKeys.Control && modifiers != (ModifierKeys.Control | ModifierKeys.Shift)) return;
   int? next = args.Key switch
   {
    Key.Add or Key.OemPlus => UiScalePreferences.Step(Percent, 1),
    Key.Subtract or Key.OemMinus when modifiers == ModifierKeys.Control => UiScalePreferences.Step(Percent, -1),
    Key.D0 or Key.NumPad0 when modifiers == ModifierKeys.Control => 100,
    _ => null
   };
   if (next == null) return;
   args.Handled = true;
   Set(next.Value);
  }));
 }

 internal static void ConfigureWindow(Window window)
 {
  if (window is MainWindow or CoachOverlayWindow || window.Content is not FrameworkElement content || content is UiScaleViewport) return;
  window.Content = null;
  window.Content = new UiScaleViewport
  {
   // Retain the dialog's design minimum. The native window may be smaller on a high-DPI monitor.
   MinimumContentWidth = Math.Max(320, window.MinWidth - 16),
   MinimumContentHeight = Math.Max(200, window.MinHeight - 40),
   Content = content
  };
  // The owner has already been fitted to its monitor, including Windows DPI scaling.
  // Keep secondary windows within that area and let the viewport expose larger content.
  var owner = window.Owner;
  double availableWidth = owner is { ActualWidth: > 0 } ? owner.ActualWidth : SystemParameters.WorkArea.Width;
  double availableHeight = owner is { ActualHeight: > 0 } ? owner.ActualHeight : SystemParameters.WorkArea.Height;
  window.MinWidth = Math.Min(window.MinWidth, Math.Min(640, availableWidth));
  window.MinHeight = Math.Min(window.MinHeight, Math.Min(400, availableHeight));
  window.MaxWidth = Math.Min(window.MaxWidth, availableWidth);
  window.MaxHeight = Math.Min(window.MaxHeight, availableHeight);
  if (double.IsFinite(window.Width)) window.Width = Math.Min(window.Width, availableWidth);
  if (double.IsFinite(window.Height)) window.Height = Math.Min(window.Height, availableHeight);
  if (owner is { IsVisible: true } && window.WindowStartupLocation == WindowStartupLocation.CenterOwner)
  {
   window.Left = owner.Left + Math.Max(0, (owner.ActualWidth - Math.Min(window.ActualWidth, availableWidth)) / 2);
   window.Top = owner.Top + Math.Max(0, (owner.ActualHeight - Math.Min(window.ActualHeight, availableHeight)) / 2);
  }
 }

 public static bool Set(int percent, bool persist = true)
 {
  if (!UiScalePreferences.Presets.Contains(percent)) return false;
  Percent = percent;
  Application.Current.Resources["AppUiScale"] = percent / 100d;
  Changed?.Invoke();
  if (!persist || App.Arg("--ui-check") != null) return true;
  try { new UiScalePreferences { Percent = percent }.Save(SettingsPath); return true; }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException) { App.Log(error); return false; }
 }
}
