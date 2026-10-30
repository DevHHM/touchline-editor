using System.Collections;
using System.ComponentModel;
using System.Windows.Controls;
using System.Windows.Data;

namespace Touchline.Wpf;

/// <summary>Use the same football order in roster, shortlist and relationship tables.</summary>
internal static class EntityRoleSorting
{
 public static void Attach(DataGrid grid) => grid.Sorting += OnSorting;

 private sealed class RoleComparer(bool descending) : IComparer
 {
  public int Compare(object? x, object? y) => x is EntityRecord a && y is EntityRecord b
   ? PositionOrder.Compare(a, b, descending) : 0;
 }

 private static void OnSorting(object sender, DataGridSortingEventArgs args)
 {
  if (sender is not DataGrid grid || grid.ItemsSource == null ||
      CollectionViewSource.GetDefaultView(grid.ItemsSource) is not ListCollectionView view) return;
  string path = args.Column.SortMemberPath;
  if (string.IsNullOrEmpty(path) && args.Column is DataGridBoundColumn { Binding: Binding binding }) path = binding.Path?.Path ?? "";
  if (path != nameof(EntityRecord.Role))
  {
   // WPF's normal sorting must take over when another column is selected.
   if (view.CustomSort is RoleComparer) view.CustomSort = null;
   return;
  }
  args.Handled = true;
  bool descending = args.Column.SortDirection == ListSortDirection.Ascending;
  view.CustomSort = new RoleComparer(descending);
  foreach (var column in grid.Columns) column.SortDirection = null;
  args.Column.SortDirection = descending ? ListSortDirection.Descending : ListSortDirection.Ascending;
 }

 // Exercise the same handler as a real header click in offline UI checks.
 internal static void CheckHeaderSort(DataGrid grid, DataGridColumn column) => OnSorting(grid, new DataGridSortingEventArgs(column));

 internal static object VerifyGrid(DataGrid grid, uint[] ascending, uint[] descending)
 {
  var role = grid.Columns.Single(column => column.SortMemberPath == nameof(EntityRecord.Role) ||
   column is DataGridBoundColumn { Binding: Binding binding } && binding.Path?.Path == nameof(EntityRecord.Role));
  role.SortDirection = null;
  CheckHeaderSort(grid, role);
  uint[] actualAscending = grid.Items.OfType<EntityRecord>().Select(person => person.Id).ToArray();
  if (!actualAscending.SequenceEqual(ascending)) throw new InvalidOperationException("표의 포지션 오름차순이 전술 순서와 다릅니다.");
  CheckHeaderSort(grid, role);
  uint[] actualDescending = grid.Items.OfType<EntityRecord>().Select(person => person.Id).ToArray();
  if (!actualDescending.SequenceEqual(descending)) throw new InvalidOperationException("표의 포지션 내림차순이 전술 순서와 다릅니다.");
  var other = grid.Columns.First(column => column != role);
  CheckHeaderSort(grid, other);
  if (CollectionViewSource.GetDefaultView(grid.ItemsSource) is ListCollectionView { CustomSort: RoleComparer })
   throw new InvalidOperationException("다른 열 선택 후 포지션 정렬이 해제되지 않았습니다.");
  role.SortDirection = null; CheckHeaderSort(grid, role);
  return new { success = true, ascending = actualAscending, descending = actualDescending, otherColumnHandoff = true };
 }
}
