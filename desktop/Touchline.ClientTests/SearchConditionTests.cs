using System.Text.Json;
using Touchline.Wpf;

internal static class SearchConditionTests
{
 internal static void Run(Action<bool,string> check)
 {
  bool Reject(Action action){try{action();return false;}catch(InvalidOperationException){return true;}}
  var fields=new[]{new EditableField{Key="passing",Label="패스",Section="기술",Min=1,Max=20,Step=1},new EditableField{Key="position_mc",Label="중앙 미드필더",Section="포지션",Min=1,Max=20,Step=1},new EditableField{Key="contract.weekly_wage",Label="주급",Section="계약",Min=0,Max=1000000,Step=100},new EditableField{Key="score_head_youth",Label="유소년 육성가",Section="직무 점수",Min=0,Max=300,Step=1}};
  var catalog=SearchConditionCatalog.Build("Player",fields,[],[new("1군","1군")],[new(776,"대한민국")]);
  var merged=SearchConditionCatalog.MergeFields(fields.Take(2),[fields[0],fields[2],new EditableField{Key="bad",Min=double.NaN,Max=20}]);
  check(merged.Count==3&&ReferenceEquals(merged[0],fields[0])&&merged.Count(f=>f.Key=="passing")==1,"detailed_sample_and_column_catalog_deduplicate_without_overwriting_sample");
  var contract=merged.Single(f=>f.Key=="contract.weekly_wage");check(!contract.Editable&&double.IsNaN(contract.Value)&&contract.Max==1000000&&contract.Step==100,"detailed_contract_metadata_available_without_sample_contract_or_fake_value");
  check(SearchConditionCatalog.Compile([new(){Key="contract.weekly_wage",Operator="lte",Value="10000"}],SearchConditionCatalog.Build("Staff",merged)).Count==1,"detailed_contract_search_compiles_from_verified_column_metadata");
  check(catalog.Any(d=>d.Key=="passing"&&d.Min==1&&d.Max==20)&&catalog.Any(d=>d.Key=="position_mc"&&d.Category=="포지션 숙련도")&&catalog.Any(d=>d.Key=="contract.weekly_wage"&&d.Category=="계약"),"detailed_catalog_actual_numeric_position_contract_fields");
  check(!catalog.Any(d=>d.Key=="unsupported_interest")&&!SearchConditionCatalog.Build("City",[]).Any(d=>d.Key=="nationality_id"),"detailed_catalog_never_fabricates_unsupported_fields");
  var field=catalog.Single(d=>d.Key=="passing");var a=SearchConditionCatalog.NewRule(field);a.Value="15";var b=a.Clone();b.Join="or";b.Operator="ne";b.Value="16";var disabled=a.Clone();disabled.Enabled=false;disabled.Value="not a number";
  var compiled=SearchConditionCatalog.Compile([a,b,disabled],catalog);
  check(compiled.Count==2&&compiled[0].join=="and"&&compiled[1].join=="or"&&compiled[1].op=="ne", "detailed_disabled_draft_omitted_and_order_operator_preserved");
  var leading=disabled.Clone();var active=b.Clone();check(SearchConditionCatalog.Compile([leading,active],catalog).Single().join=="and","detailed_first_active_join_normalized");
  foreach(string op in new[]{"gte","lte","eq","ne","gt","lt","between"}){var r=a.Clone();r.Operator=op;r.Value2="20";check(SearchConditionCatalog.Compile([r],catalog)[0].op==op,"detailed_numeric_operator_"+op);}
  foreach(string text in new[]{"NaN","Infinity","21","0",""}){var r=a.Clone();r.Value=text;check(Reject(()=>SearchConditionCatalog.Compile([r],catalog)),"detailed_reject_numeric_"+text);}
  var reversed=a.Clone();reversed.Operator="between";reversed.Value="20";reversed.Value2="1";check(Reject(()=>SearchConditionCatalog.Compile([reversed],catalog)),"detailed_reject_reversed_range");
  var unknown=a.Clone();unknown.Key="unknown";check(Reject(()=>SearchConditionCatalog.Compile([unknown],catalog)),"detailed_unknown_enabled_field_rejected");unknown.Enabled=false;check(SearchConditionCatalog.Compile([unknown],catalog).Count==0,"detailed_unavailable_disabled_saved_field_can_be_kept");
  var badOp=a.Clone();badOp.Operator="contains";check(Reject(()=>SearchConditionCatalog.Compile([badOp],catalog)),"detailed_wrong_operator_type_rejected");
  var nation=SearchConditionCatalog.NewRule(catalog.Single(d=>d.Key=="nationality_id"));var employed=SearchConditionCatalog.NewRule(catalog.Single(d=>d.Key=="employed"));var textRule=SearchConditionCatalog.NewRule(catalog.Single(d=>d.Key=="name"));textRule.Value="Kim";
  using var wire=JsonDocument.Parse(JsonSerializer.Serialize(SearchConditionCatalog.Compile([nation,employed,textRule,a],catalog)));var json=wire.RootElement;
  check(json[0].GetProperty("value").GetUInt32()==776&&json[1].GetProperty("value").ValueKind==JsonValueKind.True&&json[2].GetProperty("value").GetString()=="Kim"&&json[3].GetProperty("value").GetDouble()==15,"detailed_wire_numbers_reference_boolean_text_are_typed");
  nation.Value="999";check(Reject(()=>SearchConditionCatalog.Compile([nation],catalog)),"detailed_unavailable_reference_not_silently_all");
  var age=SearchConditionCatalog.NewRule(catalog.Single(d=>d.Key=="age"));age.Value="18.5";check(Reject(()=>SearchConditionCatalog.Compile([age],catalog)),"detailed_metadata_integer_rejects_fraction");
  check(Reject(()=>SearchConditionCatalog.Compile(Enumerable.Repeat(a,65),catalog)),"detailed_maximum_64_rules");
  var model=new SearchCriteria{Rules=[a,b,disabled],Conditions=[new("passing",10)]};var clone=model.Clone();clone.Rules[0].Value="19";clone.Rules.Reverse();check(model.Rules[0].Value=="15"&&model.Rules[2].Enabled==false,"detailed_clone_deep_and_preserves_order_enabled");
  var restored=JsonSerializer.Deserialize<SearchCriteria>(JsonSerializer.Serialize(model),JsonDefaults.Options)!;check(restored.Rules[1].Join=="or"&&restored.Rules[1].Operator=="ne"&&restored.Rules[2].Value=="not a number"&&restored.Conditions[0].Min==10,"detailed_saved_search_roundtrip_all_rule_state");
  check(JsonSerializer.Deserialize<SearchCriteria>("{\"Conditions\":[{\"Key\":\"passing\",\"Min\":15}]}",JsonDefaults.Options)!.Rules.Count==0,"detailed_old_saved_filters_default_to_empty_expression");
  var promoted=SearchConditionCatalog.PromoteLegacy([new("passing",15),new("passing",null,15),new("passing",15,15),new("passing",12,18)]);check(promoted.Select(r=>r.Operator).SequenceEqual(new[]{"gte","lte","eq","between"}),"detailed_legacy_promotion_preserves_comparison");
 }
}
