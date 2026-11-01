using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private readonly List<Action> _abilityDisplayRefresh = [];
 private string? _listSort, _listRequestedSort;
 private void InitializeAbilityVisibility()
 {
  AbilityVisibility.Current.Changed += AbilityDisplayChanged;
  Closed += (_, _) => AbilityVisibility.Current.Changed -= AbilityDisplayChanged;
  RefreshAbilityPreferences();
 }
 private void RefreshAbilityPreferences()
 {
  var preference = AbilityVisibility.Current;
  HideCaOption.IsChecked = preference.HideCa;
  HidePaOption.IsChecked = preference.HidePa;
  AbilityDisplayButton.Content = preference.AnyHidden ? "CA·PA 숨김 설정" : "CA·PA 표시";
  foreach (var item in SortBox.Items.OfType<ComboBoxItem>()) item.IsEnabled = !preference.IsHidden(item.Tag?.ToString());
  if (SortBox.SelectedItem is ComboBoxItem selected && !selected.IsEnabled) SortBox.SelectedIndex = 0;
 }
 private async void AbilityDisplayChanged(object? sender, EventArgs args)
 {
  if (!Dispatcher.CheckAccess()) { Dispatcher.Invoke(() => AbilityDisplayChanged(sender, args)); return; }
  var preference = AbilityVisibility.Current;
  bool refreshList = preference.IsHidden(_listSort) || preference.IsHidden(_listRequestedSort)
   || preference.IsHidden((SortBox.SelectedItem as ComboBoxItem)?.Tag?.ToString());
  RefreshAbilityPreferences();
  if (refreshList)
  {
   // The old page and any pending response may reveal ability ranking even after masking its cells.
   ++_searchVersion; _searchTimer.Stop(); _offset = 0; _total = 0;
   _listSort = _listRequestedSort = null;
   EntityGrid.ItemsSource = null;
   PrevPage.IsEnabled = NextPage.IsEnabled = false;
   PageLabel.Text = "1 / 1"; ResultCount.Text = "불러오는 중…";
  }
  else EntityGrid.Items.Refresh();
  foreach (var refresh in _abilityDisplayRefresh.ToArray()) refresh();
  RefreshAbilityReviewAvailability();
  if (refreshList) await SearchAsync();
 }
 private void AbilityDisplay_Click(object sender, RoutedEventArgs args)
 {
  var button = (Button)sender;
  button.ContextMenu.PlacementTarget = button;
  button.ContextMenu.Placement = PlacementMode.Bottom;
  button.ContextMenu.IsOpen = true;
 }
 private void AbilityOption_Click(object sender, RoutedEventArgs args)
 {
  AbilityVisibility.Current.Set(HideCaOption.IsChecked, HidePaOption.IsChecked);
  try
  {
   if (_qaOutput == null) AbilityVisibility.Current.Save(AbilityVisibility.DefaultPath);
   Status("표시 설정 저장됨 · 숨긴 능력은 값·정렬·검색 조건에 표시하지 않습니다.");
  }
  catch (Exception error) { App.Log(error); Status("현재 화면에는 반영했지만 표시 설정을 저장하지 못했습니다: " + error.Message); }
 }
 private void TrackAbilityDisplay(Action refresh) { _abilityDisplayRefresh.Add(refresh); refresh(); }
 private void RefreshAbilityReviewAvailability()
 {
  if (_detail == null) return;
  var dirty = _detail.Fields.Where(field => field.IsDirty).ToList();
  bool hiddenDraft = dirty.Any(field => AbilityVisibility.Current.IsHidden(field.Key));
  ReviewButton.IsEnabled = dirty.Count > 0 && _detail.Fields.All(field => field.IsValid) && DraftDateError == null && !hiddenDraft;
  if (hiddenDraft) DirtyLabel.Text = "숨긴 CA·PA에 변경 초안이 있습니다. 표시를 켜서 확인한 뒤 적용하세요.";
  else if (dirty.Count > 0) DirtyLabel.Text = DraftDateError ?? (_detail.Fields.All(field => field.IsValid) ? $"{dirty.Count}개 항목 변경됨  ·  아직 게임에 적용되지 않았습니다." : "입력값의 허용 범위를 확인하세요.");
 }
 private bool ConfirmAbilityExport() => !AbilityVisibility.Current.AnyHidden || AppDialog.Show(this,
  "숨기기는 화면 표시 설정입니다. 내보내는 원본 스냅샷에는 CA·PA 숫자가 포함됩니다. 계속할까요?",
  "원본 데이터 내보내기", MessageBoxButton.YesNo, MessageBoxImage.Information) == MessageBoxResult.Yes;
}
