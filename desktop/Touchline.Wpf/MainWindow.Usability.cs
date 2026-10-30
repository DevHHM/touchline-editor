using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Interop;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private double? _preferredListWidth;
 private static string WindowPreferencesPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "window-layout.json");

 private void InitializeUsability()
 {
  PreviewKeyDown += MainWindow_PreviewKeyDown;
  // Automated renders must not move onto a user's display or persist their off-screen position.
  if (_qaOutput != null) return;
  var preferences = DesktopWindowPreferences.Load(WindowPreferencesPath);
  if (preferences == null) return;
  _preferredListWidth = preferences.ValidListWidth;
  if (_preferredListWidth is double width) EntityListColumn.Width = new GridLength(width);
  WindowStartupLocation = WindowStartupLocation.Manual;
  SourceInitialized += (_, _) => RestoreDesktopWindow(preferences);
 }

 private void MainWindow_PreviewKeyDown(object sender, KeyEventArgs e)
 {
  var modifiers = ShortcutModifiers.None;
  if (Keyboard.Modifiers.HasFlag(ModifierKeys.Control)) modifiers |= ShortcutModifiers.Control;
  if (Keyboard.Modifiers.HasFlag(ModifierKeys.Shift)) modifiers |= ShortcutModifiers.Shift;
  if (Keyboard.Modifiers.HasFlag(ModifierKeys.Alt)) modifiers |= ShortcutModifiers.Alt;
  if (Keyboard.Modifiers.HasFlag(ModifierKeys.Windows)) modifiers |= ShortcutModifiers.Windows;
  var shortcut = EditorShortcuts.Resolve((e.Key == Key.System ? e.SystemKey : e.Key).ToString(), modifiers);
  if (shortcut == EditorShortcut.None) return;
  e.Handled = true;
  if (_busy && shortcut != EditorShortcut.Help) return;
  switch (shortcut)
  {
   case EditorShortcut.Search:
    if (SearchScreen.IsVisible) _searchView?.FocusSearch();
    else { SearchBox.Focus(); SearchBox.SelectAll(); }
    break;
   case EditorShortcut.SearchFilters: SearchScreen_Click(this, new()); break;
   case EditorShortcut.Refresh: Refresh_Click(this, new()); break;
   case EditorShortcut.Back: Back_Click(this, new()); break;
   case EditorShortcut.Forward: Forward_Click(this, new()); break;
   case EditorShortcut.Review: Review_Click(this, new()); break;
   case EditorShortcut.Help: ShortcutHelp_Click(this, new()); break;
  }
 }

 private void ShortcutHelp_Click(object sender, RoutedEventArgs e) => AppDialog.Show(this, EditorShortcuts.HelpText, "Touchline 단축키", MessageBoxButton.OK, MessageBoxImage.Information);
 private void ClearSearch_Click(object sender, RoutedEventArgs e)
 {
  if (_busy) return;
  SearchBox.Clear();
  SearchBox.Focus();
 }
 private void EntityListSplitter_DragCompleted(object sender, DragCompletedEventArgs e)
 {
  _preferredListWidth = Math.Clamp(EntityListColumn.ActualWidth, 240, 420);
 }

 private void RestoreDesktopWindow(DesktopWindowPreferences preferences)
 {
  try
  {
   var monitors = ConnectedMonitors();
   if (monitors.Count == 0) return;
   var monitor = preferences.SelectMonitor(monitors);
   var bounds = preferences.FitTo(monitor);
   // On small/high-DPI displays, keep the entire title bar and window inside the work area.
   MinWidth = Math.Min(MinWidth, monitor.WorkArea.Width / monitor.Scale);
   MinHeight = Math.Min(MinHeight, monitor.WorkArea.Height / monitor.Scale);
   var placement = new NativeWindowPlacement { Length = Marshal.SizeOf<NativeWindowPlacement>(), ShowCommand = preferences.Maximized ? 3 : 1,
    NormalPosition = ToNativeRect(bounds, monitor) };
   if (!SetWindowPlacement(new WindowInteropHelper(this).Handle, ref placement)) throw new Win32Exception(Marshal.GetLastWin32Error());
  }
  catch (Exception error) when (error is Win32Exception or ArgumentException) { App.Log(error); }
 }

 private void SaveDesktopWindow()
 {
  if (_qaOutput != null) return;
  try
  {
   nint handle = new WindowInteropHelper(this).Handle;
   var placement = new NativeWindowPlacement { Length = Marshal.SizeOf<NativeWindowPlacement>() };
   if (handle == 0 || !GetWindowPlacement(handle, ref placement)) return;
   var monitors = ConnectedMonitors();
   if (monitors.Count == 0) return;
   var normal = placement.NormalPosition;
   var approximate = new DesktopRect(normal.Left, normal.Top, normal.Right - normal.Left, normal.Bottom - normal.Top);
   // WINDOWPLACEMENT uses workspace coordinates; store physical screen coordinates instead.
   var monitor = new DesktopWindowPreferences { NormalBounds = approximate }.SelectMonitor(monitors);
   var bounds = approximate with { Left = approximate.Left + monitor.WorkArea.Left - monitor.Bounds.Left, Top = approximate.Top + monitor.WorkArea.Top - monitor.Bounds.Top };
   new DesktopWindowPreferences { NormalBounds = bounds, MonitorDevice = monitor.Device, DpiScale = monitor.Scale,
    Maximized = WindowState == WindowState.Maximized || (WindowState == WindowState.Minimized && (placement.Flags & 2) != 0),
    ListWidth = _preferredListWidth }.Save(WindowPreferencesPath);
  }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException or Win32Exception or ArgumentException) { App.Log(error); }
 }

 private static NativeRect ToNativeRect(DesktopRect bounds, DesktopMonitor monitor)
 {
  int x = (int)Math.Round(bounds.Left - monitor.WorkArea.Left + monitor.Bounds.Left);
  int y = (int)Math.Round(bounds.Top - monitor.WorkArea.Top + monitor.Bounds.Top);
  return new() { Left = x, Top = y, Right = x + (int)Math.Round(bounds.Width), Bottom = y + (int)Math.Round(bounds.Height) };
 }

 private static List<DesktopMonitor> ConnectedMonitors()
 {
  var monitors = new List<DesktopMonitor>();
  MonitorEnumeration callback = (nint monitor, nint hdc, ref NativeRect rectangle, nint data) =>
  {
   var info = new NativeMonitorInfo { Size = Marshal.SizeOf<NativeMonitorInfo>(), Device = "" };
   if (GetMonitorInfo(monitor, ref info))
   {
    double scale = GetDpiForMonitor(monitor, 0, out uint dpi, out _) == 0 ? dpi / 96d : 1;
    static DesktopRect Convert(NativeRect rect) => new(rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top);
    monitors.Add(new(info.Device, Convert(info.Bounds), Convert(info.WorkArea), scale));
   }
   return true;
  };
  if (!EnumDisplayMonitors(0, 0, callback, 0)) throw new Win32Exception(Marshal.GetLastWin32Error());
  return monitors;
 }

 private void RunUsabilityQa(string directory)
 {
  if (App.Arg("--usability-check") != "true") return;
  bool wasReady = _ready;
  string query = SearchBox.Text;
  bool initialEmpty = false, populatedVisible = false, cleared = false;
  try
  {
   _ready = false; _searchTimer.Stop();
   SearchBox.Clear(); UpdateLayout(); initialEmpty = ClearSearchButton.Visibility == Visibility.Collapsed;
   SearchBox.Text = "검색 확인"; UpdateLayout(); populatedVisible = ClearSearchButton.Visibility == Visibility.Visible;
   ClearSearch_Click(this, new()); UpdateLayout(); cleared = SearchBox.Text.Length == 0 && ClearSearchButton.Visibility == Visibility.Collapsed;
  }
  finally { SearchBox.Text = query; _ready = wasReady; }
  bool qaPreferencesBypassed = _qaOutput != null && _preferredListWidth == null;
  bool success = initialEmpty && populatedVisible && cleared && qaPreferencesBypassed && StatusText.ToolTip != null;
  File.WriteAllText(Path.Combine(directory, "wpf-usability.json"), JsonSerializer.Serialize(new { success, initialEmpty, populatedVisible, cleared, qaPreferencesBypassed, gameWrites = false }, new JsonSerializerOptions { WriteIndented = true }));
  if (!success) throw new InvalidOperationException("UI usability checks failed; inspect wpf-usability.json.");
  _qaChecks.Add("quick_search_clear_and_qa_preferences_isolation");
 }

 [StructLayout(LayoutKind.Sequential)] private struct NativePoint { public int X, Y; }
 [StructLayout(LayoutKind.Sequential)] private struct NativeRect { public int Left, Top, Right, Bottom; }
 [StructLayout(LayoutKind.Sequential)] private struct NativeWindowPlacement { public int Length, Flags, ShowCommand; public NativePoint MinPosition, MaxPosition; public NativeRect NormalPosition; }
 [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] private struct NativeMonitorInfo { public int Size; public NativeRect Bounds, WorkArea; public uint Flags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string Device; }
 private delegate bool MonitorEnumeration(nint monitor, nint hdc, ref NativeRect rectangle, nint data);
 [DllImport("user32.dll", SetLastError = true)] private static extern bool GetWindowPlacement(nint window, ref NativeWindowPlacement placement);
 [DllImport("user32.dll", SetLastError = true)] private static extern bool SetWindowPlacement(nint window, ref NativeWindowPlacement placement);
 [DllImport("user32.dll", SetLastError = true)] private static extern bool EnumDisplayMonitors(nint hdc, nint clip, MonitorEnumeration callback, nint data);
 [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern bool GetMonitorInfo(nint monitor, ref NativeMonitorInfo info);
 [DllImport("shcore.dll")] private static extern int GetDpiForMonitor(nint monitor, int type, out uint x, out uint y);
}
