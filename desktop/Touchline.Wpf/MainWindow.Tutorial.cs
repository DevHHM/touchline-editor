using System.IO;
using System.Windows;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private TutorialPreferences _tutorialPreferences = new();
 private TutorialWindow? _tutorialWindow;
 private bool _firstTutorialPending, _tutorialStartupReady;
 private bool TutorialAllowed => _qaOutput == null && !_qaRunning && TutorialPreferences.IsInteractiveLaunch(App.Arguments);

 private void InitializeTutorial()
 {
  // Deliberately do not even read real first-launch state in snapshot or unattended runs.
  if (!TutorialAllowed) return;
  _tutorialPreferences = TutorialPreferences.Load(TutorialPreferences.SettingsPath);
  _firstTutorialPending = _tutorialPreferences.ShouldShow;
  Activated += (_, _) =>
  {
   if (_tutorialStartupReady && _firstTutorialPending) Dispatcher.BeginInvoke(TryShowFirstLaunchTutorial);
  };
 }

 private void ShowFirstLaunchTutorial()
 {
  if (!TutorialAllowed) return;
  _tutorialStartupReady = true;
  TryShowFirstLaunchTutorial();
 }

 private void TryShowFirstLaunchTutorial()
 {
  // Initialization may complete while Help is modal or another app is active.
  // Keep the introduction pending until the editor owns the active, enabled window.
  if (!TutorialAllowed || !_firstTutorialPending || _closing || !IsVisible || !IsEnabled || !IsActive) return;
  ShowTutorial();
 }

 private void Tutorial_Click(object sender, RoutedEventArgs args) => ShowTutorial();

 private void ShowTutorial()
 {
  if (!TutorialAllowed || _closing || !IsEnabled) return;
  if (_tutorialWindow is { IsVisible: true }) { _tutorialWindow.Activate(); return; }
  _firstTutorialPending = false;
  var tutorial = new TutorialWindow { Owner = this };
  // Close the modal introduction before opening the existing reviewed-values tab.
  // Navigation does not preview, enable or tick a freeze operation.
  tutorial.FreezeNavigationRequested += () => Dispatcher.BeginInvoke(() => OpenWorkbench(true));
  _tutorialWindow = tutorial;
  try
  {
   tutorial.ShowDialog();
   // Closing the title bar or pressing Escape leaves the next first launch intact.
   if (tutorial.ExitAction == TutorialExitAction.None) return;
   _tutorialPreferences = _tutorialPreferences.Dismiss(tutorial.ExitAction);
   try { _tutorialPreferences.Save(TutorialPreferences.SettingsPath); }
   catch (Exception error) when (error is IOException or UnauthorizedAccessException)
   { Status("시작 안내 설정을 저장하지 못했습니다. 다음 실행에 안내가 다시 표시될 수 있습니다."); }
  }
  finally { _tutorialWindow = null; }
 }
}
