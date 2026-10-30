using System.IO;
using System.Windows;
namespace Touchline.Wpf;
public partial class App : Application
{
 public static string[] Arguments { get; private set; } = [];
 protected override void OnStartup(StartupEventArgs e)
 {
  base.OnStartup(e); Arguments=e.Args;
  string logDirectory = Arg("--ui-check") is { } qa ? Path.Combine(Path.GetDirectoryName(Path.GetFullPath(qa))!, "diagnostic-logs") : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "Logs");
  DiagnosticsLog.Current = new DiagnosticsLog(logDirectory, typeof(App).Assembly.GetName().Version?.ToString(3) ?? "unknown");
  if(Arg("--ui-check") == null) AbilityVisibility.Current.Load(AbilityVisibility.DefaultPath);
  if(Arg("--ui-check") == null) DiagnosticSharing.Start(DiagnosticsConsentPrompt.AskAsync);
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
  AppDomain.CurrentDomain.UnhandledException += (_,err) => { if(err.ExceptionObject is Exception error) DiagnosticsLog.Current?.Failure("application","unhandled_domain",error); };
  TaskScheduler.UnobservedTaskException += (_,err) => DiagnosticsLog.Current?.Failure("application","unobserved_task",err.Exception);
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
 protected override void OnExit(ExitEventArgs e) { DiagnosticSharing.Stop(); DiagnosticsLog.Current?.Dispose(); base.OnExit(e); }
 public static void Log(Exception ex) => DiagnosticsLog.Current?.Failure("application","handled",ex,"last_handled_error");
 public static string? Arg(string key) { int i=Array.IndexOf(Arguments,key); return i>=0 && i+1<Arguments.Length ? Arguments[i+1] : null; }
}
