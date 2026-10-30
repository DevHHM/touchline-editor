using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Media;

namespace Touchline.Wpf;

public sealed partial class SearchView
{
 private readonly SearchColumnSettings _columnSettings;
 private readonly Button _columnsButton;
 private SearchColumnLayout _columnLayout = new();
 private List<SearchColumnSpec> _availableColumns = [];
 private bool _buildingColumns;
 private static string SearchColumnsPath() => App.Arg("--ui-check") is string qa
  ? Path.Combine(Path.GetDirectoryName(Path.GetFullPath(qa))!, "qa-column-settings.json")
  : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "search-columns.json");
 private static bool IsMetadata(string key) => key is "name" or "english_name" or "nationality" or "id" or "age" or "club" or "role" or "group" or "ca" or "pa" or "football_category";
 private void BuildViewOptions()
 {
  _suppress = true;
  var choices = new List<SearchChoice> { new("default", "기본 정보"), new("selected", "검색 조건의 능력치"), new("custom", "사용자 지정") };
  if (_kind == "Player") choices.AddRange([new("technical", "기술 능력"), new("mental", "정신 능력"), new("physical", "신체 능력"), new("goalkeeping", "골키퍼 능력"), new("positions", "포지션 숙련도"), new("contract", "계약 · 이적"), new("personality", "성격 · 히든")]);
  if (_kind == "Staff") choices.AddRange([new("coaching", "코칭 능력"), new("mental", "정신 능력"), new("medical", "의료 능력"), new("scores", "직무 점수"), new("contract", "계약 · 이적"), new("personality", "성격 · 히든")]);
  _view.ItemsSource = choices;
  _view.SelectedValue = choices.Any(c => c.Key == _columnLayout.View) ? _columnLayout.View : "default";
  _suppress = false;
 }
 private IEnumerable<string> PresetFields()
 {
  string view = Selected(_view, "default");
  string? section = view switch { "technical" => "기술", "mental" when _kind == "Player" => "정신", "physical" => "신체", "goalkeeping" => "골키퍼", "positions" => "포지션", "personality" => "성격", _ => null };
  if (section != null) return _columnCatalog.Where(f => f.Section.Contains(section) || view == "personality" && f.Section.Contains("히든") || view == "physical" && f.Key == "height").Select(f => f.Key);
  return view switch {
   "coaching" => ["staff_attack", "staff_defense", "staff_fitness", "staff_mental", "staff_technical", "staff_tactical", "staff_gk_shot", "staff_gk_handling", "staff_gk_distribution", "staff_youth", "staff_set_pieces"],
   "mental" => ["adaptability", "staff_determination", "staff_discipline", "staff_motivation", "staff_knowledge", "staff_management", "staff_judging_ability", "staff_judging_potential", "staff_judging_staff", "staff_negotiation", "staff_analysis"],
   "medical" => ["staff_physiotherapy", "staff_sports_science"],
   "scores" => _columnCatalog.Where(f => f.Key.StartsWith("score_")).Select(f => f.Key),
   "contract" => _columnCatalog.Where(f => f.Key.StartsWith("contract.") || f.Key is "player.market_value" or "player.asking_price").Select(f => f.Key),
   "selected" => _criteria.Conditions.Select(c => c.Key),
   _ => _criteria.Conditions.Select(c => c.Key).Take(4)
  };
 }
 private List<string> ColumnKeys()
 {
  var keys = Selected(_view, "default") == "custom"
   ? _columnLayout.Keys.Count > 0 ? _columnLayout.Keys.AsEnumerable() : SearchColumns.Default(_kind)
   : SearchColumns.Default(_kind).Concat(PresetFields());
  return SearchColumns.Normalize(keys.Concat(NumericSort ? [_sortKey] : []), _availableColumns);
 }
 private IEnumerable<string> ViewFields() => ColumnKeys().Where(key => !IsMetadata(key)).Take(SearchColumns.MaxNumericColumns);
 private void RememberColumnWidths()
 {
  if (_grid == null || _buildingColumns) return;
  foreach (var column in _grid.Columns)
   if (double.IsFinite(column.ActualWidth) && column.ActualWidth >= 48 && column.ActualWidth <= 800) _columnLayout.Widths[column.SortMemberPath] = column.ActualWidth;
 }
 private void SaveColumnPreferences()
 {
  if (!_initialized || _buildingColumns || string.IsNullOrWhiteSpace(_kind)) return;
  RememberColumnWidths(); _columnLayout.View = Selected(_view, "default");
  _columnSettings.Save(_kind, _columnLayout);
 }
 private void SaveReorderedColumns()
 {
  if (_buildingColumns || !_initialized || _loading || _searching) return;
  _columnLayout.Keys = SearchColumns.Normalize(_grid.Columns.OrderBy(c => c.DisplayIndex).Select(c => c.SortMemberPath), _availableColumns);
  _suppress = true; _view.SelectedValue = "custom"; _suppress = false;
  try { SaveColumnPreferences(); } catch (Exception ex) { App.Log(ex); _status.Text = "열 설정 저장 실패: " + ex.Message; }
 }
 private async Task EditColumnsAsync()
 {
  if (!_initialized || _loading || _searching) return;
  var dialog = new SearchColumnWindow(_kind, _availableColumns, ColumnKeys()) { Owner = Window.GetWindow(this) };
  int epoch = _epoch; _activeDialog = dialog;
  try { if (dialog.ShowDialog() == true && dialog.Result != null && epoch == _epoch) await ApplyColumnKeysAsync(dialog.Result); }
  finally { if (_activeDialog == dialog) _activeDialog = null; }
 }
 private async Task ApplyColumnKeysAsync(IEnumerable<string> keys)
 {
  _columnLayout.Keys = SearchColumns.Normalize(keys, _availableColumns);
  _suppress = true; _view.SelectedValue = "custom"; _suppress = false;
  SaveColumnPreferences(); await RefreshColumnsAsync();
 }
 private void BuildColumns()
 {
  RememberColumnWidths(); _buildingColumns = true;
  try {
   _grid.Columns.Clear();
   foreach (string key in ColumnKeys())
   {
    var spec = _availableColumns.Single(c => c.Key == key);
    double width = _columnLayout.Widths.GetValueOrDefault(key, spec.Width);
    if (key == "name")
    {
     var names = new FrameworkElementFactory(typeof(StackPanel));
     var display = new FrameworkElementFactory(typeof(TextBlock)); display.SetBinding(TextBlock.TextProperty, new Binding(nameof(SearchRow.Name))); display.SetValue(TextBlock.FontWeightProperty, FontWeights.Bold); display.SetValue(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis); names.AppendChild(display);
     var english = new FrameworkElementFactory(typeof(TextBlock)); english.SetBinding(TextBlock.TextProperty, new Binding(nameof(SearchRow.EnglishName))); english.SetValue(TextBlock.ForegroundProperty, Brush("MutedBrush")); english.SetValue(TextBlock.FontSizeProperty, 11d); english.SetValue(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis); names.AppendChild(english);
     _grid.Columns.Add(new DataGridTemplateColumn { Header = "이름", CellTemplate = new DataTemplate { VisualTree = names }, Width = width, MinWidth = 175, SortMemberPath = key, CanUserReorder = false });
     continue;
    }
    string path = key switch { "english_name" => nameof(SearchRow.OriginalName), "id" => nameof(SearchRow.Id), "nationality" => nameof(SearchRow.Nationality), "age" => nameof(SearchRow.Age), "club" => nameof(SearchRow.Club), "role" => nameof(SearchRow.Role), "group" => nameof(SearchRow.Group), "ca" => nameof(SearchRow.Ca), "pa" => nameof(SearchRow.Pa), "football_category" => nameof(SearchRow.FootballCategoryLabel), _ => $"Values[{key}]" };
    Column(spec.Label, path, key, width, spec.Numeric || key is "age" or "ca" or "pa" or "id");
    var column = (DataGridTextColumn)_grid.Columns[^1];
    if (spec.Numeric) column.Binding = new Binding(path) { Converter = new SearchValueConverter(), ConverterParameter = key, TargetNullValue = "—", FallbackValue = "—" };
    column.Header = new TextBlock { Text = (key == _sortKey && spec.Numeric ? "★ " : "") + spec.Label, ToolTip = spec.Hint.Length > 0 ? spec.Hint : spec.Label };
   }
  } finally { _buildingColumns = false; }
  ApplySort();
 }
 private sealed class SearchValueConverter : IValueConverter
 {
  public object Convert(object value, Type targetType, object parameter, CultureInfo culture) => SearchColumns.Format(parameter as string ?? "", value);
  public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
 }
}
