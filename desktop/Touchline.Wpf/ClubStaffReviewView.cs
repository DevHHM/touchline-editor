using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

/// <summary>Read-only staff assessment with cancellable, bounded bridge scans.</summary>
public sealed class ClubStaffReviewView : UserControl
{
 private readonly BridgeClient _bridge;
 private readonly EntityRecord _club;
 private readonly bool _live;
 private readonly Func<EntityRecord, Task>? _open;
 private readonly StackPanel _root = new(), _staffDetail = new(), _candidateDetail = new(), _warnings = new();
 private readonly WrapPanel _squads = new() { Margin = new Thickness(0, 4, 0, 8) };
 private readonly Dictionary<string, string> _selectedStaffBySquad = [];
 private List<StaffReviewRow> _allRows = [];
 private string? _selectedSquad;
 private int _evaluatedCount;
 private readonly DataGrid _staff = Table(320), _candidates = Table(230);
 private readonly TextBlock _status = Text("현재 스태프를 불러옵니다."), _coverage = Text("후보 검토를 시작하면 비교 범위가 표시됩니다.", 11), _count = Text("현재 스태프", 16, true);
 private readonly Button _start = Button("교체 후보 검토 시작"), _cancel = Button("검토 중지"), _openStaff = Button("선택한 스태프 프로필 ↗"), _openCandidate = Button("후보 프로필 ↗");
 private readonly ComboBox _limit = new() { MinWidth = 142, MinHeight = 32, DisplayMemberPath = "Label", SelectedValuePath = "Value" };
 private readonly ProgressBar _progress = new() { Height = 5, Minimum = 0, Maximum = 1, Margin = new Thickness(0, 0, 0, 8) };
 private readonly Border _staffPanel, _candidatePanel;
 private TaskCompletionSource? _inflight;
 private StaffReviewReport? _report;
 private StaffReviewProgress? _last;
 private readonly List<EntityRecord> _roster = [];
 private ulong? _reviewId;
 private bool _active = true, _busy, _scanRunning, _cancelRequested, _loaded, _partial;
 private int _generation;
 public bool IsBusy => _busy;
 public bool HasPendingChanges => false;
 public event Action<bool>? BusyChanged;
 public event Action<string>? SquadChanged;
 public string? SelectedSquad => _selectedSquad;

 public ClubStaffReviewView(BridgeClient bridge, EntityRecord club, bool live, Func<EntityRecord, Task>? open = null, string? initialSquad = null)
 {
  _bridge = bridge; _club = club; _live = live; _open = open; _selectedSquad = initialSquad;
  FontFamily = Typography.AppFont; FontWeight = FontWeights.Medium; FontSize = 13; Foreground = Brush("InkBrush"); UseLayoutRounding = true; SnapsToDevicePixels = true;
  TextOptions.SetTextFormattingMode(this, TextFormattingMode.Display); SetResourceReference(TextOptions.TextRenderingModeProperty, "AppTextRenderingMode");
  _staffPanel = Panel(_staffDetail); _candidatePanel = Panel(_candidateDetail); _staffPanel.Visibility = _candidatePanel.Visibility = Visibility.Collapsed;
  Content = _root;
  _root.Children.Add(Text("스태프 · 전력 평가와 교체 후보", 20, true));
  _root.Children.Add(Text("직무 점수와 명성으로 대체 후보를 비교합니다. 영입 의사·해지 비용은 별도 확인이 필요합니다.", 12));
  if (!_live) _root.Children.Add(Text("스냅샷 · 저장된 자료로만 평가합니다.", 11));
  _root.Children.Add(_squads);
  var commands = new WrapPanel { Margin = new Thickness(0, 2, 0, 4) };
  _limit.ItemsSource = new[] { new ScanLimit(2000, "후보 최대 2,000명"), new ScanLimit(5000, "후보 최대 5,000명"), new ScanLimit(20000, "후보 최대 20,000명") }; _limit.SelectedIndex = 0; _limit.Margin = new Thickness(0, 0, 8, 8); commands.Children.Add(_limit); commands.Children.Add(_start); commands.Children.Add(_cancel); _root.Children.Add(commands);
  _start.Click += async (_, _) => await StartAsync(); _cancel.Click += (_, _) => RequestCancel();
  _root.Children.Add(_progress); _root.Children.Add(_status); _root.Children.Add(_coverage); _root.Children.Add(new Expander { Header = "평가 기준과 확인되지 않은 정보", Content = _warnings, Margin = new Thickness(0, 0, 0, 7), FontWeight = FontWeights.Medium });
  _count.Margin = new Thickness(0, 12, 0, 8); _root.Children.Add(_count);
  Column(_staff, "이름", nameof(StaffReviewRow.Name), 140, true); Column(_staff, "직무", nameof(StaffReviewRow.Role), 95); Column(_staff, "점수", nameof(StaffReviewRow.ScoreText), 78, sort: nameof(StaffReviewRow.Score)); Column(_staff, "명성", nameof(StaffReviewRow.ReputationText), 65, sort: nameof(StaffReviewRow.Reputation)); Column(_staff, "평가", nameof(StaffReviewRow.StatusLabel), 100); Column(_staff, "팀", nameof(StaffReviewRow.Group), 75); Column(_staff, "CA", nameof(StaffReviewRow.CaText), 55, sort: "Person.Ca"); Column(_staff, "판단 근거", nameof(StaffReviewRow.ReasonSummary), 300);
  var statusStyle = new Style(typeof(TextBlock), Application.Current.TryFindResource(typeof(TextBlock)) as Style); statusStyle.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis));
  foreach (var (status, color) in new[] { ("replace_review", "GoldBrush"), ("retain", "AccentBrush"), ("insufficient", "MutedBrush") }) { var trigger = new DataTrigger { Binding = new Binding(nameof(StaffReviewRow.Status)), Value = status }; trigger.Setters.Add(new Setter(TextBlock.ForegroundProperty, Brush(color))); statusStyle.Triggers.Add(trigger); }
  ((DataGridTextColumn)_staff.Columns[4]).ElementStyle = statusStyle;
  NameCopy.AttachGrid(_staff, item => (item as StaffReviewRow)?.Person, message => _status.Text = message);
  NameCopy.AttachGrid(_candidates, item => (item as StaffReviewCandidate)?.Person, message => _status.Text = message);
  _root.Children.Add(Text("이름 복사: 행 우클릭 · Ctrl+C / 원어 이름 Ctrl+Shift+C", 11));
  _staff.SelectionChanged += (_, _) => ShowStaff(); _staff.MouseDoubleClick += async (_, args) => { if (IsDataRow(args.OriginalSource)) await OpenAsync((_staff.SelectedItem as StaffReviewRow)?.Person); };
  _root.Children.Add(Panel(_staff)); _openStaff.Click += async (_, _) => await OpenAsync((_staff.SelectedItem as StaffReviewRow)?.Person); _root.Children.Add(_openStaff); _root.Children.Add(_staffPanel);
  Column(_candidates, "후보", nameof(StaffReviewCandidate.Name), 140, true); Column(_candidates, "점수 /300", nameof(StaffReviewCandidate.ScoreText), 78, sort: nameof(StaffReviewCandidate.Score)); Column(_candidates, "향상", nameof(StaffReviewCandidate.ImprovementText), 60, sort: nameof(StaffReviewCandidate.Improvement)); Column(_candidates, "명성", nameof(StaffReviewCandidate.ReputationText), 65, sort: nameof(StaffReviewCandidate.Reputation)); Column(_candidates, "상태 · 추정", nameof(StaffReviewCandidate.AvailabilityLabel), 160);
  _candidates.SelectionChanged += (_, _) => ShowCandidate(); _candidates.MouseDoubleClick += async (_, args) => { if (IsDataRow(args.OriginalSource)) await OpenAsync((_candidates.SelectedItem as StaffReviewCandidate)?.Person); };
  _openCandidate.Click += async (_, _) => await OpenAsync((_candidates.SelectedItem as StaffReviewCandidate)?.Person);
  AbilitySafeGridSort.Attach(_staff);
  Loaded += (_, _) => { if (_active) { AbilityVisibility.Current.Changed -= AbilityVisibilityChanged; AbilityVisibility.Current.Changed += AbilityVisibilityChanged; AbilityVisibilityChanged(null, EventArgs.Empty); } };
  Unloaded += (_, _) => AbilityVisibility.Current.Changed -= AbilityVisibilityChanged;
  UpdateButtons();
 }
 private sealed record ScanLimit(int Value, string Label);
 private static T Decode<T>(JsonElement value) => value.Deserialize<T>(JsonDefaults.Options) ?? throw new InvalidDataException("스태프 평가 응답을 읽지 못했습니다.");
 private static Brush Brush(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Text(string text, double size = 12, bool bold = false) => new() { Text = text, FontSize = size, FontWeight = bold ? FontWeights.Bold : FontWeights.Medium, Foreground = Brush(bold ? "InkBrush" : "MutedBrush"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 0, 7) };
 private static Button Button(string text) => new() { Content = text, Margin = new Thickness(0, 0, 8, 8), Padding = new Thickness(12, 7, 12, 7), FontWeight = FontWeights.Medium, HorizontalAlignment = HorizontalAlignment.Left };
 private static Border Panel(UIElement content) => new() { Child = content, Padding = new Thickness(14), Background = Brush("PanelBrush"), BorderBrush = Brush("BorderBrush"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(7), Margin = new Thickness(0, 0, 0, 12) };
 private static DataGrid Table(double maxHeight) => new() { IsReadOnly = true, AutoGenerateColumns = false, CanUserAddRows = false, CanUserDeleteRows = false, SelectionMode = DataGridSelectionMode.Single, FrozenColumnCount = 1, RowHeight = 37, MinHeight = 105, MaxHeight = maxHeight, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
 private static void Column(DataGrid grid, string label, string property, double width, bool star = false, string? sort = null)
 {
  var style = new Style(typeof(TextBlock), Application.Current.TryFindResource(typeof(TextBlock)) as Style); style.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis)); style.Setters.Add(new Setter(ToolTipService.ToolTipProperty, new Binding(property)));
  grid.Columns.Add(new DataGridTextColumn { Header = label, Binding = new Binding(property), SortMemberPath = sort ?? property, Width = new DataGridLength(star ? 1 : width, star ? DataGridLengthUnitType.Star : DataGridLengthUnitType.Pixel), MinWidth = width, ElementStyle = style });
 }
 private static bool IsDataRow(object source)
 {
  for (var element = source as DependencyObject; element != null; element = element is Visual ? VisualTreeHelper.GetParent(element) : LogicalTreeHelper.GetParent(element)) if (element is DataGridRow) return true;
  return false;
 }
 private void UpdateButtons()
 {
  _start.IsEnabled = _active && !_busy && _loaded; _limit.IsEnabled = _active && !_busy; _cancel.IsEnabled = _active && _busy && _scanRunning && !_cancelRequested;
  _start.Content = _last?.Complete == true ? "후보 다시 검토" : "교체 후보 검토 시작";
  _openStaff.IsEnabled = _active && !_busy && _open != null && _staff.SelectedItem is StaffReviewRow; _openCandidate.IsEnabled = _active && !_busy && _open != null && _candidates.SelectedItem is StaffReviewCandidate;
 }
 public Task LoadAsync() => RunAsync(async () =>
 {
  int generation = _generation, offset = 0; var roster = new List<EntityRecord>();
  while (_active && generation == _generation)
  {
   var result = await _bridge.CallAsync("search", new { kind = "Staff", club_id = _club.Id, limit = 500, offset, sort = AbilityVisibility.Current.HideCa ? "name" : "ca", descending = !AbilityVisibility.Current.HideCa });
   if (!_active || generation != _generation) return;
   var page = Decode<List<EntityRecord>>(result.GetProperty("items")); roster.AddRange(page); offset += page.Count;
   if (page.Count == 0 || offset >= result.GetProperty("total").GetInt32() || _cancelRequested) break;
  }
  if (!_active) return;
  _roster.Clear(); _roster.AddRange(roster); _loaded = true; _last = null; _report = new StaffReviewReport { Club = _club, Rows = roster.Select(PendingRow).ToList() };
  DisplayReport(_report, false); _status.Text = $"현재 스태프 {roster.Count:N0}명 · 후보 검토를 시작하세요.";
 });
 private Task StartAsync(bool cancelAfterFirstChunk = false)
 {
  if (!_loaded) return Task.CompletedTask;
  return RunAsync(() => ScanAsync(cancelAfterFirstChunk));
 }
 private async Task ScanAsync(bool cancelAfterFirstChunk)
 {
  int generation = _generation; ulong? id = null; bool closed = false;
  _scanRunning = true; _last = null; _partial = true; _progress.Value = 0; _progress.IsIndeterminate = true; _status.Text = "현재 스태프 평가를 준비합니다…"; _coverage.Text = "후보 비교 범위를 확인합니다…"; _warnings.Children.Clear();
  DisplayReport(new StaffReviewReport { Club = _club }, true); UpdateButtons();
  try
  {
   var progress = Decode<StaffReviewProgress>(await _bridge.CallAsync("staff_review_start", new { club_id = _club.Id, max_candidates = _limit.SelectedValue is int limit ? limit : 2000 }));
   id = _reviewId = progress.ReviewId; if (id == 0) throw new InvalidDataException("스태프 평가 ID를 받지 못했습니다.");
   if (cancelAfterFirstChunk) _cancelRequested = true;
   while (true)
   {
    if (progress.Complete) { closed = true; if (_active && generation == _generation) { _last = progress; ShowProgress(progress, false); } return; }
    if (!_active || generation != _generation || _cancelRequested)
    {
     var cancelled = Decode<StaffReviewProgress>(await _bridge.CallAsync("staff_review_cancel", new { review_id = id })); closed = true;
     if (_active && generation == _generation) { _last = cancelled; ShowProgress(cancelled, true); }
     return;
    }
    _last = progress; ShowProgress(progress, false);
    await Dispatcher.Yield(DispatcherPriority.Background);
    if (_cancelRequested || !_active) continue;
    progress = Decode<StaffReviewProgress>(await _bridge.CallAsync("staff_review_step", new { review_id = id }));
   }
  }
  finally
  {
   if (!closed && id != null)
   {
    try { await _bridge.CallAsync("staff_review_cancel", new { review_id = id }); } catch (Exception error) { App.Log(error); }
   }
   _reviewId = null; _scanRunning = false; _progress.IsIndeterminate = false;
  }
 }
 private void ShowProgress(StaffReviewProgress progress, bool cancelled)
 {
  _partial = cancelled || !progress.Complete; _progress.IsIndeterminate = progress.Total <= 0 && !cancelled && !progress.Complete; _progress.Maximum = Math.Max(1, progress.Total); _progress.Value = Math.Clamp(progress.Scanned, 0, Math.Max(1, progress.Total));
  string phase = progress.Phase switch { "incumbents" => "현재 스태프 평가", "candidates" => "대체 후보 비교", "complete" => "평가 완료", _ => "스태프 평가" };
  _status.Text = cancelled ? $"검토 중지 · {progress.Scanned:N0}/{progress.Total:N0}명 확인 · 부분 결과입니다." : progress.Complete ? $"평가 완료 · {progress.Scanned:N0}명 확인 · 읽기 실패 {progress.Failed:N0}명" : $"{phase} · {progress.Scanned:N0}/{progress.Total:N0}명 · 읽기 실패 {progress.Failed:N0}명";
  if (progress.Report != null) { _report = progress.Report; DisplayReport(_report, _partial); }
 }
 private void DisplayReport(StaffReviewReport report, bool partial)
 {
  var rows = partial ? report.Rows.Concat(_roster.Where(person => !report.Rows.Any(row => row.Person.Identity == person.Identity)).Select(PendingRow)).ToList() : report.Rows;
  RememberSelectedStaff(); _allRows = rows; _evaluatedCount = report.Rows.Count;
  var options = SquadGroups.Options(rows.Select(row => row.Group));
  _selectedSquad = SquadGroups.DefaultKey(options, _selectedSquad);
  RenderSquads(options); ApplySquadFilter();
  var coverage = report.Coverage;
  if (_last != null || coverage.SelectedCandidates > 0) _coverage.Text = $"구단 명성 {StaffReviewFormat.Number(report.ClubReputation)} · 후보 {coverage.ScannedCandidates:N0}/{coverage.SelectedCandidates:N0}명 비교 · 전체 대상 {coverage.EligibleCandidates:N0}명 · 실패 {coverage.FailedCandidates:N0}명";
  _warnings.Children.Clear(); if (coverage.Method.Length > 0) _warnings.Children.Add(Text(coverage.Method, 11)); foreach (string warning in report.Warnings.Distinct()) { var text = Text(warning, 11); text.Foreground = Brush("GoldBrush"); _warnings.Children.Add(text); }
  ShowStaff(); UpdateButtons();
 }
 private void RememberSelectedStaff()
 {
  if (_selectedSquad != null && _staff.SelectedItem is StaffReviewRow selected) _selectedStaffBySquad[_selectedSquad] = selected.Person.Identity;
 }
 private void RenderSquads(IReadOnlyList<SquadGroupOption> options)
 {
  _squads.Children.Clear();
  foreach (var option in options)
  {
   var button = Button(option.Display); button.Tag = option.Key; button.ToolTip = $"{option.Label} 소속 스태프 {option.Count:N0}명";
   button.Background = Brush("PanelBrush");
   if (option.Key == _selectedSquad) { button.Background = new SolidColorBrush(Color.FromRgb(38, 61, 57)); button.Foreground = Brush("AccentBrush"); }
   button.Click += (_, _) => SelectSquad(option.Key); _squads.Children.Add(button);
  }
 }
 private void SelectSquad(string key)
 {
  if (!_active) return;
  RememberSelectedStaff(); _selectedSquad = key;
  RenderSquads(SquadGroups.Options(_allRows.Select(row => row.Group))); ApplySquadFilter(); SquadChanged?.Invoke(key);
 }
 private void ApplySquadFilter()
 {
  var rows = _selectedSquad == SquadGroups.All ? _allRows : _allRows.Where(row => SquadGroups.Key(row.Group) == _selectedSquad).ToList();
  _selectedStaffBySquad.TryGetValue(_selectedSquad ?? SquadGroups.All, out var selected);
  _staff.ItemsSource = rows; _staff.SelectedItem = rows.FirstOrDefault(row => row.Person.Identity == selected) ?? rows.FirstOrDefault();
  AbilitySafeGridSort.Refresh(_staff, nameof(StaffReviewRow.Name), path => path == "Person.Ca" && AbilityVisibility.Current.HideCa, AbilityVisibility.Current.HideCa);
  string label = SquadGroups.Options(_allRows.Select(row => row.Group)).FirstOrDefault(option => option.Key == _selectedSquad)?.Label ?? "전체";
  _count.Text = $"{label} 스태프 {rows.Count:N0}명 / 구단 전체 {_allRows.Count:N0}명" + (_partial ? $" · {_evaluatedCount:N0}명 평가 / 부분 결과" : "");
  ShowStaff(); UpdateButtons();
 }
 private static StaffReviewRow PendingRow(EntityRecord person) => new() { Person = person, Role = person.Role, Group = person.Group, Reasons = ["아직 평가되지 않았습니다. 후보 검토를 완료하면 직무 점수·명성과 비교 결과가 표시됩니다."] };
 private void ShowStaff()
 {
  string? selectedCandidate = (_candidates.SelectedItem as StaffReviewCandidate)?.Person.Identity;
  DetachCandidates(); _staffDetail.Children.Clear(); _candidateDetail.Children.Clear(); _candidatePanel.Visibility = Visibility.Collapsed;
  if (_staff.SelectedItem is not StaffReviewRow row) { _staffPanel.Visibility = Visibility.Collapsed; UpdateButtons(); return; }
  _staffPanel.Visibility = Visibility.Visible; _staffDetail.Children.Add(Text(row.Name + " · " + row.Role + (row.Group.Length > 0 ? " · " + row.Group : ""), 16, true));
  _staffDetail.Children.Add(Text((row.StatusLabel.Length == 0 ? "자료 부족" : row.StatusLabel) + (_partial ? " · 전체 비교가 완료되지 않았습니다." : ""), 13, true));
  foreach (string reason in row.Reasons) _staffDetail.Children.Add(Text("• " + reason));
  _staffDetail.Children.Add(Text($"{(row.ScoreLabel.Length == 0 ? "직무 점수" : row.ScoreLabel)} {StaffReviewFormat.Number(row.Score, "0.#")} / {row.MaxScore:0} · 명성 {StaffReviewFormat.Number(row.Reputation)} · 주급 {StaffReviewFormat.Wage(row.WeeklyWage)} · 계약 만료 {StaffReviewFormat.Date(row.ContractExpires)}", 11));
  Components(_staffDetail, row.Components);
  _staffDetail.Children.Add(Text("대체 후보 · 직무 점수 비교", 15, true));
  _staffDetail.Children.Add(Text("소속·계약·명성에 따른 비교이며 실제 영입 의사는 확인되지 않았습니다.", 11));
  _candidates.ItemsSource = row.Alternatives; _staffDetail.Children.Add(_candidates); _staffDetail.Children.Add(_openCandidate); _staffDetail.Children.Add(_candidatePanel);
  if (row.Alternatives.Count == 0) _staffDetail.Children.Add(Text(_last?.Complete == true ? "검토 범위 안에서 비교할 대체 후보가 없습니다." : "아직 비교할 후보가 없습니다. 후보 검토를 완료하면 결과가 표시됩니다.", 11));
  _candidates.SelectedItem = row.Alternatives.FirstOrDefault(candidate => candidate.Person.Identity == selectedCandidate) ?? row.Alternatives.FirstOrDefault(); ShowCandidate(); UpdateButtons();
 }
 private void DetachCandidates()
 {
  foreach (FrameworkElement child in new FrameworkElement[] { _candidates, _openCandidate, _candidatePanel }) if (child.Parent is System.Windows.Controls.Panel parent) parent.Children.Remove(child);
 }
 private void ShowCandidate()
 {
  _candidateDetail.Children.Clear();
  if (_candidates.SelectedItem is not StaffReviewCandidate candidate) { _candidatePanel.Visibility = Visibility.Collapsed; UpdateButtons(); return; }
  _candidatePanel.Visibility = Visibility.Visible; _candidateDetail.Children.Add(Text(candidate.Name + " · " + (candidate.Person.Club.Length == 0 ? "소속 정보 없음" : candidate.Person.Club), 14, true));
  _candidateDetail.Children.Add(Text($"직무 점수 {candidate.Score:0.#} / 300 · 현재 스태프보다 {candidate.Improvement:+0.#;-0.#;0} · 명성 {StaffReviewFormat.Number(candidate.Reputation)}", 12));
  _candidateDetail.Children.Add(Text("가능성 추정 · " + candidate.AvailabilityLabel + " / 영입 의사 미확인", 12, true));
  _candidateDetail.Children.Add(Text(candidate.RoleFit switch { "same" => "현재 등록 직무가 일치합니다.", "adjacent" => "인접 직무 후보 · 해당 직무 수락 여부는 확인되지 않았습니다.", _ => "현재 직무 적합 여부를 확인할 수 없습니다." }, 11));
  _candidateDetail.Children.Add(Text($"확인된 자료 · 주급 {StaffReviewFormat.Wage(candidate.WeeklyWage)} · 계약 만료 {StaffReviewFormat.Date(candidate.ContractExpires)}\n소속 구단 명성 {StaffReviewFormat.Number(candidate.ClubReputation)} · 구단과 후보의 명성 차이 {StaffReviewFormat.Number(candidate.ReputationGap)}", 11));
  foreach (string reason in candidate.Reasons) _candidateDetail.Children.Add(Text("• " + reason, 11));
  Components(_candidateDetail, candidate.Components); UpdateButtons();
 }
 private static void Components(StackPanel host, List<StaffReviewComponent> components)
 {
  if (components.Count == 0) return; var grid = new AdaptiveColumnsPanel { MinColumnWidth = 170, MaxColumns = 3, ColumnGap = 8 };
  foreach (var component in components)
  {
   var text = Text(component.Label + " " + component.DisplayValue, 11);
   if (component.Weight.HasValue && component.Contribution.HasValue) text.ToolTip = $"{component.DisplayValue} × {component.Weight:0.##} = {component.Contribution:0.##}점";
   grid.Children.Add(text);
  }
  host.Children.Add(grid);
 }
 private async Task OpenAsync(EntityRecord? entity)
 {
  if (!_active || _busy || entity == null || _open == null) return;
  try { await _open(entity); } catch (Exception error) { App.Log(error); if (_active) _status.Text = "프로필을 열지 못했습니다 · " + error.Message; }
 }
 private void RequestCancel() { if (!_busy) return; _cancelRequested = true; _status.Text = "현재 조회를 마친 뒤 검토를 중지합니다…"; UpdateButtons(); }
 private async Task RunAsync(Func<Task> work)
 {
  if (!_active || _busy) return; _busy = true; _cancelRequested = false; var completion = _inflight = new(TaskCreationOptions.RunContinuationsAsynchronously); BusyChanged?.Invoke(true); UpdateButtons();
  try { await work(); }
  catch (Exception error) { App.Log(error); if (_active) { _status.Text = "평가 실패 · " + error.Message; _partial = true; } }
  finally { _busy = false; completion.TrySetResult(); UpdateButtons(); BusyChanged?.Invoke(false); }
 }
 private void AbilityVisibilityChanged(object? sender, EventArgs args)
 {
  if (!Dispatcher.CheckAccess()) { Dispatcher.Invoke(() => AbilityVisibilityChanged(sender, args)); return; }
  _staff.Items.Refresh();
  foreach (var column in _staff.Columns.Where(column => column.SortMemberPath == "Person.Ca")) column.CanUserSort = !AbilityVisibility.Current.HideCa;
  AbilitySafeGridSort.Refresh(_staff, nameof(StaffReviewRow.Name), path => path == "Person.Ca" && AbilityVisibility.Current.HideCa, AbilityVisibility.Current.HideCa);
 }
 public void Invalidate() { AbilityVisibility.Current.Changed -= AbilityVisibilityChanged; _active = false; ++_generation; _cancelRequested = true; UpdateButtons(); }
 public async Task StopAsync() { Invalidate(); var work = _inflight; if (work != null) await work.Task; }

 public async Task<object> QaAbilityVisibilityAsync(string directory)
 {
  if (_live || !IsLoaded) throw new InvalidOperationException("능력치 표시 QA는 표시된 스냅샷 스태프 명단에서 실행합니다.");
  if (!_loaded) await LoadAsync();
  SelectSquad(SquadGroups.All);
  var row = _staff.Items.OfType<StaffReviewRow>().FirstOrDefault(row => row.Person.Ca.HasValue) ?? throw new InvalidOperationException("CA QA 스태프가 없습니다.");
  var column = _staff.Columns.OfType<DataGridTextColumn>().Single(column => (column.Binding as Binding)?.Path.Path == nameof(StaffReviewRow.CaText));
  bool originalCa = AbilityVisibility.Current.HideCa, originalPa = AbilityVisibility.Current.HidePa;
  var raw = (row.Person.Ca, row.Person.Pa); var reports = new List<object>(); Directory.CreateDirectory(directory);
  try
  {
   foreach (var (hideCa, hidePa) in new[] { (true, false), (false, true), (true, true), (false, false) })
   {
    AbilityVisibility.Current.Set(hideCa, hidePa);
    string actual = await WorkbenchWindow.ReadAbilityQaCellAsync(_staff, row, column);
    if (actual != AbilityVisibility.Current.Format("ca", row.Person.Ca) || (column.GetCellContent(row) as TextBlock)?.ToolTip?.ToString() != actual) throw new InvalidOperationException("스태프 CA 셀/툴팁 숨김 불일치.");
    if (hideCa && (column.CanUserSort || column.SortDirection.HasValue)) throw new InvalidOperationException("숨긴 스태프 CA 정렬이 남았습니다.");
    if (raw != (row.Person.Ca, row.Person.Pa)) throw new InvalidOperationException("숨김 변경이 스태프 원본을 변경했습니다.");
    await CaptureAsync(Path.Combine(directory, $"staff-ability-{hideCa}-{hidePa}.png")); reports.Add(new { hideCa, hidePa, renderedCellAndTooltipMasked = true, rawUnchanged = true });
   }
   return new { success = true, gameWrites = false, reports };
  }
  finally { AbilityVisibility.Current.Set(originalCa, originalPa); }
 }

 public async Task<object> QaAsync(string outputDir)
 {
  if (_live) throw new InvalidOperationException("스태프 교체 평가 QA는 스냅샷에서만 실행합니다.");
  Directory.CreateDirectory(outputDir); if (!_loaded) await LoadAsync(); if (!_loaded) throw new InvalidOperationException("현재 스태프 조회 실패 · " + _status.Text);
  int rosterCount = _allRows.Count; string? initialSquad = _selectedSquad;
  var groupChecks = new List<object>();
  foreach (var option in SquadGroups.Options(_allRows.Select(row => row.Group)))
  {
   SelectSquad(option.Key);
   if (_staff.Items.Count != option.Count || option.Key != SquadGroups.All && _staff.Items.OfType<StaffReviewRow>().Any(row => SquadGroups.Key(row.Group) != option.Key)) throw new InvalidOperationException("스태프 선수단 분류/인원 검증 실패: " + option.Label);
   groupChecks.Add(new { option.Key, option.Count });
   await CaptureAsync(Path.Combine(outputDir, "staff-squad-" + groupChecks.Count + ".png"));
  }
  SelectSquad(initialSquad ?? SquadGroups.All); await CaptureAsync(Path.Combine(outputDir, "staff-before-review.png"));
  await StartAsync(true); if (_last is not { Cancelled: true, Complete: false } || _reviewId != null || _busy) throw new InvalidOperationException("스태프 평가 취소 검증 실패 · " + _status.Text);
  if (_allRows.Count != rosterCount || _allRows.Any(row => row.Status == "replace_review") || _selectedSquad != initialSquad) throw new InvalidOperationException("중지된 검토가 전체 명단 또는 선택한 선수단을 바꾸었습니다.");
  await CaptureAsync(Path.Combine(outputDir, "staff-cancelled.png"));
  await StartAsync(); if (_last is not { Complete: true, Cancelled: false } || _report == null || _reviewId != null || _busy) throw new InvalidOperationException("스태프 평가 완료 검증 실패 · " + _status.Text);
  if (_report.Rows.Count != rosterCount) throw new InvalidOperationException("평가 후 현재 스태프 목록이 누락되었습니다.");
  if (_report.Rows.Any(row => row.Person.Kind != "Staff" || row.Person.ClubId != _club.Id)) throw new InvalidOperationException("현재 구단 이외의 스태프가 목록에 섞였습니다.");
  if (_report.Rows.SelectMany(row => row.Alternatives).Any(candidate => candidate.Person.ClubId == _club.Id)) throw new InvalidOperationException("현재 구단 스태프를 교체 후보로 표시했습니다.");
  if (_selectedSquad != initialSquad) throw new InvalidOperationException("후보 검토 후 선택한 선수단이 변경되었습니다.");
  var withCandidates = _report.Rows.FirstOrDefault(row => row.Alternatives.Count > 0) ?? _report.Rows.FirstOrDefault();
  if (withCandidates != null) SelectSquad(SquadGroups.Key(withCandidates.Group));
  _staff.SelectedItem = withCandidates;
  await CaptureAsync(Path.Combine(outputDir, "staff-review-complete.png"));
  if (withCandidates != null)
  {
   _candidates.SelectedItem = withCandidates.Alternatives.FirstOrDefault(); await CaptureAsync(Path.Combine(outputDir, "staff-review-candidates.png"), true);
   if (_open != null && (!_openStaff.IsEnabled || withCandidates.Alternatives.Count > 0 && !_openCandidate.IsEnabled)) throw new InvalidOperationException("스태프/후보 프로필 연결이 비활성화되었습니다.");
  }
  return new { success = true, live = false, club = _club.Identity, rosterCount, groups = groupChecks, initialSquad, squadPreservedAcrossReview = true, cancellation = true, completed = _last.Complete, coverage = _report.Coverage, rows = _report.Rows.Select(row => new { person = row.Person.Identity, row.Status, row.Score, reasons = row.Reasons.Count, candidates = row.Alternatives.Count }).ToArray(), gameWrites = 0 };
 }
 private async Task CaptureAsync(string path, bool candidate = false)
 {
  var window = Window.GetWindow(this) ?? throw new InvalidOperationException("스태프 평가 화면을 표시한 뒤 QA를 실행하세요."); ScrollViewer? scroll = null;
  for (DependencyObject? element = VisualTreeHelper.GetParent(this); element != null; element = VisualTreeHelper.GetParent(element)) if (element is ScrollViewer viewer) { scroll = viewer; break; }
  try
  {
   await Dispatcher.InvokeAsync(window.UpdateLayout, DispatcherPriority.Render); if (candidate && _candidatePanel.Visibility == Visibility.Visible) _candidatePanel.BringIntoView(); else scroll?.ScrollToTop();
   await Dispatcher.InvokeAsync(window.UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
   var bitmap = new RenderTargetBitmap((int)Math.Ceiling(window.ActualWidth), (int)Math.Ceiling(window.ActualHeight), 96, 96, PixelFormats.Pbgra32); bitmap.Render(window); var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(path); encoder.Save(stream);
  }
  finally { scroll?.ScrollToTop(); await Dispatcher.InvokeAsync(window.UpdateLayout, DispatcherPriority.Render); }
 }
}
