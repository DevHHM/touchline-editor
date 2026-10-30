using System.Globalization;
using Touchline.Wpf;

internal static class PositionFilterTests
{
 internal static void Run(Action<bool, string> check)
 {
  string[] expected = ["GK", "SW", "DR", "DC", "DL", "WBR", "WBL", "DM", "MR", "MC", "ML", "AMR", "AMC", "AML", "ST"];
  var alphabetic = expected.Order(StringComparer.Ordinal).Select(key => new SearchChoice(key, key + " (123)")).ToList();
  var original = alphabetic.ToArray();
  var ordered = SearchRoleChoices.Build("Player", alphabetic, "MC");
  check(ordered[0].Key == "" && ordered.Skip(1).Select(choice => choice.Key).SequenceEqual(expected), "position_filter_uses_football_order_instead_of_server_alphabetic_order");
  check(alphabetic.SequenceEqual(original) && ordered.Skip(1).All(choice => ReferenceEquals(choice, original.Single(old => old.Key == choice.Key))), "position_filter_preserves_keys_labels_counts_and_input");
  var duplicate = SearchRoleChoices.Build("Player", [new("ST", "First ST"), new("", "전체 포지션"), new("GK", "Goalkeeper"), new("ST", "Second ST"), new("", "Duplicate all")]);
  check(duplicate.Select(choice => choice.Key).SequenceEqual(new[] { "", "GK", "ST" }) && duplicate[0].Label == "전체 포지션" && duplicate[^1].Label == "First ST", "position_filter_all_first_and_duplicate_keys_keep_original_label");
  var mixed = SearchRoleChoices.Build("Player", [new("Unmapped", "Unmapped"), new("ST / DC", "Compound"), new("DMC", "Malformed"), new("GK", "GK"), new("AMCfoo", "Malformed"), new("dc · dm", "Compound lower")]);
  check(mixed.Skip(1).Select(choice => choice.Key).SequenceEqual(new[] { "GK", "dc · dm", "ST / DC", "AMCfoo", "DMC", "Unmapped" }), "position_filter_compounds_use_exact_tokens_and_unknowns_are_last");
  var missingKnown = SearchRoleChoices.Build("Player", [new("ST", "ST"), new("GK", "GK")], "DR");
  check(missingKnown.Select(choice => choice.Key).SequenceEqual(new[] { "", "GK", "DR", "ST" }), "position_filter_retains_saved_known_selection_at_football_position");
  var missingUnknown = SearchRoleChoices.Build("Player", [new("GK", "GK"), new("ST", "ST")], "Retired role");
  check(missingUnknown[^1].Key == "Retired role" && SearchRoleChoices.Build("Player", [], "DM").Select(choice => choice.Key).SequenceEqual(new[] { "", "DM" }), "position_filter_preserves_missing_unknown_and_empty_facets");
  var culture = CultureInfo.CurrentCulture;
  try
  {
   CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo("ko-KR");
   var jobs = new[] { new SearchChoice("GK", "GK"), new SearchChoice("AMC", "AMC"), new SearchChoice("코치", "코치"), new SearchChoice("감독", "감독") };
   var staff = SearchRoleChoices.Build("Staff", jobs);
   check(staff.Skip(1).Select(choice => choice.Key).SequenceEqual(jobs.Select(choice => choice.Key).Order(StringComparer.CurrentCultureIgnoreCase)), "staff_role_filter_remains_lexical_even_for_position_like_job_names");
  }
  finally { CultureInfo.CurrentCulture = culture; }
 }
}
