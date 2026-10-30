using System.Globalization;
using System.Text.Json;
using Touchline.Wpf;

internal static class AbilityVisibilityTests
{
 public static void Run(Action<bool, string> check)
 {
  var current = AbilityVisibility.Current;
  bool originalHideCa = current.HideCa, originalHidePa = current.HidePa;
  var culture = CultureInfo.CurrentCulture;
  string directory = Path.Combine(Path.GetTempPath(), "Touchline-AbilityVisibility-" + Guid.NewGuid().ToString("N"));
  string path = Path.Combine(directory, "visibility.json");
  try
  {
   var policy = new AbilityVisibility();
   check(!policy.HideCa && !policy.HidePa && !policy.AnyHidden, "ability_visibility_defaults_show_both_values");
   string[] caAliases = ["ca", "player.ca", "staff.ca", "current_ca", "current_ability"];
   string[] paAliases = ["pa", "player.pa", "staff.pa", "potential_ca", "potential_ability"];
   check(caAliases.All(key => AbilityVisibility.AbilityKey(" " + key.ToUpperInvariant() + " ") == "ca") && paAliases.All(key => AbilityVisibility.AbilityKey(" " + key.ToUpperInvariant() + " ") == "pa"), "ability_visibility_aliases_ignore_case_and_surrounding_whitespace");
   string?[] unrelatedKeys = [null, "", " ", "passing", "capacity", "player.passing", "ca_extra", "club.pa"];
   check(unrelatedKeys.All(key => AbilityVisibility.AbilityKey(key) == null), "ability_visibility_keys_match_exact_aliases_without_substrings");

   var people = new[]
   {
    new EntityRecord { Id = 1, Kind = "Player", Ca = 123, Pa = 178 },
    new EntityRecord { Id = 2, Kind = "Staff", Ca = 145, Pa = 190 }
   };
   string[] originalJson = people.Select(person => JsonSerializer.Serialize(person, JsonDefaults.Options)).ToArray();
   foreach (bool hideCa in new[] { false, true })
   foreach (bool hidePa in new[] { false, true })
   {
    string combination = $"ca_{hideCa.ToString().ToLowerInvariant()}_pa_{hidePa.ToString().ToLowerInvariant()}";
    policy.Set(hideCa, hidePa);
    check(policy.HideCa == hideCa && policy.HidePa == hidePa && policy.AnyHidden == (hideCa || hidePa) && caAliases.All(key => policy.IsHidden(key) == hideCa) && paAliases.All(key => policy.IsHidden(key) == hidePa), "ability_visibility_independent_flags_" + combination);
    check(policy.Format(" CA ", 123) == (hideCa ? "숨김" : "123") && policy.Format("STAFF.PA", 178) == (hidePa ? "숨김" : "178") && unrelatedKeys.All(key => !policy.IsHidden(key) && policy.Format(key, 17) == "17"), "ability_visibility_formats_only_selected_ability_" + combination);
    check(policy.Format("ca", null) == (hideCa ? "숨김" : "—") && policy.Format("pa", "") == (hidePa ? "숨김" : "—") && policy.Format("passing", null) == "—", "ability_visibility_missing_values_are_not_zero_" + combination);

    current.Set(hideCa, hidePa);
    check(people.All(person => person.AbilityText == $"{(hideCa ? "숨김" : person.Ca!.Value.ToString(CultureInfo.InvariantCulture))} / {(hidePa ? "숨김" : person.Pa!.Value.ToString(CultureInfo.InvariantCulture))}"), "ability_visibility_entity_display_obeys_current_policy_" + combination);
    check(people.Select(person => JsonSerializer.Serialize(person, JsonDefaults.Options)).SequenceEqual(originalJson) && people[0].Ca == 123 && people[0].Pa == 178 && people[1].Ca == 145 && people[1].Pa == 190, "ability_visibility_never_mutates_raw_values_or_serialization_" + combination);

    policy.Save(path);
    var reopened = new AbilityVisibility();
    reopened.Set(!hideCa, !hidePa);
    reopened.Load(path);
    check(reopened.HideCa == hideCa && reopened.HidePa == hidePa && Directory.GetFiles(directory, "*.tmp").Length == 0, "ability_visibility_preferences_roundtrip_" + combination);
   }

   policy.Set(false, false);
   CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo("fr-FR");
   check(policy.Format("ca", 123.5) == "123.5", "ability_visibility_visible_numeric_format_is_culture_independent");
   current.Set(false, false);
   check(new EntityRecord { Ca = 123 }.AbilityText == "123 / —" && new EntityRecord { Pa = 178 }.AbilityText == "— / 178" && new EntityRecord().AbilityText == "—", "ability_visibility_visible_entity_missing_values_keep_existing_display");

   var observed = new AbilityVisibility();
   int notifications = 0;
   bool eventCarriesCurrentState = true;
   bool expectedCa = false, expectedPa = false;
   observed.Changed += (sender, _) =>
   {
    notifications++;
    eventCarriesCurrentState &= ReferenceEquals(sender, observed) && observed.HideCa == expectedCa && observed.HidePa == expectedPa;
   };
   observed.Set(false, false);
   check(notifications == 0, "ability_visibility_unchanged_defaults_do_not_notify");
   expectedCa = true;
   observed.Set(true, false);
   observed.Set(true, false);
   check(notifications == 1 && eventCarriesCurrentState, "ability_visibility_notifies_once_after_ca_changes");
   expectedPa = true;
   observed.Set(true, true);
   observed.Set(true, true);
   check(notifications == 2 && eventCarriesCurrentState, "ability_visibility_notifies_once_after_pa_changes");
   observed.Save(path);
   observed.Load(path);
   check(notifications == 2, "ability_visibility_loading_identical_preferences_does_not_notify");
   File.Delete(path);
   observed.Load(path);
   check(observed.HideCa && observed.HidePa && notifications == 2 && !File.Exists(path), "ability_visibility_missing_preferences_preserve_current_settings_without_writing");
   foreach (var invalid in new[]
   {
    (Name: "corrupt", Json: "{incomplete"),
    (Name: "wrong_version", Json: """{"Version":999,"HideCa":false,"HidePa":false}"""),
    (Name: "missing_version", Json: """{"HideCa":false,"HidePa":false}"""),
    (Name: "wrong_type", Json: """{"Version":1,"HideCa":"false","HidePa":false}"""),
    (Name: "null", Json: "null")
   })
   {
    File.WriteAllText(path, invalid.Json);
    observed.Load(path);
    check(observed.HideCa && observed.HidePa && notifications == 2 && File.ReadAllText(path) == invalid.Json, "ability_visibility_" + invalid.Name + "_preferences_preserve_current_settings");
   }
   observed.Save(path);
   var recovered = new AbilityVisibility();
   recovered.Load(path);
   check(recovered.HideCa && recovered.HidePa && Directory.GetFiles(directory, "*.tmp").Length == 0, "ability_visibility_save_replaces_invalid_preferences_without_temporary_files");
   policy.Set(false, false);
   policy.Save(path);
   expectedCa = expectedPa = false;
   observed.Load(path);
   check(!observed.AnyHidden && notifications == 3 && eventCarriesCurrentState, "ability_visibility_load_notifies_once_for_changed_settings");
  }
  finally
  {
   current.Set(originalHideCa, originalHidePa);
   CultureInfo.CurrentCulture = culture;
   if (File.Exists(path)) File.Delete(path);
   if (Directory.Exists(directory)) Directory.Delete(directory);
  }
 }
}
