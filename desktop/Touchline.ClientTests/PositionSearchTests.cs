using System.Text.Json;
using Touchline.Wpf;

internal static class PositionSearchTests
{
 internal static void Run(Action<bool,string> check)
 {
  var fields=PositionSearch.Codes.Select(code=>new EditableField{Key=PositionSearch.FieldKey(code),Label=code,Min=0,Max=20,Step=1,Section="포지션"}).ToList();
  var catalog=SearchConditionCatalog.Build("Player",fields);
  var position=new SearchConditionRule{Source="position",Key="any",Operator="gte",Value="20",Positions=["position_dl","position_dr"]};
  var criteria=new SearchCriteria{Rules=[position,new(){Source="entity",Key="ca",Operator="gte",Value="100"}]};
  var requests=SearchConditionCatalog.Compile(criteria.Rules,catalog);
  check(requests.Count==2&&requests[0].positions!.SequenceEqual(new[]{"position_dl","position_dr"})&&requests[1].join=="and","position_two_alternatives_remain_one_operand_before_ca_constraint");
  check(Convert.ToInt32(requests[0].value)==20&&requests[0].source=="position"&&requests[0].op=="gte","position_natural_means_numeric_proficiency_20");
  var restored=JsonSerializer.Deserialize<SearchCriteria>(JsonSerializer.Serialize(criteria))!;
  var clone=restored.Clone();clone.Rules[0].Positions.RemoveAt(0);
  check(restored.Rules[0].Positions.Count==2&&criteria.Rules[0].Positions.Count==2&&SearchConditionCatalog.Compile(restored.Rules,catalog)[0].positions!.Count==2,"position_saved_criteria_roundtrip_and_clone_are_independent");
  position.Value="17";check(Convert.ToInt32(SearchConditionCatalog.Compile([position],catalog)[0].value)==17,"position_exact_numeric_proficiency_threshold_supported");
  bool Rejected(SearchConditionRule rule,IReadOnlyList<SearchConditionDefinition>? definitions=null){try{SearchConditionCatalog.Compile([rule],definitions??catalog);return false;}catch(InvalidOperationException){return true;}}
  foreach(string invalid in new[]{"0","21","19.5","natural"}){var copy=position.Clone();copy.Value=invalid;check(Rejected(copy),"position_invalid_threshold_"+invalid);}
  var missing=position.Clone();missing.Positions=["position_unknown"];check(Rejected(missing),"position_unsupported_field_is_rejected");
  missing.Positions=["position_dl","position_dl"];check(Rejected(missing),"position_duplicate_keys_rejected");
  missing.Positions=[];check(Rejected(missing),"position_empty_selection_rejected");
  check(Rejected(position,SearchConditionCatalog.Build("Staff",fields)),"position_player_only_catalog");
  check(Rejected(position,SearchConditionCatalog.Build("Player",fields.Where(f=>f.Key!="position_dr"))),"position_unavailable_catalog_field_rejected");
  position.Enabled=false;position.Value="uncommitted";check(SearchConditionCatalog.Compile([position],catalog).Count==0,"position_disabled_draft_is_preserved_but_not_queried");
  var legacy=JsonSerializer.Deserialize<SearchCriteria>("""{"Rules":[{"Source":"field","Key":"position_dc","Operator":"gte","Value":"15"}]}""")!;
  var oldRequest=SearchConditionCatalog.Compile(legacy.Rules,catalog)[0];check(oldRequest.source=="field"&&!JsonSerializer.Serialize(oldRequest).Contains("positions"),"position_legacy_numeric_condition_contract_preserved");
 }
}
