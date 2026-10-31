using System.Windows;

namespace Touchline.Wpf;

public sealed partial class SearchView
{
 private async Task OpenShortlistsAsync()
 {
  var selected = _grid.SelectedItems.Cast<SearchRow>().Select(row => row.Entity).ToArray();
  // A cancelled/partial cursor must not be presented as all search results.
  var all = _complete && _failed == 0 ? _resultView.Cast<SearchRow>().Select(row => row.Entity).ToArray() : [];
  var dialog = new ShortlistWindow(_bridge, selected, all) { Owner = Window.GetWindow(this) };
  int epoch = _epoch; _activeDialog = dialog;
  try
  {
   dialog.ShowDialog();
   if (epoch == _epoch && dialog.ProfileToOpen is { } entity) await _open(entity);
  }
  finally { if (_activeDialog == dialog) _activeDialog = null; }
 }
}
