using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private async Task RunContractDatesQaAsync(string directory)
 {
  if (_live) throw new InvalidOperationException("계약 날짜 UI QA는 스냅샷에서만 실행합니다.");
  var original = _detail; string originalTab = _tab; var reports = new List<object>();
  var localFields = new List<EditableField>();
  void LocalFieldChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs args) { FieldChanged(sender, args); _scoreTimer.Stop(); }
  try
  {
   foreach (string kind in new[] { "Player", "Staff" })
   {
    var dob = new EditableField { Key = "person.dob", Label = "생년월일", Section = "신상", Min = 19000101, Max = 21001231, Step = 1, Editable = true, Value = 20000229 };
    var expires = new EditableField { Key = "contract.expires", Label = "계약 만료일", Section = "계약", Min = 19000101, Max = 21001231, Step = 1, Editable = true, Value = 20280630 };
    // Local UI fixture only: no bridge edit/preview/apply call is made.
    _detail = new DetailRecord { Entity = new() { Kind = kind, Name = "날짜 UI 검증 · " + kind }, Fields = [dob, expires] };
    foreach (var field in _detail.Fields) { field.PropertyChanged += LocalFieldChanged; localFields.Add(field); }
    _tab = "계약·신상"; await RenderDetail(); await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded);
    var editors = DateDescendants(DetailHost).OfType<DateFieldEditor>().ToArray();
    var expiryEditor = editors.Single(editor => ReferenceEquals(editor.DataContext, expires));
    if (!expiryEditor.Picker.IsEnabled || expiryEditor.Picker.SelectedDate != new DateTime(2028, 6, 30)) throw new InvalidOperationException(kind + " 계약 날짜 달력 표시 실패.");
    var dateInput = expiryEditor.Picker.Template.FindName("PART_TextBox", expiryEditor.Picker) as DatePickerTextBox ?? throw new InvalidOperationException("날짜 입력란을 찾지 못했습니다.");
    dateInput.Text = "invalid date";
    if (expires.IsValid || ReviewButton.IsEnabled) throw new InvalidOperationException("잘못 입력한 날짜가 포커스 이동 전에 검토를 차단하지 않았습니다.");
    dateInput.Text = "2028-06-30";
    if (!expires.IsValid || expires.IsDirty || expires.Value != 20280630 || expiryEditor.Picker.SelectedDate != new DateTime(2028, 6, 30)) throw new InvalidOperationException("같은 원본 날짜로 수정해도 입력 오류가 남았습니다.");
    dateInput.Text = "2031-07-01";
    if (!expires.IsValid || !expires.IsDirty || expires.Value != 20310701 || !ReviewButton.IsEnabled) throw new InvalidOperationException($"입력한 날짜가 포커스 이동 전에 검토 초안에 반영되지 않았습니다: {kind}, text={dateInput.Text}, draft={expires.EditText}, value={expires.Value}, valid={expires.IsValid}, dirty={expires.IsDirty}, review={ReviewButton.IsEnabled}, selected={expiryEditor.Picker.SelectedDate}, fields={string.Join(";", _detail.Fields.Select(f => $"{f.Key}:{f.EditText}:{f.IsValid}"))}, busy={_busy}.");
    expires.Reset();
    if (expires.IsDirty || !DateFieldPresentation.TryParseInput(dateInput.Text, System.Globalization.CultureInfo.CurrentCulture, out var resetDate) || resetDate != new DateTime(2028, 6, 30)) throw new InvalidOperationException("입력 날짜 초기화가 같은 선택 날짜의 텍스트를 복원하지 못했습니다.");
    if (DateDescendants(DetailHost).OfType<TextBlock>().Count(text => text.Text == "확인할 수 없음") != 1 || _detail.Fields.Any(field => field.Key == "contract.starts")) throw new InvalidOperationException("미확인 계약 시작일을 생성했습니다.");
    expiryEditor.Picker.SelectedDate = new DateTime(2030, 6, 30);
    if (!expires.IsDirty || expires.Value != 20300630 || !ReviewButton.IsEnabled) throw new InvalidOperationException(kind + " 달력 변경이 미리보기 초안으로 연결되지 않았습니다.");
    UpdateLayout(); Capture(Path.Combine(directory, "contract-date-" + kind.ToLowerInvariant() + ".png"));
    expiryEditor.Picker.SelectedDate = null;
    if (expires.IsValid || !expires.IsDirty || ReviewButton.IsEnabled) throw new InvalidOperationException("날짜를 비운 상태에서 미리보기가 활성화되었습니다.");
    expires.Reset();
    if (expires.IsDirty || expires.Value != 20280630 || expiryEditor.Picker.SelectedDate != new DateTime(2028, 6, 30)) throw new InvalidOperationException("날짜 초안 초기화가 원본 날짜를 복원하지 못했습니다.");
    expires.Editable = false;
    if (expiryEditor.Picker.IsEnabled) throw new InvalidOperationException("읽기 전용 날짜 편집기가 활성화되었습니다.");
    expires.Editable = true;
    var starts = new EditableField { Key = "contract.starts", Label = "계약 시작일", Section = "계약", Min = 19000101, Max = 21001231, Step = 1, Editable = true, Value = 20240701, Hint = "후속 계약 조건이 있으면 시작일은 읽기 전용입니다." };
    starts.PropertyChanged += LocalFieldChanged; localFields.Add(starts); _detail.Fields.Add(starts);
    await RenderDetail(); await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded);
    editors = DateDescendants(DetailHost).OfType<DateFieldEditor>().ToArray();
    var startEditor = editors.Single(editor => ReferenceEquals(editor.DataContext, starts)); expiryEditor = editors.Single(editor => ReferenceEquals(editor.DataContext, expires));
    startEditor.Picker.SelectedDate = new DateTime(2025, 7, 1);
    if (!starts.IsDirty || starts.Value != 20250701 || !ReviewButton.IsEnabled) throw new InvalidOperationException("제공된 계약 시작일의 달력 수정이 초안에 반영되지 않았습니다.");
    expiryEditor.Picker.SelectedDate = new DateTime(2024, 6, 30);
    if (ReviewButton.IsEnabled || DateFieldPresentation.PeriodError(_detail.Fields) == null) throw new InvalidOperationException("시작일보다 이른 만료일이 검토를 통과했습니다.");
    expires.Reset(); starts.Reset(); starts.Editable = false;
    if (startEditor.Picker.IsEnabled || !ToolTipService.GetShowOnDisabled(startEditor.Picker) || startEditor.Picker.ToolTip?.ToString() != starts.Hint) throw new InvalidOperationException("읽기 전용 계약 시작일의 사유를 확인할 수 없습니다.");
    UpdateLayout(); Capture(Path.Combine(directory, "contract-period-" + kind.ToLowerInvariant() + ".png"));
    _detail.Fields.Remove(starts); _detail.Fields.Remove(expires); _detail.Notes = ["현재 자료에서는 계약 날짜 조회·편집을 지원하지 않습니다."];
    await RenderDetail(); await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Loaded);
    if (DateDescendants(DetailHost).OfType<DateFieldEditor>().Any(editor => editor.DataContext is EditableField field && DateFieldPresentation.IsContractDate(field.Key)) || DateDescendants(DetailHost).OfType<TextBlock>().Count(text => text.Text == "확인할 수 없음") != 2) throw new InvalidOperationException("지원되지 않는 계약 날짜가 달력에 표시되었습니다.");
    Capture(Path.Combine(directory, "contract-unavailable-" + kind.ToLowerInvariant() + ".png"));
    reports.Add(new { kind, calendarEditsDraft = true, typedInvalidRecoversToSameDate = true, typedDraftValidatesBeforeFocusLoss = true, typedDraftResetRestoresText = true, clearBlocksReview = true, resetRestoresDate = true, readOnlyDisabled = true, suppliedStartEditable = true, guardedStartReadOnlyWithReason = true, reversedPeriodBlocksReview = true, missingDatesExplicit = true });
   }
   File.WriteAllText(Path.Combine(directory, "contract-dates.json"), JsonSerializer.Serialize(new { success = true, syntheticUiFixtures = true, gameWrites = false, reports }, new JsonSerializerOptions { WriteIndented = true }));
   _qaChecks.Add("player_staff_contract_date_calendar_draft_review_guard_and_unavailable_states");
  }
  finally
  {
   foreach (var field in localFields) field.PropertyChanged -= LocalFieldChanged;
   _detail = original; _tab = originalTab;
   if (_detail != null) { await RenderDetail(); FieldChanged(null, new(nameof(EditableField.IsDirty))); }
  }
 }
 private static IEnumerable<DependencyObject> DateDescendants(DependencyObject parent)
 {
  for (int i = 0; i < VisualTreeHelper.GetChildrenCount(parent); ++i)
  {
   var child = VisualTreeHelper.GetChild(parent, i); yield return child;
   foreach (var descendant in DateDescendants(child)) yield return descendant;
  }
 }
}
