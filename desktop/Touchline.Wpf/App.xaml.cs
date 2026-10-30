using System.IO;
using System.Windows;
namespace Touchline.Wpf;
public partial class App : Application
{
 public static string[] Arguments { get; private set; } = [];
 protected override void OnStartup(StartupEventArgs e)
 {
  base.OnStartup(e); Arguments=e.Args;
  if (Arg("--ui-check") is not null && Arg("--qa-offscreen") == "true")
  {
   EventManager.RegisterClassHandler(typeof(Window), FrameworkElement.LoadedEvent,
    new RoutedEventHandler((sender, args) =>
    {
     if (sender is Window testWindow && ReferenceEquals(args.OriginalSource, testWindow))
      PlaceQaWindow(testWindow);
    }));
  }
  DispatcherUnhandledException += (_,err) => { Log(err.Exception); AppDialog.Show(err.Exception.Message,"Touchline Editor"); err.Handled=true; };
  var window = new MainWindow();
  if (Arg("--ui-check") is not null && Arg("--qa-offscreen") == "true")
  {
   PlaceQaWindow(window);
  }
  window.Show();
 }
 private static void PlaceQaWindow(Window window)
 {
  window.ShowActivated = false;
  window.ShowInTaskbar = false;
  window.WindowStartupLocation = WindowStartupLocation.Manual;
  window.WindowState = WindowState.Normal;
  window.Left = -16000;
  window.Top = 0;
 }
 public static void Log(Exception ex) { try { var dir=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"TouchlineEditor"); Directory.CreateDirectory(dir); File.AppendAllText(Path.Combine(dir,"wpf-errors.log"),DateTime.Now+" "+ex+Environment.NewLine); } catch {} }
 public static string? Arg(string key) { int i=Array.IndexOf(Arguments,key); return i>=0 && i+1<Arguments.Length ? Arguments[i+1] : null; }
}
