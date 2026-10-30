using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;

namespace Touchline.Wpf;

public sealed partial class SearchView
{
 /// <summary>Exercises real WPF display, tooltip and copy bindings against local synthetic rows.</summary>
 public object QaAbilityVisibility()
 {
  bool originalCa = AbilityVisibility.Current.HideCa, originalPa = AbilityVisibility.Current.HidePa;
  string[] aliases = ["ca", "pa", "player.ca", "player.pa", "staff.ca", "staff.pa"];
  var fields = aliases.Where(key => key.Contains('.')).Select(key => new EditableField { Key = key, Label = key, Section = "기본 정보", Min = 0, Max = 200, Step = 1 }).ToList();
  var qa = new SearchView(_bridge, false, null, _ => Task.CompletedTask);
  qa._kind = "Player"; qa._initialized = true; qa._availableColumns = SearchColumns.Available("Player", fields); qa._columnCatalog.AddRange(fields);
  qa._columnLayout = new() { View = "custom", Keys = ["name", ..aliases] }; qa.BuildViewOptions();
  var row = new SearchRow { Entity = new() { Id = 1, Kind = "Player", Name = "Zulu", Ca = 123, Pa = 178 }, Values = aliases.Where(key => key.Contains('.')).ToDictionary(key => key, key => key.EndsWith(".ca") ? 123d : 178d) };
  qa._rows.Add(row);
  qa._rows.Add(new SearchRow { Entity = new() { Id = 2, Kind = "Player", Name = "Alpha", Ca = 180, Pa = 190 } });
  string raw = JsonSerializer.Serialize(row.Entity) + JsonSerializer.Serialize(row.Values);
  string[] keys = qa._columnLayout.Keys.ToArray();
  int checkedBindings = 0;
  static string ReadBinding(BindingBase binding, object item)
  {
   var text = new TextBlock { DataContext = item };
   BindingOperations.SetBinding(text, TextBlock.TextProperty, binding); text.GetBindingExpression(TextBlock.TextProperty)!.UpdateTarget();
   return text.Text;
  }
  try
  {
   foreach (var state in new[] { (Ca: false, Pa: false), (Ca: true, Pa: false), (Ca: false, Pa: true), (Ca: true, Pa: true), (Ca: false, Pa: false) })
   {
    AbilityVisibility.Current.Set(state.Ca, state.Pa); qa.ApplyAbilityVisibility();
    if (!keys.SequenceEqual(qa._columnLayout.Keys)) throw new InvalidOperationException("숨김 설정이 저장된 검색 열을 변경했습니다.");
    foreach (string key in aliases)
    {
     bool hidden = AbilityVisibility.Current.IsHidden(key);
     var column = (DataGridTextColumn)qa._grid.Columns.Single(column => column.SortMemberPath == key);
     string expected = hidden ? "숨김" : key.EndsWith("ca") ? "123" : "178";
     var tooltip = (BindingBase)column.ElementStyle.Setters.OfType<Setter>().Single(setter => setter.Property == ToolTipService.ToolTipProperty).Value;
     foreach (var binding in new[] { column.Binding, column.ClipboardContentBinding, tooltip })
     {
      if (ReadBinding(binding, row) != expected) throw new InvalidOperationException("검색 셀 / 툴팁 / 복사 숨김 실패: " + key);
      if (hidden && ReadBinding(binding, new SearchRow()) != "숨김") throw new InvalidOperationException("누락된 검색 값의 숨김 표시 실패: " + key);
      checkedBindings++;
     }
     if (column.CanUserSort == hidden) throw new InvalidOperationException("숨긴 검색 열 정렬이 활성화되어 있습니다: " + key);
     if (hidden && qa._sort.Items.Cast<SearchChoice>().Any(choice => choice.Key == key)) throw new InvalidOperationException("숨긴 정렬 선택지가 표시됩니다: " + key);
     if (hidden) { qa._sortKey = key; qa.ApplySort(); if (qa._sortKey != "name" || qa._resultView.Cast<SearchRow>().First().Id != 2) throw new InvalidOperationException("숨긴 검색 정렬이 유지되었습니다."); }
     var saved = new SearchCriteria { Rules = [new() { Source = key.Contains('.') ? "field" : "entity", Key = key, Value = "123" }] };
     if (HasHiddenAbilityCriteria(saved) != hidden) throw new InvalidOperationException("저장된 상세 능력 조건 숨김 검사 실패: " + key);
     if (hidden)
     {
      qa._criteria = saved;
      if (qa.CriteriaLabels().Any(label => label.Contains("123", StringComparison.Ordinal))) throw new InvalidOperationException("검색 조건 요약에 숨긴 값이 표시됩니다.");
      bool blocked = false; try { qa.ValidateCriteria(saved); } catch (InvalidOperationException error) when (error.Message == HiddenAbilityCriteriaMessage) { blocked = true; }
      if (!blocked) throw new InvalidOperationException("숨긴 능력 조건 검색이 차단되지 않았습니다.");
      blocked = false; try { _ = new SearchCriteriaWindow("Player", fields, [], [], null, saved); } catch (InvalidOperationException) { blocked = true; }
      if (!blocked) throw new InvalidOperationException("저장된 숨긴 조건을 편집기에서 노출했습니다.");
      qa._criteria = new();
     }
    }
    var editor = new SearchCriteriaWindow("Player", fields, [], [], null, new());
    editor.QaAssertAbilityVisibility();
    if (raw != JsonSerializer.Serialize(row.Entity) + JsonSerializer.Serialize(row.Values)) throw new InvalidOperationException("숨김이 원래 검색 데이터를 변경했습니다.");
   }
   return new { success = true, localOnly = true, gameWrites = 0, checkedBindings, aliases = aliases.Length, independentToggles = true, savedColumnsPreserved = true, hiddenSortBlocked = true, hiddenCriteriaBlocked = true, rawValuesUnchanged = true };
  }
  finally { AbilityVisibility.Current.Set(originalCa, originalPa); }
 }
}
