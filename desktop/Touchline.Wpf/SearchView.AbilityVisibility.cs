using System.Windows;

namespace Touchline.Wpf;

public sealed partial class SearchView
{
 private const string HiddenAbilityCriteriaMessage = "숨긴 CA / PA를 사용하는 검색 조건입니다. 해당 능력을 표시한 뒤 조건을 편집하거나, 모두 지우기로 새 검색을 시작하세요.";

 internal static bool HasHiddenAbilityCriteria(SearchCriteria criteria, bool includeDisabled = false)
 {
  var visibility = AbilityVisibility.Current;
  return visibility.HideCa && (!string.IsNullOrWhiteSpace(criteria.CaMin) || !string.IsNullOrWhiteSpace(criteria.CaMax))
   || visibility.HidePa && (!string.IsNullOrWhiteSpace(criteria.PaMin) || !string.IsNullOrWhiteSpace(criteria.PaMax))
   || criteria.Conditions.Any(condition => visibility.IsHidden(condition.Key))
   || criteria.Rules.Any(rule => (includeDisabled || rule.Enabled) && visibility.IsHidden(rule.Key));
 }

 private void AbilityVisibilityChanged(object? sender, EventArgs args)
 {
  if (!Dispatcher.CheckAccess()) { Dispatcher.Invoke(ApplyAbilityVisibility); return; }
  ApplyAbilityVisibility();
 }

 /// <summary>Refreshes presentation only; cached records and saved column choices stay intact.</summary>
 public void ApplyAbilityVisibility()
 {
  if (!_initialized) return;
  BuildSortOptions(preserveSelection: true); BuildColumns(); RefreshSummary();
  if (HasHiddenAbilityCriteria(_criteria))
  {
   // Preserve an open criteria editor and its draft. Only an in-flight query needs a new epoch.
   if (_searching) ++_epoch;
   _resultsInvalidated = true; _cancellation?.Cancel(); _queryId = null;
   _rows.Clear(); _loadedFields.Clear(); _errors.Clear(); _scanned = _total = _matched = _failed = 0; _complete = false;
   _grid.SelectedItem = null; _openButton.IsEnabled = false; _progress.Visibility = _errorPanel.Visibility = Visibility.Collapsed;
   _emptyPanel.Visibility = Visibility.Visible;
   _empty.Text = "숨긴 능력치를 사용하는 검색 결과를 지웠습니다.";
   _count.Text = "검색 조건 확인 필요"; _status.Text = HiddenAbilityCriteriaMessage;
   UpdateButtons();
  }
 }

 private void EnsureVisibleSort()
 {
  if (!AbilityVisibility.Current.IsHidden(_sortKey)) return;
  _sortKey = "name"; _descending = false;
  bool suppressed = _suppress; _suppress = true; _sort.SelectedValue = _sortKey; _suppress = suppressed;
  UpdateDirection();
 }
}
