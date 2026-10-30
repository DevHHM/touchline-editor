using System.ComponentModel;
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

/// <summary>Semantic person relationship editor. All reads and reviewed writes use the private bridge.</summary>
public sealed class PersonRelationsView : UserControl
{
 private readonly BridgeClient _bridge;
 private readonly EntityRecord _person;
 private readonly bool _live;
 private readonly Func<EntityRecord, Task>? _open;
 private readonly Func<Task>? _committed;
 private readonly Func<string, Task>? _failed;
 private readonly StackPanel _root = new(), _editor = new(), _review = new();
 private readonly Border _editorPanel, _reviewPanel;
 private readonly TextBlock _status = Text("관계 정보를 불러오세요."), _targetText = Text("대상을 선택하세요."), _donorText = Text("");
 private readonly Button _previewButton = Button("변경 미리보기"), _applyButton = Button("검토한 변경을 FM에 적용"), _pickButton = Button("대상 선택…");
 private readonly CheckBox _confirmed = new() { Content = "변경 전후를 확인했습니다", Margin = new Thickness(0, 10, 0, 8), Foreground = Brush("InkBrush") };
 private readonly ComboBox _reason = Choices(), _condition = Choices(), _job = Choices(), _scope = Choices(), _socialKind = Choices(), _careerType = Choices();
 private readonly TextBox _level = Input("80");
 private readonly List<(Button Button, Func<bool>? Enabled)> _actions = [];
 private readonly CancellationTokenSource _lifetime = new();
 private string _section, _action = "", _nameField = "", _baseline = "";
 private int _slot, _generation, _draftVersion;
 private bool _active = true, _busy, _suppress, _dirty;
 private EntityRecord? _target;
 private CareerTargetOption? _careerTarget;
 private uint? _nation;
 private PersonRelationsData? _data;
 private PersonRelationPreview? _preview;
 private Window? _dialog;
 private TaskCompletionSource? _inflight;
 public event Action<bool>? BusyChanged;
 public bool IsBusy => _busy;
 // Selecting a target is a read-only modal interaction, not a mutation.
 // Keep watching FM so a reload can close this picker and invalidate its draft.
 public bool CanPollConnection => !_busy || _dialog is CareerTargetPicker or RelationEntityPicker or NameLocalizationWindow {IsBusy:false};
 public bool HasPendingChanges => _dirty || _preview != null;
 public object? CapturePendingDraft() => !HasPendingChanges ? null : new {
  kind = _person.Kind, id = _person.Id, person = _person.DisplayName,
  action = _action, slot = _slot, target = _target == null ? null : new { kind = _target.Kind, id = _target.Id, name = _target.DisplayName },
  condition = _condition.SelectedValue, scope = _scope.SelectedValue, relation_kind = _socialKind.SelectedValue, target_type = _careerType.SelectedValue, job = _job.SelectedValue, reason = _reason.SelectedValue,
  level = _level.Text, nation = _nation, name_field = _nameField,
  career_target = _careerTarget,
  note = "이전 세이브의 미적용 초안 · 자동으로 다시 적용하지 않음"
 };

 public PersonRelationsView(BridgeClient bridge, EntityRecord person, bool live, string section, Func<EntityRecord, Task>? open = null, Func<Task>? committed = null, Func<string, Task>? failed = null)
 {
  _bridge = bridge; _person = person; _live = live; _section = NormalizeSection(section); _open = open; _committed = committed; _failed = failed;
  _editorPanel = Panel(_editor); _reviewPanel = Panel(_review);
  _editorPanel.SetBinding(VisibilityProperty, new Binding(nameof(Visibility)) { Source = _editor });
  _reviewPanel.SetBinding(VisibilityProperty, new Binding(nameof(Visibility)) { Source = _review });
  FontFamily = Typography.AppFont; FontWeight = FontWeights.Medium; Foreground = Brush("InkBrush"); FontSize = 13; UseLayoutRounding = true; SnapsToDevicePixels = true;
  TextOptions.SetTextFormattingMode(this, TextFormattingMode.Display); SetResourceReference(TextOptions.TextRenderingModeProperty, "AppTextRenderingMode");
  _status.TextWrapping = TextWrapping.Wrap; Content = _root;
  _reason.ItemsSource = new[] { new PersonRelationOption { Value = 0, Label = "없음" }, new() { Value = 1, Label = "서포터" }, new() { Value = 2, Label = "선수" }, new() { Value = 3, Label = "감독" }, new() { Value = 4, Label = "스태프" } };
  foreach (var choice in new[] { _reason, _condition, _job, _scope }) choice.SelectionChanged += (_, _) => DraftChanged();
  _scope.ItemsSource = new[] { new PersonRelationOption {Value=1,Label="구단"}, new() {Value=2,Label="국가대표"} };
  _socialKind.SelectionChanged += (_,_) => { if (_suppress) return; bool wantsClub=_socialKind.SelectedValue is int kind&&kind>=3; if(_target!=null&&(_target.Kind=="Club")!=wantsClub){_target=null;_targetText.Text="대상을 선택하세요.";}SetSocialReasons();DraftChanged(); };
  _careerType.ItemsSource = new[] {new PersonRelationOption {Value=3,Label="구단"},new() {Value=25,Label="대회"},new() {Value=9,Label="국가"},new() {Value=5,Label="대륙"}};
  _careerType.SelectionChanged += (_,_) => {if(_suppress)return;_careerTarget=null;_target=null;_targetText.Text="대상을 선택하세요.";DraftChanged();};
  _level.TextChanged += (_, _) => DraftChanged();
  _pickButton.Click += async (_, _) => await RunAsync(PickTargetAsync);
  _previewButton.Click += async (_, _) => await RunAsync(PreviewAsync);
  _applyButton.Click += async (_, _) => await ApplyAsync();
  _confirmed.Checked += (_, _) => UpdateButtons(); _confirmed.Unchecked += (_, _) => UpdateButtons();
  BuildContent();
 }
 private static string NormalizeSection(string section) => section switch { "커리어 계획" or "career" => "career", "국적·이름" or "identity" => "identity", _ => "relations" };
 private static T Decode<T>(JsonElement value) => value.Deserialize<T>(JsonDefaults.Options) ?? throw new InvalidDataException("관계 응답을 읽지 못했습니다.");
 private static Brush Brush(string key) => Application.Current.TryFindResource(key) as Brush ?? Brushes.White;
 private static TextBlock Text(string text, double size = 12, bool bold = false) => new() { Text = text, FontSize = size, FontWeight = bold ? FontWeights.Bold : FontWeights.Medium, Foreground = Brush(bold ? "InkBrush" : "MutedBrush"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 0, 7) };
 private static Button Button(string text) => new() { Content = text, Padding = new Thickness(12, 7, 12, 7), Margin = new Thickness(0, 0, 7, 7), FontWeight = FontWeights.Medium };
 private static TextBox Input(string value = "") => new() { Text = value, MinHeight = 32, Padding = new Thickness(9, 6, 9, 6), FontWeight = FontWeights.Medium };
 private static ComboBox Choices() => new() { DisplayMemberPath = "Label", SelectedValuePath = "Value", MinHeight = 32, FontWeight = FontWeights.Medium, MaxDropDownHeight = 320 };
 private static Border Panel(UIElement content) => new() { Child = content, Padding = new Thickness(15), Background = Brush("PanelBrush"), BorderBrush = Brush("BorderBrush"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(7), Margin = new Thickness(0, 0, 0, 12) };
 private static DataGrid Table() => new() { AutoGenerateColumns = false, IsReadOnly = true, CanUserAddRows = false, CanUserDeleteRows = false, SelectionMode = DataGridSelectionMode.Single, RowHeight = 37, MinHeight = 100, MaxHeight = 255, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
 private static void Column(DataGrid grid, string name, string property, double width, bool star = false, string? tooltip = null)
 {
  var style = new Style(typeof(TextBlock), Application.Current.TryFindResource(typeof(TextBlock)) as Style); style.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis)); style.Setters.Add(new Setter(ToolTipService.ToolTipProperty, new Binding(tooltip ?? property)));
  grid.Columns.Add(new DataGridTextColumn { Header = name, Binding = new Binding(property), ElementStyle = style, Width = new DataGridLength(width, star ? DataGridLengthUnitType.Star : DataGridLengthUnitType.Pixel), MinWidth = star ? 140 : Math.Min(width, 70) });
 }
 private static FrameworkElement Field(string label, FrameworkElement input)
 {
  var panel = new StackPanel { Margin = new Thickness(0, 0, 12, 10), MinWidth = 145 }; panel.Children.Add(Text(label)); panel.Children.Add(input); return panel;
 }
 private Button Action(string label, Func<Task> work, Func<bool>? enabled = null, string? hint = null)
 {
  var button = Button(label); button.ToolTip = hint; ToolTipService.SetShowOnDisabled(button, true); button.Click += async (_, _) => { if (enabled?.Invoke() != false) await RunAsync(work); }; _actions.Add((button, enabled)); return button;
 }
 private void DetachEditorControls()
 {
  foreach (FrameworkElement element in new FrameworkElement[] { _reason, _condition, _job, _scope, _socialKind, _careerType, _level, _pickButton, _previewButton, _targetText, _donorText })
   if (element.Parent is System.Windows.Controls.Panel parent) parent.Children.Remove(element);
 }
 private void BuildContent()
 {
  _root.Children.Clear(); _actions.Clear(); DetachEditorControls(); _editor.Children.Clear(); _review.Children.Clear(); _editor.Visibility = Visibility.Collapsed; _review.Visibility = Visibility.Collapsed;
  _root.Children.Add(Text(_section == "career" ? "커리어 계획" : _section == "identity" ? "국적·이름" : "선호·관계", 20, true));
  var commands = new WrapPanel(); commands.Children.Add(Action("다시 읽기", RefreshAsync)); _root.Children.Add(commands);
  _root.Children.Add(Text(_live ? "변경은 미리보기와 확인을 거친 뒤 적용됩니다." : "스냅샷 · 변경 미리보기만 가능하며 FM에는 적용되지 않습니다."));
  if (_data == null) _root.Children.Add(Text("아직 이 인물의 관계 자료를 불러오지 않았습니다."));
  else if (_section == "career") BuildCareer();
  else if (_section == "identity") BuildIdentity();
  else BuildRelations();
  if (_data != null) Notes(_data.Notes);
  _root.Children.Add(_editorPanel); _root.Children.Add(_reviewPanel); _status.Margin = new Thickness(0, 2, 0, 0); _root.Children.Add(_status); UpdateButtons();
 }
 private void BuildCareer()
 {
  var career = _data!.Career;
  if (!career.Available) _root.Children.Add(Text("이 자료에서는 커리어 계획 목록을 확인할 수 없습니다."));
  var grid = Table(); grid.ItemsSource = career.Rows; Column(grid, "계획", nameof(PersonCareerRow.ConditionLabel), 150); Column(grid, "대상", nameof(PersonCareerRow.TargetName), 1, true); Column(grid, "직무", nameof(PersonCareerRow.JobLabel), 140); Column(grid, "범위", nameof(PersonCareerRow.ScopeLabel), 110); Column(grid, "상태", nameof(PersonCareerRow.Access), 95, tooltip: nameof(PersonCareerRow.AccessReason));
  grid.MouseDoubleClick += async (_, _) => { if (grid.SelectedItem is PersonCareerRow row && row.Target != null) await RunAsync(() => OpenTargetAsync(row.Target, row.ObjectType)); };
  _root.Children.Add(Panel(grid));
  var actions = new WrapPanel(); var edit = Action("선택한 계획 편집", () => { if (grid.SelectedItem is PersonCareerRow row) BeginCareerTarget(row); return Task.CompletedTask; }, () => grid.SelectedItem is PersonCareerRow { EditableDetails: true }); grid.SelectionChanged += (_, _) => UpdateButtons(); actions.Children.Add(edit);
  actions.Children.Add(Action("선택한 계획 삭제",()=>{if(grid.SelectedItem is PersonCareerRow row)BeginDelete("delete_career_plan",row.Slot,"커리어 계획",$"{row.ConditionLabel} · {row.TargetName} · {row.JobLabel}");return Task.CompletedTask;},()=>grid.SelectedItem is PersonCareerRow {CanDelete:true}));
  string addReason = AddExplanation(career.CanAdd, career.AddReason, "커리어 계획");
  actions.Children.Add(Action("커리어 계획 추가",()=>{BeginCareerTarget(null);return Task.CompletedTask;},()=>career.CanAdd,addReason));
  _root.Children.Add(actions);
  if (career.Rows.Count == 0) _root.Children.Add(Text("저장된 커리어 계획이 없습니다."));
  _root.Children.Add(Text(addReason,11));
  Notes(career.Notes);
 }
 private void BuildRelations()
 {
  var rows = _data!.Rows.Where(row => row.Kind != 9).ToList(); var grid = Table(); grid.ItemsSource = rows; Column(grid, "관계", nameof(PersonRelationRow.KindLabel), 150); Column(grid, "대상", nameof(PersonRelationRow.TargetName), 1, true); Column(grid, "이유", nameof(PersonRelationRow.ReasonLabel), 90); Column(grid, "선호도", nameof(PersonRelationRow.Strength), 65); Column(grid, "상태", nameof(PersonRelationRow.Access), 95, tooltip: nameof(PersonRelationRow.AccessReason));
  grid.MouseDoubleClick += async (_, _) => { if (grid.SelectedItem is PersonRelationRow { Target: { } target } row) await RunAsync(() => OpenTargetAsync(target, row.ObjectType)); }; _root.Children.Add(Panel(grid));
  var actions = new WrapPanel(); var edit = Action("선택한 관계 편집", () => { if (grid.SelectedItem is PersonRelationRow row) BeginSocial(row); return Task.CompletedTask; }, () => grid.SelectedItem is PersonRelationRow { EditableSocial: true }); grid.SelectionChanged += (_, _) => UpdateButtons(); actions.Children.Add(edit);
  string addReason = AddExplanation(_data.CanAddFavourite, _data.FavouriteAddExplanation, "선호·비선호 관계");
  var add = Action("관계 추가", () => { BeginSocial(null); return Task.CompletedTask; }, () => _data.CanAddFavourite, addReason); actions.Children.Add(add); _root.Children.Add(actions);
  actions.Children.Add(Action("선택한 관계 삭제",()=>{if(grid.SelectedItem is PersonRelationRow row)BeginDelete("delete_social_relation",row.Slot,row.KindLabel,$"{row.TargetName} · {row.ReasonLabel} · 강도 {row.Level}");return Task.CompletedTask;},()=>grid.SelectedItem is PersonRelationRow {EditableSocial:true,CanDelete:true}));
  _root.Children.Add(Text("좋아하는·싫어하는 인물과 구단을 추가·편집·삭제할 수 있습니다. 대회 등록·홈그로운 등 게임 관리 정보는 선호 관계와 별개입니다.", 11));
  if (rows.Count == 0) _root.Children.Add(Text("저장된 선호·관계가 없습니다."));
  _root.Children.Add(Text(addReason,11));
 }
 private void BuildIdentity()
 {
  if (_person.Kind == "Player")
  {
   var names = new StackPanel(); names.Children.Add(Text("선수명 한글화", 16, true)); names.Children.Add(Text("원래 이름·성은 유지하고, 직접 작성하거나 기존 한글 표기를 선택해 FM 표시 이름을 바꿉니다. 파일 설치 후 FM 재시작이 필요합니다."));
   names.Children.Add(Action("선수명 한글화…", OpenNameLocalizationAsync)); _root.Children.Add(Panel(names));
  }
  _root.Children.Add(Text("기본 국적 · " + (_data!.PrimaryNationality?.Name ?? "확인할 수 없음"), 15, true));
  var rows = _data.Rows.Where(row => row.Kind == 9).ToList(); var grid = Table(); grid.MaxHeight = 170; grid.ItemsSource = rows; Column(grid, "추가 국적", nameof(PersonRelationRow.TargetName), 1, true); Column(grid, "국가 ID", nameof(PersonRelationRow.TargetId), 100); Column(grid, "상태", nameof(PersonRelationRow.Access), 100, tooltip: nameof(PersonRelationRow.AccessReason)); _root.Children.Add(Panel(grid));
  var actions = new WrapPanel(); var edit = Action("추가 국적 바꾸기", () => { if (grid.SelectedItem is PersonRelationRow row) BeginNationality(row); return Task.CompletedTask; }, () => grid.SelectedItem is PersonRelationRow { EditableSecondaryNationality: true }); grid.SelectionChanged += (_, _) => UpdateButtons(); actions.Children.Add(edit);
  string addReason = AddExplanation(_data.CanAddSecondaryNationality, _data.SecondaryNationalityAddExplanation, "추가 국적");
  var add = Action("추가 국적 추가", () => { BeginNationality(null); return Task.CompletedTask; }, () => _data.CanAddSecondaryNationality, addReason); actions.Children.Add(add); _root.Children.Add(actions);
  actions.Children.Add(Action("선택한 추가 국적 삭제",()=>{if(grid.SelectedItem is PersonRelationRow row)BeginDelete("delete_secondary_nationality",row.Slot,"추가 국적",row.TargetName);return Task.CompletedTask;},()=>grid.SelectedItem is PersonRelationRow {Kind:9,ObjectType:8,CanDelete:true}));
  _root.Children.Add(Text("선택한 인물의 기본 국적을 추가 국적으로 사용합니다. 출전 자격·취업허가·국가대표 등록은 자동 변경되지 않습니다.", 11));
  _root.Children.Add(Text(addReason,11));
  _root.Children.Add(Text("데이터베이스 이름", 16, true));
  foreach (var name in _data.Names)
  {
   var row = new DockPanel { Margin = new Thickness(0, 0, 0, 5) }; var reuse = Action("다른 인물에서 선택…", () => { BeginName(name); return Task.CompletedTask; }); DockPanel.SetDock(reuse, Dock.Right); row.Children.Add(reuse); row.Children.Add(Text($"{name.Label} · {(name.Text.Length == 0 ? "없음" : name.Text)}", 13)); _root.Children.Add(row);
  }
  _root.Children.Add(Text("이 항목은 게임에 이미 존재하는 원본 이름을 재사용합니다. 한글 표시명을 직접 입력하려면 위의 선수명 한글화를 사용하세요.", 11));
 }
 private Task OpenNameLocalizationAsync()
 {
  if (_person.Kind != "Player") return Task.CompletedTask;
  var window = new NameLocalizationWindow(_bridge, [_person.Id], _person.DisplayName) { Owner = Window.GetWindow(this) };
  _dialog = window; try { window.ShowDialog(); } finally { _dialog = null; }
  if (window.FilesChanged) _status.Text = "한글화 파일 변경 완료 · FM 재시작 후 세이브 다시 불러오기";
  return Task.CompletedTask;
 }
 private void Notes(IEnumerable<string> notes) { foreach (string note in notes) _root.Children.Add(Text(note, 11)); }
 private static string AddExplanation(bool canAdd, string reason, string label) => string.IsNullOrWhiteSpace(reason) ? canAdd ? $"새 {label}을 추가할 수 있습니다." : $"현재 자료에서는 {label} 추가를 지원하지 않습니다. 다시 읽어 상태를 확인하세요." : reason;
 public async Task LoadAsync() => await RunAsync(LoadCoreAsync);
 private async Task LoadCoreAsync()
 {
  int generation = _generation; var data = Decode<PersonRelationsData>(await _bridge.CallAsync("relation_detail", new { kind = _person.Kind, id = _person.Id }, _lifetime.Token));
  if (!_active || generation != _generation) return;
  _data = data; ResetDraft(); BuildContent(); _status.Text = $"{_person.DisplayName} · 관계 {data.Rows.Count}개 · 커리어 계획 {data.Career.Rows.Count}개";
 }
 private async Task RefreshAsync()
 {
  if (HasPendingChanges && AppDialog.Show(Window.GetWindow(this), "작성 중인 관계 변경을 버리고 다시 읽을까요?", "관계 정보 새로 읽기", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
  ResetDraft(); await LoadCoreAsync();
 }
 private void ResetDraft() { ++_draftVersion; _action = ""; _dirty = false; _target = null; _careerTarget=null; _nation = null; _baseline = ""; ClearPreview(); _editor.Visibility = Visibility.Collapsed; UpdateButtons(); }
 private bool StartDraft(string action, int slot = 0)
 {
  if (HasPendingChanges && AppDialog.Show(Window.GetWindow(this), "작성 중인 관계 변경을 버리고 다른 항목을 편집할까요?", "관계 변경", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return false;
  ++_draftVersion; _suppress = true; _action = action; _slot = slot; _target = null; _careerTarget=null; _nation = null; _nameField = ""; _targetText.Text = "대상을 선택하세요."; _donorText.Text = ""; DetachEditorControls(); _editor.Children.Clear(); ClearPreview(); _editor.Visibility = Visibility.Visible; return true;
 }
 private void FinishDraft(bool pickTarget = true)
 {
  if(pickTarget){var targetLine = new DockPanel(); DockPanel.SetDock(_pickButton, Dock.Right); targetLine.Children.Add(_pickButton); targetLine.Children.Add(_targetText); _editor.Children.Add(targetLine); _editor.Children.Add(_donorText);}
  var actions = new WrapPanel(); actions.Children.Add(_previewButton); var cancel = Button("작성 취소"); cancel.Click += (_, _) => ResetDraft(); actions.Children.Add(cancel); _editor.Children.Add(actions); _suppress = false; _baseline = Fingerprint(); _dirty = false; UpdateButtons();
 }
 private void BeginCareerTarget(PersonCareerRow? row)
 {
  if(!StartDraft(row==null?"add_career_target":"set_career_target",row?.Slot??0))return;
  _editor.Children.Add(Text(row==null?"커리어 계획 추가":"커리어 계획 편집",16,true));
  _condition.ItemsSource=_data!.Options.CareerConditions;_condition.SelectedValue=row?.Condition??1;
  _job.ItemsSource=_data.Options.CareerJobs;_job.SelectedValue=row?.Job??(_person.Kind=="Player"?1:20);
  _scope.SelectedValue=row?.Scope??1;_careerType.SelectedValue=row?.ObjectType is 3 or 5 or 9 or 25 ? row.ObjectType : 3;
  var line=new WrapPanel();line.Children.Add(Field("대상 종류",_careerType));line.Children.Add(Field("계획",_condition));line.Children.Add(Field("범위",_scope));line.Children.Add(Field("직무",_job));_editor.Children.Add(line);
  if(row?.Target!=null){_careerTarget=new(){Uid=row.Target.Uid,Name=row.Target.Name,ObjectType=row.ObjectType};_targetText.Text=row.Target.Name;}
  FinishDraft();
 }
 private void BeginSocial(PersonRelationRow? row)
 {
  if(!StartDraft(row==null?"add_social_relation":"set_social_relation",row?.Slot??0))return;
  _editor.Children.Add(Text(row==null?"관계 추가":"관계 편집",16,true));
  var all=new[]{new PersonRelationOption{Value=1,Label="좋아하는 인물"},new(){Value=2,Label="싫어하는 인물"},new(){Value=3,Label="좋아하는 구단"},new(){Value=4,Label="싫어하는 구단"}};
  _socialKind.ItemsSource=row==null?all:all.Where(x=>(x.Value<=2)==(row.Kind<=2)).ToArray();
  _socialKind.SelectedValue=row?.Kind??3;SetSocialReasons();_reason.SelectedValue=row?.Reason??(_socialKind.SelectedValue is 3?1:0);_level.Text=(row?.Level??80).ToString(CultureInfo.InvariantCulture);
  var line=new WrapPanel();line.Children.Add(Field("관계 종류",_socialKind));line.Children.Add(Field("이유",_reason));line.Children.Add(Field("강도 · 0–100",_level));_editor.Children.Add(line);
  if(row?.Target!=null){_target=new(){Id=row.Target.Uid,Name=row.Target.Name,Kind=row.ObjectType==1?"Club":"Person"};_targetText.Text=row.Target.Name;}
  FinishDraft();
 }
 private void SetSocialReasons()
 {
  int kind=_socialKind.SelectedValue is int value?value:3;
  string[] labels=kind switch {1=>["없음","부모","형제자매","자녀","친척","동료","감독","선수","스태프","우상","친구","유소년 스태프"],3=>["없음","서포터","선수","감독","스태프"],_=>["없음"]};
  _reason.ItemsSource=labels.Select((label,i)=>new PersonRelationOption{Value=i,Label=label}).ToArray();_reason.SelectedValue=0;
 }
 private bool ClubPicker() => (_action is "set_career_club" or "add_career_club" or "set_favourite_club" or "add_favourite_club") || ((_action is "set_social_relation" or "add_social_relation") && _socialKind.SelectedValue is int kind && kind>=3);
 private void BeginCareer(PersonCareerRow row)
 {
  if (!StartDraft("set_career_club", row.Slot)) return; _editor.Children.Add(Text(row.Target == null ? "빈 커리어 항목 설정" : "커리어 계획 편집", 16, true));
  _condition.ItemsSource = _data!.Options.CareerConditions; _condition.SelectedValue = row.Condition; _job.ItemsSource = _data.Options.CareerJobs; _job.SelectedValue = row.Job;
  var line = new WrapPanel(); line.Children.Add(Field("계획", _condition)); line.Children.Add(Field("직무", _job)); _editor.Children.Add(line);
  if (row.Target != null) { _target = new() { Id = row.Target.Uid, Kind = "Club", Name = row.Target.Name }; _targetText.Text = row.Target.Name; } FinishDraft();
 }
 private void BeginAddCareer()
 {
  if(!StartDraft("add_career_club"))return;
  _editor.Children.Add(Text("커리어 계획 추가",16,true));
  _condition.ItemsSource=_data!.Options.CareerConditions;_condition.SelectedValue=1;
  _job.ItemsSource=_data.Options.CareerJobs;_job.SelectedValue=_person.Kind=="Player"?1:20;
  var line=new WrapPanel();line.Children.Add(Field("계획",_condition));line.Children.Add(Field("직무",_job));_editor.Children.Add(line);
  _editor.Children.Add(Text(_data.Career.AddReason));FinishDraft();
 }
 private void BeginFavourite(PersonRelationRow? row, bool add)
 {
  if (!StartDraft(add ? "add_favourite_club" : "set_favourite_club", row?.Slot??0)) return; _editor.Children.Add(Text(add ? "선호 구단 추가" : "선호 구단 편집", 16, true));
  _reason.SelectedValue = row?.Reason??1; _level.Text = (row?.Level??80).ToString(CultureInfo.InvariantCulture);
  var line = new WrapPanel(); line.Children.Add(Field("선호 이유", _reason)); line.Children.Add(Field("선호도 · 0–100", _level));
  if (!add && row?.Target != null) { _target = new() { Id = row.Target.Uid, Kind = "Club", Name = row.Target.Name }; _targetText.Text = row.Target.Name; }
  _editor.Children.Add(line); FinishDraft();
 }
 private void BeginNationality(PersonRelationRow? row)
 {
  if (!StartDraft(row == null ? "add_secondary_nationality" : "set_secondary_nationality", row?.Slot ?? 0)) return; _editor.Children.Add(Text(row == null ? "추가 국적 추가" : "추가 국적 변경", 16, true));
  _editor.Children.Add(Text(row == null ? "해당 국적을 가진 기준 인물을 선택하세요." : $"현재 국적 · {row.TargetName}\n변경할 국적을 가진 기준 인물을 선택하세요.")); FinishDraft();
 }
 private void BeginName(PersonNameRow row)
 {
  if (!StartDraft("reuse_name")) return; _nameField = row.Field; _editor.Children.Add(Text(row.Label + " 재사용", 16, true)); _editor.Children.Add(Text("현재 · " + (row.Text.Length == 0 ? "없음" : row.Text))); FinishDraft();
 }
 private void BeginDelete(string action, int slot, string label, string description)
 {
  if(!StartDraft(action,slot))return;
  _editor.Children.Add(Text(label+" 삭제",16,true));
  _editor.Children.Add(Text(description,14,true));
  _editor.Children.Add(Text("선택한 항목을 목록에서 제거합니다. 미리보기에서 대상을 확인한 뒤 적용하세요. 적용한 삭제는 변경 기록에서 되돌릴 수 있습니다."));
  FinishDraft(false);_dirty=true;
 }
 private string Fingerprint() => JsonSerializer.Serialize(new { _action, _slot, _nameField, target = _target?.Identity, _careerTarget, _nation, reason = _reason.SelectedValue, relation_kind = _socialKind.SelectedValue, target_type = _careerType.SelectedValue, scope = _scope.SelectedValue, level = _level.Text, condition = _condition.SelectedValue, job = _job.SelectedValue });
 private void DraftChanged() { if (_suppress || _action.Length == 0) return; ++_draftVersion; _dirty = Fingerprint() != _baseline; ClearPreview(); UpdateButtons(); }
 private void ClearPreview() { _preview = null; _confirmed.IsChecked = false; _review.Visibility = Visibility.Collapsed; _review.Children.Clear(); _applyButton.IsEnabled = false; }
 private Task PickTargetAsync() => PickTargetCoreAsync();
 private async Task PickTargetCoreAsync(EntityRecord? qaTarget = null, string? qaCapture = null, bool qaCancel = false)
 {
  if(_action is "add_career_target" or "set_career_target") {
   int type=_careerType.SelectedValue is int selectedType?selectedType:3;
   var catalog=new CareerTargetPicker(_bridge,type){Owner=Window.GetWindow(this)};bool? selected;
   Exception? catalogError=null;
   if(qaCancel)catalog.Loaded+=(_,_)=>catalog.Close();
   else if(qaTarget!=null)catalog.Loaded+=async(_,_)=>{try{await catalog.QaAsync(qaTarget.Id,qaCapture!);}catch(Exception error){catalogError=error;catalog.Close();}};
   _dialog=catalog;try{selected=catalog.ShowDialog();}finally{_dialog=null;}
   if(catalogError!=null)throw new InvalidOperationException("커리어 대상 검색 검증 실패",catalogError);
   if(_active&&selected==true&&catalog.Result!=null){_careerTarget=catalog.Result;_targetText.Text=_careerTarget.Name+" · ID "+_careerTarget.Uid;DraftChanged();}return;
  }
  bool club = ClubPicker();
  var picker = new RelationEntityPicker(_bridge, club) { Owner = Window.GetWindow(this) }; bool? accepted;
  Exception? qaError = null;
  if (qaTarget != null || qaCancel)
   picker.Loaded += async (_, _) => { try { if (qaCancel) picker.Close(); else await picker.QaAsync(qaTarget!, qaCapture!); } catch (Exception error) { qaError = error; picker.Close(); } };
  _dialog = picker; try { accepted = picker.ShowDialog(); } finally { _dialog = null; }
  if (qaError != null) throw new InvalidOperationException("대상 선택 창 검증 실패", qaError);
  if (!_active || accepted != true || picker.Result == null) return;
  await UseTargetAsync(picker.Result);
 }
 private async Task UseTargetAsync(EntityRecord selected)
 {
  if (!_active) return;
  bool club = ClubPicker();
  if (club ? selected.Kind != "Club" : selected.Kind is not ("Player" or "Staff" or "Person")) throw new InvalidOperationException(club ? "대상 구단을 선택하세요." : "기준 인물을 선택하세요.");
  int version = _draftVersion, generation = _generation;
  PersonRelationsData? donor = null;
  if (!club && _action is not ("add_social_relation" or "set_social_relation")) donor = Decode<PersonRelationsData>(await _bridge.CallAsync("relation_detail", new { kind = selected.Kind, id = selected.Id }, _lifetime.Token));
  if (!_active || generation != _generation || version != _draftVersion) return;
  if (_action.Contains("nationality", StringComparison.Ordinal))
  {
   if (donor?.PrimaryNationality == null) throw new InvalidOperationException("선택한 인물의 기본 국적을 확인할 수 없습니다.");
   _nation = donor.PrimaryNationality.Uid; _donorText.Text = "사용할 국적 · " + donor.PrimaryNationality.Name;
  }
  else if (_action == "reuse_name")
  {
   string value = donor?.Names.FirstOrDefault(item => item.Field == _nameField)?.Text ?? ""; if (value.Length == 0) throw new InvalidOperationException("선택한 인물의 해당 이름이 비어 있습니다."); _donorText.Text = "사용할 이름 · " + value;
  }
  _target = selected; _targetText.Text = selected.DisplayName + $" · ID {selected.Id}"; DraftChanged();
 }
 private object Operation()
 {
  if(_action is "delete_career_plan" or "delete_favourite_club" or "delete_secondary_nationality" or "delete_social_relation")return new {action=_action,slot=_slot};
  int Choice(ComboBox combo, string name) => combo.SelectedValue is int value ? value : throw new InvalidOperationException(name + "을 선택하세요.");
  if(_action is "add_career_target" or "set_career_target") {
   var target=_careerTarget??throw new InvalidOperationException("계획의 대상을 선택하세요.");
   var data=new Dictionary<string,object?>{{"action",_action},{"target",new {object_type=target.ObjectType,uid=target.Uid}},{"condition",Choice(_condition,"계획")},{"scope",Choice(_scope,"범위")},{"job",Choice(_job,"직무")}};
   if(_action=="set_career_target")data["slot"]=_slot;return data;
  }
  if (_target == null) throw new InvalidOperationException("변경에 사용할 대상을 선택하세요.");
  int Level() => int.TryParse(_level.Text, NumberStyles.Integer, CultureInfo.InvariantCulture, out int value) && value is >= 0 and <= 100 ? value : throw new InvalidOperationException("선호도는 0–100 사이의 정수입니다.");
  return _action switch
  {
   "add_social_relation" => new {action=_action,relation_kind=Choice(_socialKind,"관계 종류"),target=new {kind=_target.Kind,id=_target.Id},reason=Choice(_reason,"이유"),level=Level()},
   "set_social_relation" => new {action=_action,slot=_slot,relation_kind=Choice(_socialKind,"관계 종류"),target=new {kind=_target.Kind,id=_target.Id},reason=Choice(_reason,"이유"),level=Level()},
   "set_career_club" => new { action = _action, slot = _slot, club_id = _target.Id, condition = Choice(_condition, "계획"), job = Choice(_job, "직무") },
   "add_career_club" => new { action = _action, club_id = _target.Id, condition = Choice(_condition, "계획"), job = Choice(_job, "직무") },
   "set_favourite_club" => new { action = _action, slot = _slot, club_id = _target.Id, reason = Choice(_reason, "선호 이유"), level = Level() },
   "add_favourite_club" => new { action = _action, template_slot = (int?)null, club_id = _target.Id, reason = Choice(_reason, "선호 이유"), level = Level() },
   "set_secondary_nationality" => new { action = _action, slot = _slot, donor = new { kind = _target.Kind, id = _target.Id }, expected_nation_uid = _nation ?? throw new InvalidOperationException("기본 국적을 확인할 수 있는 인물을 선택하세요.") },
   "add_secondary_nationality" => new { action = _action, donor = new { kind = _target.Kind, id = _target.Id }, expected_nation_uid = _nation ?? throw new InvalidOperationException("기본 국적을 확인할 수 있는 인물을 선택하세요.") },
   "reuse_name" => new { action = _action, field = _nameField, donor = new { kind = _target.Kind, id = _target.Id } },
   _ => throw new InvalidOperationException("변경할 항목을 선택하세요.")
  };
 }
 private async Task PreviewAsync()
 {
  var operation = Operation(); int version = _draftVersion, generation = _generation; ClearPreview(); _status.Text="변경 미리보기를 준비하고 있습니다…"; var preview = Decode<PersonRelationPreview>(await _bridge.CallAsync("relation_preview", new { kind = _person.Kind, id = _person.Id, operation }, _lifetime.Token));
  if (!_active || version != _draftVersion || generation != _generation) return;
  if (string.IsNullOrWhiteSpace(preview.Token)) throw new InvalidDataException("검토 토큰을 받지 못했습니다."); _preview = preview;
  _review.Children.Add(Text("적용 전 검토 · " + preview.Summary, 16, true));
  foreach (var (title, value) in new[] { ("현재", preview.Before), ("변경 후", preview.After) }) { _review.Children.Add(Text(title, 12, true)); _review.Children.Add(new TextBox { Text = value, IsReadOnly = true, TextWrapping = TextWrapping.Wrap, MinHeight = 36, MaxHeight = 150, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, Margin = new Thickness(0, 0, 0, 8) }); }
  _review.Children.Add(_confirmed); _review.Children.Add(_applyButton); _review.Visibility = Visibility.Visible; _status.Text = !_live || !preview.Live ? "스냅샷 미리보기 · 실제 게임 적용은 비활성화되어 있습니다." : preview.CanApply ? "변경 전후를 확인한 다음 적용하세요." : "이 변경은 현재 적용할 수 없습니다. 다시 읽어 지원 여부를 확인하세요."; UpdateButtons();
  // The review lives below a potentially long relationship table. Rendering it
  // alone leaves the user looking at the edit button with no visible feedback.
  await Dispatcher.InvokeAsync(() => { UpdateLayout(); _reviewPanel.BringIntoView(); }, DispatcherPriority.Loaded);
 }
 private async Task ApplyAsync()
 {
  if (!_active || _busy || !_live || _preview is not { CanApply: true, Live: true } || _confirmed.IsChecked != true) return;
  var reviewed = _preview;
  if (AppDialog.Show(Window.GetWindow(this), "표시한 관계 변경을 현재 FM에 적용할까요?", "관계 변경 적용", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
  await ApplyReviewedAsync(reviewed);
 }
 private async Task ApplyReviewedAsync(PersonRelationPreview reviewed)
 {
  if (!_active || _busy || !_live || !reviewed.Live || !reviewed.CanApply || _preview != reviewed || _confirmed.IsChecked != true) return;
  string token = reviewed.Token; bool applied = false; string? applyError = null;
  await RunAsync(async () =>
  {
   ClearPreview();
   try { await _bridge.CallAsync("relation_apply", new { token }); applied = true; _dirty = false; }
   catch (Exception error) { applyError = error.Message; throw; }
   try { await LoadCoreAsync(); _status.Text = "적용 완료 · 변경 기록에서 변경 전후를 확인하고 되돌릴 수 있습니다."; }
   catch (Exception error) { App.Log(error); _data = null; ResetDraft(); BuildContent(); _status.Text = "변경은 적용되었습니다. 다시 읽기 실패 · " + error.Message; }
  });
  // Parent refresh may call StopAsync. It must run after this operation's completion source is released.
  if (applied && _committed != null) { try { await _committed(); } catch (Exception error) { App.Log(error); _status.Text = "변경은 적용되었습니다. 프로필 새로 읽기 실패 · " + error.Message; } }
  else if (applyError != null && _failed != null) { try { await _failed(applyError); } catch (Exception error) { App.Log(error); _status.Text = applyError + "\n연결 상태 확인 실패 · " + error.Message; } }
 }
 private async Task OpenTargetAsync(PersonRelationTarget target, int objectType)
 {
  if (_open == null) return;
  EntityRecord? entity = null; string? kind = target.Kind ?? (objectType == 1 ? "Club" : null);
  if (kind is "Club" or "Player" or "Staff" or "Person") entity = new() { Kind = kind, Id = target.Uid, Name = target.Name };
  else if (objectType == 3)
  {
   foreach (string candidate in new[] { "Player", "Staff", "Person" }) { var found = await _bridge.CallAsync("search", new { kind = candidate, query = target.Uid.ToString(), limit = 10, offset = 0, sort = "name" }, _lifetime.Token); entity = Decode<List<EntityRecord>>(found.GetProperty("items")).FirstOrDefault(item => item.Id == target.Uid); if (entity != null) break; }
  }
  if (entity == null) { _status.Text = "이 대상의 프로필은 현재 자료에서 열 수 없습니다."; return; }
  // Schedule navigation after RunAsync has released the bridge-work lifetime.
  _ = Dispatcher.BeginInvoke(async () => { if (_active) await _open(entity); }, DispatcherPriority.Background);
 }
 private void UpdateButtons()
 {
  _editor.IsEnabled = _active && !_busy; foreach (var (button, enabled) in _actions) button.IsEnabled = _active && !_busy && enabled?.Invoke() != false;
  _pickButton.IsEnabled = _active && !_busy; _previewButton.IsEnabled = _active && !_busy && _action.Length > 0; _confirmed.IsEnabled = _active && !_busy && _live;
  _applyButton.IsEnabled = _active && !_busy && _live && _preview is { CanApply: true, Live: true } && _confirmed.IsChecked == true;
 }
 private async Task RunAsync(Func<Task> work)
 {
  if (!_active || _busy) return; _busy = true; var completion = _inflight = new(TaskCreationOptions.RunContinuationsAsynchronously); BusyChanged?.Invoke(true); UpdateButtons();
  try { await work(); }
  catch (OperationCanceledException) { if (_active) _status.Text = "작업을 취소했습니다."; }
  catch (Exception error) { App.Log(error); if (_active) { _status.Text = "처리하지 못했습니다 · " + error.Message; await Dispatcher.InvokeAsync(() => { UpdateLayout(); _status.BringIntoView(); },DispatcherPriority.Loaded); } ClearPreview(); }
  finally { _busy = false; completion.TrySetResult(); UpdateButtons(); BusyChanged?.Invoke(false); }
 }
 public void Invalidate() { _active = false; ++_generation; ++_draftVersion; ClearPreview(); _lifetime.Cancel(); if(_dialog is NameLocalizationWindow names)names.InvalidateConnection();else _dialog?.Close(); UpdateButtons(); }
 public async Task StopAsync() { Invalidate(); var work = _inflight; if (work != null) await work.Task; }

 public async Task<object> QaAsync(string outputDir)
 {
  if (_live) throw new InvalidOperationException("관계 화면 QA는 스냅샷에서만 실행합니다.");
  if (_data == null) await LoadAsync(); if (_data == null) throw new InvalidOperationException("관계 스냅샷 자료가 필요합니다.");
  Directory.CreateDirectory(outputDir); string section = _section; var captures = new List<string>(); var previews = new List<object>();
  async Task Execute(Func<Task> work)
  {
   Exception? failure = null; bool ran = false;
   await RunAsync(async () => { ran = true; try { await work(); } catch (Exception error) { failure = error; throw; } });
   if (!ran || failure != null) throw new InvalidOperationException("관계 QA 작업 실패 · " + (failure?.Message ?? "화면이 사용 중이거나 만료되었습니다."), failure);
  }
  async Task<List<EntityRecord>> Search(string kind)
  {
   List<EntityRecord> result = []; await Execute(async () => result = Decode<List<EntityRecord>>((await _bridge.CallAsync("search", new { kind, query = "", limit = 100, offset = 0, sort = "name" }, _lifetime.Token)).GetProperty("items"))); return result;
  }
  async Task Review(string operation)
  {
   await Execute(PreviewAsync);
   var host=Window.GetWindow(this)??throw new InvalidOperationException("미리보기 창이 없습니다.");
   await Dispatcher.InvokeAsync(host.UpdateLayout,DispatcherPriority.Render);
   var applyPoint=_applyButton.TransformToAncestor(host).Transform(new Point());
   if(!_review.IsVisible||applyPoint.Y<0||applyPoint.Y+_applyButton.ActualHeight>host.ActualHeight)throw new InvalidOperationException("미리보기 후 적용 버튼이 화면 밖에 있습니다 · "+operation);
   if (_preview == null || _preview.CanApply || _preview.Live || _applyButton.IsEnabled || string.IsNullOrWhiteSpace(_preview.Before) || string.IsNullOrWhiteSpace(_preview.After)) throw new InvalidOperationException("오프라인 검토 / 적용 제한 검증 실패 · " + operation);
   _confirmed.IsChecked = true; UpdateButtons(); if (_applyButton.IsEnabled) throw new InvalidOperationException("오프라인 적용 버튼이 활성화되었습니다.");
   previews.Add(new { operation, _preview.Summary, _preview.Before, _preview.After, can_apply = _preview.CanApply });
   await CaptureAsync(Path.Combine(outputDir, "person-" + operation + "-preview.png"));
   if(!_action.StartsWith("delete_",StringComparison.Ordinal)){string token = _preview.Token; await Execute(() => PickTargetCoreAsync(qaCancel: true)); if (_preview?.Token != token) throw new InvalidOperationException("대상 선택 취소가 기존 검토를 바꾸었습니다.");}
   _confirmed.IsChecked = false;
  }
  void SelectSection(string area) { ResetDraft(); _section = area; BuildContent(); }
  try
  {
   foreach (string area in new[] { "career", "relations", "identity" })
   {
    SelectSection(area); await CaptureAsync(Path.Combine(outputDir, "person-" + area + ".png")); captures.Add(area);
    if (_actions.Any(action => action.Button.IsEnabled && action.Enabled?.Invoke() == false)) throw new InvalidOperationException("편집 불가능한 관계 작업이 활성화되었습니다.");
   }
   var clubs = await Search("Club"); var career = _data.Career.Rows.FirstOrDefault(row => row.EditableClub);
   if(_data.Career.Rows.FirstOrDefault(row=>row.EditableDetails&&row.Target!=null) is {} typedCareer)
   {
    SelectSection("career");BeginCareerTarget(typedCareer);_condition.SelectedValue=typedCareer.Condition==1?2:1;
    await Review("typed-career-condition-only");
    if(typedCareer.ObjectType==3&&clubs.FirstOrDefault(club=>club.Id!=typedCareer.Target!.Uid) is {} typedClub){
     await Execute(()=>PickTargetCoreAsync(typedClub,Path.Combine(outputDir,"picker-typed-career.png")));await Review("typed-career-target-change");
    }
   }
   if (career != null)
   {
    var club = clubs.FirstOrDefault(item => item.Id != career.Target?.Uid) ?? clubs.FirstOrDefault() ?? throw new InvalidOperationException("커리어 QA의 구단 검색 자료가 필요합니다.");
    SelectSection("career"); BeginCareer(career); if (_condition.SelectedIndex < 0) _condition.SelectedIndex = 0; if (_job.SelectedIndex < 0) _job.SelectedIndex = 0;
    await Execute(() => PickTargetCoreAsync(club, Path.Combine(outputDir, "picker-career.png"))); await Review("career");
    _condition.SelectedIndex = _condition.SelectedIndex == 0 && _condition.Items.Count > 1 ? 1 : 0;
    if (_preview != null) throw new InvalidOperationException("커리어 조건 변경이 검토를 무효화하지 못했습니다.");
   }
   if(_data.Career.CanAdd)
   {
    SelectSection("career");BeginAddCareer();
    var club=clubs.FirstOrDefault()??throw new InvalidOperationException("커리어 추가 QA에 구단이 필요합니다.");
    await Execute(()=>PickTargetCoreAsync(club,Path.Combine(outputDir,"picker-add-career.png")));await Review("add-career");
   }
   if(_data.Career.Rows.FirstOrDefault(row=>row.CanDelete) is {} deletedCareer)
   {
    SelectSection("career");BeginDelete("delete_career_plan",deletedCareer.Slot,"커리어 계획",$"{deletedCareer.ConditionLabel} · {deletedCareer.TargetName} · {deletedCareer.JobLabel}");
    if(!HasPendingChanges)throw new InvalidOperationException("커리어 삭제가 변경 상태로 표시되지 않습니다.");await Review("delete-career");
   }
   var favourite = _data.Rows.FirstOrDefault(row => row.EditableFavourite && row.Target != null);
   if (favourite != null)
   {
    SelectSection("relations"); BeginSocial(favourite); _level.Text = (favourite.Level == 100 ? 99 : favourite.Level + 1).ToString(CultureInfo.InvariantCulture);
    if(_target?.Id!=favourite.Target!.Uid)throw new InvalidOperationException("기존 관계 수치 편집에서 대상이 보존되지 않았습니다.");
    await Review("existing-social-level-only"); _level.Text = favourite.Level.ToString(CultureInfo.InvariantCulture);
    if (_preview != null || _applyButton.IsEnabled) throw new InvalidOperationException("선호도 변경이 검토를 무효화하지 못했습니다.");
    if (_data.CanAddFavourite && clubs.FirstOrDefault(item => !_data.Rows.Any(row => row.EditableFavourite && row.Target?.Uid == item.Id)) is { } addedClub)
    {
     SelectSection("relations"); BeginFavourite(favourite, true); await Execute(() => PickTargetCoreAsync(addedClub, Path.Combine(outputDir, "picker-favourite.png"))); await Review("add-favourite");
    }
    if(favourite.CanDelete){SelectSection("relations");BeginDelete("delete_favourite_club",favourite.Slot,"선호 구단",favourite.TargetName);await Review("delete-favourite");}
   }
   else if(_data.CanAddFavourite && clubs.FirstOrDefault() is {} firstFavourite)
   {
    SelectSection("relations");BeginFavourite(null,true);await Execute(()=>PickTargetCoreAsync(firstFavourite,Path.Combine(outputDir,"picker-first-favourite.png")));await Review("first-favourite");
   }
   if(_data.Rows.FirstOrDefault(row=>row.Kind==9&&row.CanDelete) is {} deletedNationality)
   {
    SelectSection("identity");BeginDelete("delete_secondary_nationality",deletedNationality.Slot,"추가 국적",deletedNationality.TargetName);await Review("delete-nationality");
   }
   EntityRecord? donor = null; PersonRelationsData? donorData = null;
   foreach (string kind in new[] { "Staff", "Player", "Person" })
   {
    foreach (var person in (await Search(kind)).Where(item => item.Identity != _person.Identity))
    {
     PersonRelationsData? candidate = null;
     await Execute(async () => { try { candidate = Decode<PersonRelationsData>(await _bridge.CallAsync("relation_detail", new { kind = person.Kind, id = person.Id }, _lifetime.Token)); } catch (BridgeException) { /* A snapshot may deliberately omit other people's relation fixtures. */ } });
     if (candidate?.PrimaryNationality != null && candidate.Names.Any(name => name.Text.Length > 0 && _data.Names.Any(current => current.Field == name.Field && current.Text != name.Text))) { donor = person; donorData = candidate; break; }
    }
    if (donor != null) break;
   }
   if (donor != null && donorData != null)
   {
    var nationality = _data.Rows.FirstOrDefault(row => row.EditableSecondaryNationality && row.Target?.Uid != donorData.PrimaryNationality!.Uid);
    bool canAddNationality = _data.CanAddSecondaryNationality && _data.PrimaryNationality?.Uid != donorData.PrimaryNationality!.Uid && !_data.Rows.Any(row => row.Kind == 9 && row.Target?.Uid == donorData.PrimaryNationality.Uid);
    if (nationality != null || canAddNationality)
    {
     SelectSection("identity"); BeginNationality(nationality); await Execute(() => PickTargetCoreAsync(donor, Path.Combine(outputDir, "picker-nationality.png"))); await Review(nationality == null ? "add-nationality" : "nationality");
    }
    var name = _data.Names.First(current => donorData.Names.Any(value => value.Field == current.Field && value.Text.Length > 0 && value.Text != current.Text));
    SelectSection("identity"); BeginName(name); if (HasPendingChanges) throw new InvalidOperationException("이름 편집을 열기만 해도 변경 상태가 되었습니다.");
    await Execute(() => PickTargetCoreAsync(donor, Path.Combine(outputDir, "picker-name.png"))); await Review("name");
   }
   else if (_data.Names.Count > 0) throw new InvalidOperationException("국적·이름 QA의 기준 인물 관계 스냅샷이 필요합니다.");
   ResetDraft(); if (HasPendingChanges || _applyButton.IsEnabled) throw new InvalidOperationException("작성 취소가 변경 상태를 지우지 못했습니다.");
   return new { success = true, live = false, entity = _person.Identity, relationRows = _data.Rows.Count, careerRows = _data.Career.Rows.Count, captures, previews, pickerCancelPreservesReview = true, draftCancelClears = true, applyDisabled = !_applyButton.IsEnabled, gameWrites = 0 };
  }
  finally { ResetDraft(); _section = section; BuildContent(); }
 }
 private async Task CaptureAsync(string path)
 {
  var window = Window.GetWindow(this) ?? throw new InvalidOperationException("관계 화면을 표시한 뒤 QA를 실행하세요.");
  ScrollViewer? scroll = null;
  for (DependencyObject? parent = VisualTreeHelper.GetParent(this); parent != null; parent = VisualTreeHelper.GetParent(parent)) if (parent is ScrollViewer viewer) { scroll = viewer; break; }
  try
  {
   await Dispatcher.InvokeAsync(window.UpdateLayout, DispatcherPriority.Render);
   if (_review.Visibility == Visibility.Visible) _reviewPanel.BringIntoView(); else scroll?.ScrollToTop();
   await Dispatcher.InvokeAsync(window.UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
   if (window.ActualWidth <= 0 || window.ActualHeight <= 0) throw new InvalidOperationException("관계 화면을 표시한 뒤 QA를 실행하세요.");
   // Capture the actual viewport, rather than a scrolled child whose inherited clip and offset truncate its review.
   var bitmap = new RenderTargetBitmap((int)Math.Ceiling(window.ActualWidth), (int)Math.Ceiling(window.ActualHeight), 96, 96, PixelFormats.Pbgra32); bitmap.Render(window); var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(path); encoder.Save(stream);
  }
  finally { scroll?.ScrollToTop(); await Dispatcher.InvokeAsync(window.UpdateLayout, DispatcherPriority.Render); }
 }

 private sealed class RelationEntityPicker : Window
 {
  private readonly BridgeClient _bridge;
  private readonly ComboBox _kind = new() { MinWidth = 110, DisplayMemberPath = "Label", SelectedValuePath = "Key" };
  private readonly TextBox _query = Input();
  private readonly DataGrid _grid = Table();
  private readonly TextBlock _status = Text("이름 또는 UID로 검색하세요.");
  private readonly Button _search = Button("검색"), _select = Button("선택");
  private Task? _pending;
  private bool _closing;
  public EntityRecord? Result { get; private set; }
  public RelationEntityPicker(BridgeClient bridge, bool clubs)
  {
   _bridge = bridge; Title = clubs ? "대상 구단 선택" : "기준 인물 선택"; Width = 760; Height = 560; MinWidth = 640; MinHeight = 460; WindowStartupLocation = WindowStartupLocation.CenterOwner; ShowInTaskbar = false; Background = Brush("BaseBrush"); Foreground = Brush("InkBrush"); Typography.Initialize(this);
   var root = new DockPanel { Margin = new Thickness(18) }; Content = root; var toolbar = new Grid { Margin = new Thickness(0, 0, 0, 12) }; toolbar.ColumnDefinitions.Add(new() { Width = new GridLength(130) }); toolbar.ColumnDefinitions.Add(new()); toolbar.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
   _kind.ItemsSource = clubs ? new[] { new SearchChoice("Club", "구단") } : new[] { new SearchChoice("Player", "선수"), new SearchChoice("Staff", "스태프"), new SearchChoice("Person", "기타 인물") }; _kind.SelectedIndex = 0; toolbar.Children.Add(_kind); _query.Margin = new Thickness(8, 0, 8, 0); Grid.SetColumn(_query, 1); toolbar.Children.Add(_query); Grid.SetColumn(_search, 2); toolbar.Children.Add(_search); DockPanel.SetDock(toolbar, Dock.Top); root.Children.Add(toolbar);
   var bottom = new DockPanel { Margin = new Thickness(0, 10, 0, 0) }; var buttons = new StackPanel { Orientation = Orientation.Horizontal }; var cancel = Button("취소"); cancel.IsCancel = true; cancel.Click += (_, _) => Close(); buttons.Children.Add(cancel); _select.IsEnabled = false; _select.Click += async (_, _) => await ChooseAsync(); buttons.Children.Add(_select); DockPanel.SetDock(buttons, Dock.Right); bottom.Children.Add(buttons); bottom.Children.Add(_status); DockPanel.SetDock(bottom, Dock.Bottom); root.Children.Add(bottom);
   _grid.MaxHeight = double.PositiveInfinity; Column(_grid, "이름", nameof(EntityRecord.DisplayName), 1, true); Column(_grid, "UID", nameof(EntityRecord.Id), 100); Column(_grid, "구단", nameof(EntityRecord.Club), 160); Column(_grid, "직무 / 포지션", nameof(EntityRecord.Role), 110); EntityRoleSorting.Attach(_grid); root.Children.Add(_grid); _grid.SelectionChanged += (_, _) => _select.IsEnabled = _pending?.IsCompleted != false && _grid.SelectedItem is EntityRecord; _grid.MouseDoubleClick += async (_, _) => await ChooseAsync();
   _kind.SelectionChanged += (_, _) => { _grid.ItemsSource = null; _select.IsEnabled = false; _status.Text = "이름 또는 UID로 검색하세요."; };
   _search.Click += async (_, _) => await SearchAsync(); _query.KeyDown += async (_, args) => { if (args.Key == Key.Enter) { args.Handled = true; await SearchAsync(); } }; Closing += OnClosing;
  }
  private async Task SearchAsync()
  {
   if (_closing || _pending?.IsCompleted == false) return; _grid.ItemsSource = null; _search.IsEnabled = _select.IsEnabled = _kind.IsEnabled = _query.IsEnabled = false; _status.Text = "검색 중…";
   async Task Fetch() { try { var result = await _bridge.CallAsync("search", new { kind = _kind.SelectedValue, query = _query.Text.Trim(), limit = 100, offset = 0, sort = "name" }); if (_closing) return; _grid.ItemsSource = Decode<List<EntityRecord>>(result.GetProperty("items")); _status.Text = $"{result.GetProperty("total").GetInt32():N0}개 중 최대 100개 표시"; } catch (Exception error) { _status.Text = error.Message; } }
   _pending = Fetch(); await _pending; _search.IsEnabled = _kind.IsEnabled = _query.IsEnabled = !_closing; _select.IsEnabled = !_closing && _grid.SelectedItem is EntityRecord;
  }
  private async Task ChooseAsync() { if (_closing || _pending?.IsCompleted == false || _grid.SelectedItem is not EntityRecord entity) return; Result = entity; if (_pending != null) await _pending; DialogResult = true; }
  public async Task QaAsync(EntityRecord entity, string path)
  {
   _kind.SelectedValue = entity.Kind; _query.Text = entity.Id.ToString(CultureInfo.InvariantCulture); await SearchAsync();
   _grid.SelectedItem = _grid.Items.OfType<EntityRecord>().FirstOrDefault(item => item.Identity == entity.Identity) ?? throw new InvalidOperationException("실제 검색 결과에 QA 대상이 없습니다.");
   await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render); await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
   var bitmap = new RenderTargetBitmap((int)Math.Ceiling(ActualWidth), (int)Math.Ceiling(ActualHeight), 96, 96, PixelFormats.Pbgra32); bitmap.Render(this); var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using (var stream = File.Create(path)) encoder.Save(stream);
   await ChooseAsync(); if (Result?.Identity != entity.Identity) throw new InvalidOperationException("대상 선택 결과가 다릅니다.");
  }
  private async void OnClosing(object? sender, CancelEventArgs args) { if (_closing) return; if (_pending?.IsCompleted == false) { args.Cancel = true; _closing = true; await _pending; Close(); } }
 }
}
