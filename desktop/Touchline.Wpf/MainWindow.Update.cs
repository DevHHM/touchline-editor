using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private UpdatePreferences _updatePreferences = UpdatePreferences.Load();
 private readonly CancellationTokenSource _updateCancellation = new();
 private CancellationTokenSource? _updateDownloadCancellation;
 private UpdateRelease? _availableUpdate;
 private DownloadedUpdate? _pendingUpdate;
 private string? _updateJob;
 private bool _checkingUpdate, _downloadingUpdate;
 private int _updateCheckGeneration;
 private TextBlock? _updateStatusLabel;
 private string _updateStatus = "시작할 때 공식 GitHub 릴리스를 확인합니다.";
 private Window? _updateWindow;

 private void InitializeUpdates()
 {
  Closed += (_, _) => { _updateCancellation.Cancel(); _updateWindow?.Close(); };
  Loaded += async (_, _) =>
  {
   if (_qaOutput != null || App.Arg("--no-update") == "true") return;
   ReadUpdateResult();
   if (!_updatePreferences.CheckAtStartup || _updatePreferences.DeferUntil > DateTimeOffset.UtcNow) return;
   try { await Task.Delay(1800, _updateCancellation.Token); await CheckForUpdatesAsync(); }
   catch (OperationCanceledException) { }
  };
 }
 private void UpdateStatus(string message)
 {
  _updateStatus = message; if (_updateStatusLabel != null) _updateStatusLabel.Text = message;
 }
 private void SaveUpdatePreferences()
 {
  try { _updatePreferences.Save(); } catch (Exception ex) { App.Log(ex); UpdateStatus("업데이트 설정을 저장하지 못했습니다: " + ex.Message); }
 }
 private void ReadUpdateResult()
 {
  try
  {
   if (!File.Exists(UpdatePreferences.ResultPath)) return;
   var result = JsonSerializer.Deserialize<UpdateInstallResult>(File.ReadAllText(UpdatePreferences.ResultPath));
   File.Move(UpdatePreferences.ResultPath, UpdatePreferences.ResultPath + ".read", true);
   if (result == null) return;
   UpdateStatus(result.Success ? $"{result.Version} 설치 완료. 이전 버전: {result.BackupDirectory}" : "자동 설치를 완료하지 못했습니다. " + result.Message);
   if (!result.Success) AppDialog.Show(this, _updateStatus, "업데이트 결과", MessageBoxButton.OK, MessageBoxImage.Information);
  }
  catch (Exception ex) { App.Log(ex); }
 }
 public FrameworkElement CreateUpdateSettings()
 {
  var panel = new StackPanel();
  panel.Children.Add(Text("Touchline 업데이트", 22, bold: true));
  panel.Children.Add(Text("공식 배포: DevHHM/touchline-editor\n다운로드와 설치는 동의한 뒤 진행합니다. 설치는 에디터를 정상 종료한 다음 실행되며 자동으로 다시 시작하지 않습니다.", 13));
  var automatic = new CheckBox { Content = "시작할 때 새 버전 자동 확인", IsChecked = _updatePreferences.CheckAtStartup, Margin = new Thickness(0, 18, 0, 10) };
  automatic.Checked += (_, _) => { _updatePreferences = _updatePreferences with { CheckAtStartup = true }; SaveUpdatePreferences(); };
  automatic.Unchecked += (_, _) => { _updatePreferences = _updatePreferences with { CheckAtStartup = false }; SaveUpdatePreferences(); };
  panel.Children.Add(automatic);
  var preview = new CheckBox { Content = "테스트 릴리스 포함", IsChecked = _updatePreferences.IncludePreview, Margin = new Thickness(0, 0, 0, 16) };
  preview.Checked += (_, _) => SetUpdateChannel(true);
  preview.Unchecked += (_, _) => SetUpdateChannel(false);
  panel.Children.Add(preview);
  var buttons = new WrapPanel();
  buttons.Children.Add(Button("지금 확인", async (_, _) => await CheckForUpdatesAsync(true)));
  buttons.Children.Add(Button("다운로드 · 종료 후 설치", async (_, _) => await OfferUpdateAsync()));
  buttons.Children.Add(Button("다운로드 · 설치 예약 취소", (_, _) => { _updateDownloadCancellation?.Cancel(); _pendingUpdate = null; UpdateStatus("다운로드 또는 설치 예약을 취소했습니다."); }));
  panel.Children.Add(buttons);
  _updateStatusLabel = Text(_updateStatus, 13); _updateStatusLabel.Margin = new Thickness(0, 16, 0, 0); panel.Children.Add(_updateStatusLabel);
  return panel;
 }
 private void SetUpdateChannel(bool includePreview)
 {
  _updatePreferences = _updatePreferences with { IncludePreview = includePreview };
  _updateCheckGeneration++; _availableUpdate = null;
  SaveUpdatePreferences();
  UpdateStatus("릴리스 채널을 변경했습니다. '지금 확인'을 눌러 다시 확인하세요.");
 }
 public void OpenUpdateSettings()
 {
  if (_updateWindow is { IsVisible: true }) { _updateWindow.Activate(); return; }
  _updateWindow = new Window { Title = "Touchline 업데이트", Owner = this, Width = 640, Height = 400, MinWidth = 520, MinHeight = 300, WindowStartupLocation = WindowStartupLocation.CenterOwner, Background = ResourceBrush("PanelBrush"), Foreground = ResourceBrush("InkBrush"), Content = new ScrollViewer { VerticalScrollBarVisibility = ScrollBarVisibility.Auto, Content = new Border { Padding = new Thickness(24), Child = CreateUpdateSettings() } } };
  Typography.Initialize(_updateWindow);
  _updateWindow.Closed += (_, _) => { _updateWindow = null; _updateStatusLabel = null; };
  _updateWindow.Show();
 }
 public async Task CheckForUpdatesAsync(bool manual = false)
 {
  if (_checkingUpdate || _downloadingUpdate || _closing || _qaOutput != null) return;
  if (_pendingUpdate != null) { UpdateStatus($"{_pendingUpdate.Version} 설치 예약됨 · 에디터를 정상 종료하면 설치합니다."); return; }
  _checkingUpdate = true; int generation = ++_updateCheckGeneration;
  bool includePreview = _updatePreferences.IncludePreview;
  UpdateStatus("공식 릴리스 확인 중…");
  try
  {
   using var download = new UpdateDownload();
   var version = UpdateVersion.Parse(typeof(App).Assembly.GetName().Version!.ToString(3));
   var available = await download.CheckAsync(version, includePreview, _updateCancellation.Token);
   if (_closing || generation != _updateCheckGeneration || includePreview != _updatePreferences.IncludePreview) return;
   _availableUpdate = available;
   UpdateStatus(_availableUpdate == null ? $"현재 버전 {version} · 새로 설치할 릴리스가 없습니다." : $"새 버전 {_availableUpdate.Version} · {(_availableUpdate.ArchiveBytes / 1048576d):N0} MB");
   if (_availableUpdate != null) Status($"새 버전 {_availableUpdate.Version} 사용 가능 · 업데이트 버튼에서 확인하세요.");
   // Do not interrupt an active edit/review with an automatic modal.
   if (_availableUpdate != null && _tutorialWindow == null && (manual || (!_busy && _detail?.Fields.Any(f => f.IsDirty) != true && _relationsView?.HasPendingChanges != true))) await OfferUpdateAsync();
  }
  catch (OperationCanceledException) { if (!_closing && generation == _updateCheckGeneration) UpdateStatus("업데이트 확인이 취소되었거나 시간이 초과되었습니다. 나중에 다시 확인할 수 있습니다."); }
  catch (Exception ex) { App.Log(ex); if (generation == _updateCheckGeneration) UpdateStatus("업데이트를 확인하지 못했습니다. 오프라인이어도 에디터를 계속 사용할 수 있습니다. " + ex.Message); }
  finally { _checkingUpdate = false; }
 }
 private async Task OfferUpdateAsync()
 {
  if (_tutorialWindow != null) { UpdateStatus("새 버전이 있습니다. 시작 안내를 닫은 뒤 업데이트에서 설치할 수 있습니다."); return; }
  if (_downloadingUpdate || _closing) return;
  if (_availableUpdate == null) { UpdateStatus("먼저 '지금 확인'을 눌러주세요."); return; }
  if (_pendingUpdate != null) { UpdateStatus("이미 종료 후 설치가 예약되어 있습니다."); return; }
  string? target = UpdateInstall.PackageRoot(AppContext.BaseDirectory);
  if (target == null || !File.Exists(Path.Combine(AppContext.BaseDirectory, "Touchline Updater.exe"))) { UpdateStatus("이 실행본은 자동 설치를 지원하는 배포 ZIP이 아닙니다. 새 배포본 전체를 받아 실행하세요."); return; }
  var release = _availableUpdate;
  if (AppDialog.Show(this, $"Touchline Editor {release.Version}을 내려받고 에디터를 정상 종료한 뒤 설치할까요?\n\n현재 작업은 계속할 수 있습니다. 이전 배포본과 패키지 안의 개인 파일은 보존합니다. 자동 재시작은 하지 않습니다.\n공식 배포: {release.Page}\n\n'아니요'를 선택하면 내일까지 자동 알림을 미룹니다.", "새 버전 사용 가능", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes)
  {
   _updatePreferences = _updatePreferences with { DeferUntil = DateTimeOffset.UtcNow.AddDays(1) }; SaveUpdatePreferences(); UpdateStatus("내일까지 자동 알림을 미뤘습니다. '지금 확인'은 언제든 사용할 수 있습니다."); return;
  }
  _downloadingUpdate = true;
  _updateDownloadCancellation = CancellationTokenSource.CreateLinkedTokenSource(_updateCancellation.Token);
  var cancellation = _updateDownloadCancellation.Token;
  try
  {
   UpdateInstall.ValidateTarget(target);
   _updateJob = Path.Combine(UpdatePreferences.DataDirectory, "job-" + Guid.NewGuid().ToString("N"));
   using var download = new UpdateDownload();
   var result = await download.DownloadAsync(release, _updateJob, new Progress<double>(p => UpdateStatus($"{release.Version} 다운로드 중… {p:P0}")), cancellation);
   UpdateStatus("다운로드 완료 · ZIP과 패키지 파일 검증 중…");
   await Task.Run(() => UpdatePackage.Extract(result, Path.Combine(_updateJob, "verified"), cancellation), cancellation);
   cancellation.ThrowIfCancellationRequested();
   _pendingUpdate = result;
   UpdateStatus($"{release.Version} 설치 준비 완료. 에디터를 정상 종료하면 설치합니다. '설치 예약 취소'로 미룰 수 있습니다.");
   Status($"{release.Version} 업데이트 준비 완료 · 종료 후 설치 예약됨");
  }
  catch (OperationCanceledException) { UpdateStatus("다운로드가 취소되었습니다. 설치하지 않았습니다."); }
  catch (Exception ex) { App.Log(ex); UpdateStatus("업데이트를 준비하지 못했습니다. 설치하지 않았습니다. " + ex.Message); }
  finally { _downloadingUpdate = false; _updateDownloadCancellation.Dispose(); _updateDownloadCancellation = null; }
 }
 // Called only after normal close guards and bridge/tool disposal have succeeded.
 private void LaunchPendingUpdate()
 {
  _updateCancellation.Cancel();
  if (_pendingUpdate == null || _updateJob == null || _qaOutput != null) return;
  try
  {
   string target = UpdateInstall.PackageRoot(AppContext.BaseDirectory) ?? throw new InvalidDataException("배포 폴더를 찾지 못했습니다.");
   string helper = Path.Combine(_updateJob, "Touchline Updater.exe");
   File.Copy(Path.Combine(AppContext.BaseDirectory, "Touchline Updater.exe"), helper, true);
   using var current = Process.GetCurrentProcess();
   var request = new UpdateInstallRequest(_pendingUpdate, target, current.Id, current.StartTime.ToUniversalTime().Ticks);
   string requestPath = Path.Combine(_updateJob, "install-request.json"); File.WriteAllText(requestPath, JsonSerializer.Serialize(request));
   var start = new ProcessStartInfo(helper) { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden, WorkingDirectory = _updateJob };
   start.ArgumentList.Add(requestPath);
   using var process = Process.Start(start) ?? throw new IOException("설치 도우미를 시작하지 못했습니다.");
  }
  catch (Exception ex) { App.Log(ex); AppDialog.Show(this, "자동 설치를 시작하지 못했습니다. 현재 버전은 그대로 유지됩니다.\n" + ex.Message, "업데이트", MessageBoxButton.OK, MessageBoxImage.Information); }
 }
}
