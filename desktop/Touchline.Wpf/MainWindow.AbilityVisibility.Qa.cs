using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private async Task RunAbilityVisibilityQaAsync(string directory)
 {
  if (_live) throw new InvalidOperationException("Ability display QA requires an offline snapshot.");
  var visibility = AbilityVisibility.Current;
  var original = (visibility.HideCa, visibility.HidePa);
  var reports = new List<object>();
  var initial = _detail?.Entity ?? throw new InvalidOperationException("Load a player fixture first.");
  try
  {
   visibility.Set(false, false);
   foreach (string kind in new[] { "Player", "Staff" })
   {
    var result = await _bridge!.CallAsync("search", new { kind, limit = 1, sort = "name" });
    uint id = result.GetProperty("items")[0].GetProperty("id").GetUInt32();
    await OpenEntity(kind, id); _tab = "전체 능력치"; await RenderDetail();
    var d = _detail!;
    var ca = d.Fields.First(field => AbilityVisibility.AbilityKey(field.Key) == "ca");
    var pa = d.Fields.First(field => AbilityVisibility.AbilityKey(field.Key) == "pa");
    double caOriginal = ca.Value, paOriginal = pa.Value;
    // Snapshot fixtures intentionally omit write bindings. Enable only this local
    // view model to exercise edit preservation; no apply command is issued.
    ca.Editable = true;
    ca.Adjust(ca.Value < ca.Max ? 1 : -1);
    string draft = ca.EditText;
    foreach (var flags in new[] { (false, false), (true, false), (false, true), (true, true), (false, false) })
    {
     visibility.Set(flags.Item1, flags.Item2);
     await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
     foreach (var field in new[] { ca, pa })
     {
      var editors = AbilityDescendants(DetailHost).OfType<TextBox>().Where(edit => ReferenceEquals(edit.DataContext, field)).ToList();
      bool hidden = visibility.IsHidden(field.Key);
      if (editors.Count == 0 || editors.Any(edit => edit.IsVisible == hidden)) throw new InvalidOperationException($"{kind} {field.Key}: incorrect visible field state.");
     }
     if (ca.EditText != draft || pa.Value != paOriginal || !ca.IsDirty) throw new InvalidOperationException($"{kind} hide={flags}: display toggle changed draft (dirty={ca.IsDirty}).");
     if (ReviewButton.IsEnabled == flags.Item1) throw new InvalidOperationException("Hidden ability draft review guard failed.");
     var hero = DetailHost.Children[0];
     var bars = AbilityDescendants(hero).OfType<ProgressBar>().ToList();
     if (bars.Count != 2 || bars[0].IsVisible == flags.Item1 || bars[1].IsVisible == flags.Item2) throw new InvalidOperationException("Hidden hero progress bar leaked.");
     if ((flags.Item1 && bars[0].Value != 0) || (flags.Item2 && bars[1].Value != 0)) throw new InvalidOperationException("Hidden hero progress retained value.");
     if (flags is (true, true))
     {
      var texts = AbilityDescendants(hero).OfType<TextBlock>().Select(text => text.Text).ToList();
      if (texts.Count(text => text == "숨김") != 2) throw new InvalidOperationException("Hero ability labels not masked.");
     }
     reports.Add(new { kind, hide_ca = flags.Item1, hide_pa = flags.Item2, draft_preserved = true, field_masked = true, hero_masked = true });
    }
    ca.Reset();
    if (ca.Value != caOriginal || ca.IsDirty) throw new InvalidOperationException("Draft reset lost original.");
   }
   visibility.Set(false, true);
   await OpenEntity(initial.Kind, initial.Id); _tab = "개요"; await RenderDetail();
   Capture(Path.Combine(directory, "ability-pa-hidden.png"));
   visibility.Set(true, true);
   Capture(Path.Combine(directory, "ability-both-hidden.png"));
   AbilityDisplay_Click(AbilityDisplayButton, new RoutedEventArgs());
   await Dispatcher.Yield(DispatcherPriority.Render);
   if (!HideCaOption.IsChecked || !HidePaOption.IsChecked) throw new InvalidOperationException("Display menu not synchronized.");
   AbilityDisplayButton.ContextMenu.IsOpen = false;
   visibility.Set(false, false);
   await ShowSearchAsync("Player");
   var search = _searchView!.QaAbilityVisibility();
   File.WriteAllText(Path.Combine(directory, "ability-search.json"), JsonSerializer.Serialize(search, new JsonSerializerOptions { WriteIndented = true }));
   var workbench = new WorkbenchWindow(_bridge!, false, initial, _ownClub, _ => { }) { Owner = this };
   workbench.Show();
   try
   {
    var compared = await workbench.QaAbilityVisibilityAsync(directory);
    File.WriteAllText(Path.Combine(directory, "ability-workbench.json"), JsonSerializer.Serialize(compared, new JsonSerializerOptions { WriteIndented = true }));
   }
   finally { await workbench.CloseAsync(); }
   if (_ownClub is { } club)
   {
    await OpenEntity("Club", club.Id); _tab = "선수단"; await RenderDetail();
    var roster = await _rosterView!.QaAbilityVisibilityAsync(directory);
    File.WriteAllText(Path.Combine(directory, "ability-roster.json"), JsonSerializer.Serialize(roster, new JsonSerializerOptions { WriteIndented = true }));
    _tab = "스태프"; await RenderDetail();
    var staff = await _staffReviewView!.QaAbilityVisibilityAsync(directory);
    File.WriteAllText(Path.Combine(directory, "ability-staff.json"), JsonSerializer.Serialize(staff, new JsonSerializerOptions { WriteIndented = true }));
   }
   var history = new ToolsWindow(_bridge!, "history", null, null, false) { Owner = this };
   history.Show();
   try
   {
    var journal = await history.QaAbilityVisibilityAsync();
    File.WriteAllText(Path.Combine(directory, "ability-history.json"), JsonSerializer.Serialize(journal, new JsonSerializerOptions { WriteIndented = true }));
   }
   finally { history.Close(); }
   File.WriteAllText(Path.Combine(directory, "ability-detail.json"), JsonSerializer.Serialize(new { success = true, game_writes = false, scenarios = reports }, new JsonSerializerOptions { WriteIndented = true }));
   _qaChecks.Add("ability_visibility_independent_player_staff_hero_fields_draft_and_search");
   _qaChecks.Add("ability_visibility_workbench_comparison_roster_staff_history");
  }
  finally { visibility.Set(original.HideCa, original.HidePa); }
 }
 private static IEnumerable<DependencyObject> AbilityDescendants(DependencyObject parent)
 {
  for (int i = 0; i < VisualTreeHelper.GetChildrenCount(parent); ++i)
  {
   var child = VisualTreeHelper.GetChild(parent, i); yield return child;
   foreach (var item in AbilityDescendants(child)) yield return item;
  }
 }
}
