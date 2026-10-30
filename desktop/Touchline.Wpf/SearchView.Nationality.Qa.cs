using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;

namespace Touchline.Wpf;

public sealed partial class SearchView
{
 /// <summary>Exercises the native picker and real snapshot cursor. No game reads or writes.</summary>
 public async Task<object> QaNationalityAsync(string directory)
 {
  if (_live) throw new InvalidOperationException("국적 검색 QA는 스냅샷으로만 실행합니다.");
  Directory.CreateDirectory(directory); var checks = new List<object>();
  foreach (string kind in new[] { "Player", "Staff" })
  {
   await SetKindAsync(kind);
   var selectedNation = _nations.FirstOrDefault(nation => nation.Id.HasValue && nation.Count > 0) ?? throw new InvalidOperationException(kind + " 국적 검색 QA에 주 국적이 있는 샘플이 필요합니다.");
   var dialog = CriteriaWindow(new()); Exception? failure = null; object? layout = null;
   dialog.Loaded += async (_, _) =>
   {
    try
    {
     var selector = dialog.FindName("CriteriaNationality") as ComboBox ?? throw new InvalidOperationException("주 국적 선택기가 없습니다.");
     selector.ApplyTemplate();
     var input = selector.Template.FindName("PART_EditableTextBox", selector) as TextBox ?? throw new InvalidOperationException("국적 선택기의 실제 입력창이 없습니다.");
     await Dispatcher.InvokeAsync(dialog.UpdateLayout, DispatcherPriority.Render);
     if (!input.IsVisible || input.Text != SearchNationalityChoices.AllLabel) throw new InvalidOperationException("기본 국적 선택기의 ‘모든 국적’ 표시가 비어 있습니다.");
     await dialog.QaNationalityLayoutAsync(Path.Combine(directory, kind.ToLowerInvariant(), "all"));
     input.Text = selectedNation.Name;
     if (dialog.Snapshot().NationalityId != selectedNation.Id) throw new InvalidOperationException("실제 국가명 입력이 조건에 연결되지 않습니다.");
     selector.SelectedItem = selector.Items.Cast<SearchNationChoice>().Single(nation => nation.Id == selectedNation.Id);
     selector.Text = selectedNation.Name;
     if (!selector.IsEditable || dialog.Snapshot().NationalityId != selectedNation.Id) throw new InvalidOperationException("국적 선택기가 검색 조건을 만들지 못했습니다.");
     input.Text = "__invalid_country__"; bool rejected = false; try { dialog.Snapshot(); } catch (InvalidOperationException) { rejected = true; }
     if (!rejected) throw new InvalidOperationException("입력한 국가를 찾지 못했는데 기존 선택으로 처리했습니다.");
     selector.SelectedItem = selector.Items.Cast<SearchNationChoice>().Single(nation => nation.Id == selectedNation.Id); selector.Text = selectedNation.Name;
     layout = await dialog.QaNationalityLayoutAsync(Path.Combine(directory, kind.ToLowerInvariant()));
     await Dispatcher.InvokeAsync(dialog.UpdateLayout, DispatcherPriority.Render);
     var nationBounds = selector.TransformToAncestor(dialog).TransformBounds(new Rect(selector.RenderSize));
     if (!selector.IsVisible || nationBounds.Top < 0 || nationBounds.Bottom > dialog.ActualHeight - 50) throw new InvalidOperationException("주 국적 선택기가 화면에서 잘립니다.");
     (dialog.FindName("CriteriaApplyButton") as Button)!.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent));
    }
    catch (Exception error) { failure = error; dialog.Close(); }
   };
   if (dialog.ShowDialog() != true || dialog.Result == null) throw failure ?? new InvalidOperationException("주 국적 선택기를 적용하지 못했습니다.");
   await ApplyCriteriaAsync(dialog.Result);
   if (!_complete || _failed != 0 || _rows.Count != selectedNation.Count || _rows.Any(row => row.Entity.NationalityId != selectedNation.Id)) throw new InvalidOperationException(kind + " 주 국적 필터 결과 불일치.");
   if (!CriteriaLabels().Any(label => label.Contains("주 국적") && label.Contains(selectedNation.Name))) throw new InvalidOperationException("검색 요약에 국적이 없습니다.");
   uint[] matched = _rows.Select(row => row.Id).Order().ToArray();
   string proof = JsonSerializer.Serialize(_criteria, JsonDefaults.Options);
   var cancel = CriteriaWindow(_criteria); cancel.Loaded += (_, _) =>
   {
    var selector = (ComboBox)cancel.FindName("CriteriaNationality"); selector.SelectedIndex = 0; selector.Text = SearchNationalityChoices.AllLabel;
    (cancel.FindName("CriteriaCancelButton") as Button)!.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Button.ClickEvent));
   };
   cancel.ShowDialog();
   if (JsonSerializer.Serialize(_criteria, JsonDefaults.Options) != proof) throw new InvalidOperationException("국적 검색 취소가 적용된 조건을 바꿨습니다.");
   var saved = new SavedSearch { Name = "국적 QA", Kind = kind, Criteria = _criteria.Clone() };
   string path = Path.Combine(directory, "nationality-" + kind.ToLowerInvariant() + "-preset.json"); await File.WriteAllTextAsync(path, JsonSerializer.Serialize(saved, JsonDefaults.Options));
   using var savedDocument = JsonDocument.Parse(await File.ReadAllTextAsync(path));
   var restored = Read<SavedSearch>(savedDocument.RootElement); ValidateCriteria(restored.Criteria);
   var restoreState = new SearchReloadState(kind, restored.Criteria, _sortKey, _descending); await ResetAsync();
   if (_criteria.NationalityId != null || _criteria.NationalityName != "") throw new InvalidOperationException("모두 지우기가 국적 조건을 지우지 못했습니다.");
   await RestoreReloadStateAsync(restoreState);
   if (!_rows.Select(row => row.Id).Order().SequenceEqual(matched) || _criteria.NationalityId != selectedNation.Id) throw new InvalidOperationException("저장한 국적 검색 복원 실패.");
   var combined = _criteria.Clone(); combined.Name = matched[0].ToString(); await ApplyCriteriaAsync(combined);
   if (_rows.Count != 1 || _rows[0].Id != matched[0] || _rows[0].Entity.NationalityId != selectedNation.Id) throw new InvalidOperationException("국적과 이름/UID 조건 결합 실패.");
   await ApplyCriteriaAsync(restored.Criteria); await CaptureAsync(Path.Combine(directory, "nationality-" + kind.ToLowerInvariant() + "-results.png"));
   checks.Add(new { kind, nation = selectedNation.Name, nationality_id = selectedNation.Id, ids = matched, nativePicker = true, invalidInputRejected = true, cancelPreservesCriteria = true, presetAndReloadRoundTrip = true, combinedUid = true, reset = true, summaryChip = true, layout });
  }
  return new { success = true, live = false, gameWrites = 0, checks };
 }
}
