using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private TutorialPreferences _tutorialPreferences = new();
 private GuidedTutorialOverlay? _tutorialOverlay;
 internal bool IsTutorialOpen => _tutorialOverlay != null;
 private IInputElement? _tutorialPreviousFocus;
 private (double X, double Y, double SidebarY) _tutorialScrollPosition;
 private bool _firstTutorialPending, _tutorialStartupReady;
 private bool TutorialAllowed => _qaOutput == null && !_qaRunning && TutorialPreferences.IsInteractiveLaunch(App.Arguments);

 private void InitializeTutorial()
 {
  Closed += (_, _) => RemoveTutorialOverlay(restoreFocus: false);
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
  if (!TutorialAllowed || !_firstTutorialPending || _closing || _busy || !IsVisible || !IsEnabled || !IsActive) return;
  ShowTutorial();
 }

 private void Tutorial_Click(object sender, RoutedEventArgs args) => ShowTutorial();

 private void ShowTutorial()
 {
  if (!TutorialAllowed || _closing || _busy || !IsEnabled) return;
  if (_tutorialOverlay != null) { _tutorialOverlay.Focus(); return; }
  _firstTutorialPending = false;
  ShowTutorialOverlay(persistDismissal: true);
 }

 private FrameworkElement? ResolveTutorialTarget(string key) => key switch
 {
  "connection" => ConnectButton,
  "navigation" => NavigationButtons,
  "search" => SearchScreenButton,
  "review" => ReviewButton,
  "history" => ChangeHistoryButton,
  "help" => HelpButton,
  _ => null
 };

 // The guide highlights the real controls without invoking any editor/game operation.
 // It sits above the scaled workspace so target bounds also work with UI zoom and DPI.
 private GuidedTutorialOverlay ShowTutorialOverlay(bool persistDismissal)
 {
  _tutorialPreviousFocus = Keyboard.FocusedElement;
  _tutorialScrollPosition = (WorkspaceViewport.HorizontalOffset, WorkspaceViewport.VerticalOffset, SidebarScroll.VerticalOffset);
  var tutorial = new GuidedTutorialOverlay(TutorialTour.Steps, ResolveTutorialTarget);
  _tutorialOverlay = tutorial;
  Grid.SetRowSpan(tutorial, 3);
  Panel.SetZIndex(tutorial, 1000);
  System.Windows.Shell.WindowChrome.SetIsHitTestVisibleInChrome(tutorial, true);
  tutorial.Finished += action =>
  {
   RemoveTutorialOverlay(restoreFocus: true);
   // Escape/close only ends this session; explicit Skip/Done controls first-launch state.
   if (!persistDismissal || action == TutorialExitAction.None) return;
   _tutorialPreferences = _tutorialPreferences.Dismiss(action);
   try { _tutorialPreferences.Save(TutorialPreferences.SettingsPath); }
   catch (Exception error) when (error is IOException or UnauthorizedAccessException)
   { Status("시작 안내 설정을 저장하지 못했습니다. 다음 실행에 안내가 다시 표시될 수 있습니다."); }
  };
  TutorialRoot.Children.Add(tutorial);
  UpdateLayout();
  tutorial.Start();
  return tutorial;
 }

 private void RemoveTutorialOverlay(bool restoreFocus)
 {
  var tutorial = _tutorialOverlay;
  if (tutorial == null) return;
  _tutorialOverlay = null;
  tutorial.Stop();
  TutorialRoot.Children.Remove(tutorial);
  WorkspaceViewport.ScrollToHorizontalOffset(_tutorialScrollPosition.X);
  WorkspaceViewport.ScrollToVerticalOffset(_tutorialScrollPosition.Y);
  SidebarScroll.ScrollToVerticalOffset(_tutorialScrollPosition.SidebarY);
  if (restoreFocus && !_closing && IsActive)
  {
   if (_tutorialPreviousFocus is UIElement { IsVisible: true, IsEnabled: true } previous) Keyboard.Focus(previous);
   else ConnectButton.Focus();
  }
  _tutorialPreviousFocus = null;
 }
}
