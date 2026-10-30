using Touchline.Wpf;

internal static class DetailPresentationTests
{
 internal static void Run(Action<bool,string> check)
 {
  string[] fm27Scores = ["score_technical_attack", "score_technical_defense", "score_technical_mental", "score_tactical_attack", "score_tactical_defense", "score_tactical_mental", "score_fitness", "score_gk_shot", "score_set_pieces", "score_assistant", "score_general"];
  var training = fm27Scores.Where(DetailPresentation.IsTrainingScore).ToList();
  check(training.Count == 9 && training.Contains("score_gk_shot") && !training.Contains("score_assistant"), "fm27_training_cards_do_not_fill_missing_tenth_score_with_management");
  check(DetailPresentation.IsTrainingScore("score_gk_distribution") && !DetailPresentation.IsTrainingScore("score_youth_goalkeeper"), "fm26_training_distribution_retained_when_supplied_only");
  var person = new DetailRecord { Entity = new EntityRecord { Kind = "Person", Name = "기타 인물" } };
  check(DetailPresentation.BasicPersonOnly(person) && DetailPresentation.BasicPersonMessages(person).Single().Contains("기본 정보"), "basic_person_without_fields_has_explicit_support_note");
  person.Notes.Add("FM27 세부 구조 미검증");
  check(DetailPresentation.BasicPersonMessages(person).SequenceEqual(person.Notes), "basic_person_preserves_backend_explanation");
  person.Fields.Add(new EditableField { Key = "person.loyalty" });
  check(!DetailPresentation.BasicPersonOnly(person), "verified_person_fields_keep_existing_details");
  var catalog = SearchColumns.Available("Staff", [new EditableField { Key = "staff_gk_shot", Label = "골키퍼 훈련" }, new EditableField { Key = "score_gk_shot", Label = "골키퍼 코치" }]);
  string[] saved = ["staff_gk_shot", "staff_gk_handling", "staff_gk_distribution", "score_gk_shot", "score_gk_distribution", "score_youth_goalkeeper"];
  check(SearchColumns.Normalize(saved, catalog).SequenceEqual(new[] { "name", "staff_gk_shot", "score_gk_shot" }) && catalog.Single(c=>c.Key=="staff_gk_shot").Label == "골키퍼 훈련", "fm27_saved_coaching_columns_drop_unsupported_fields_and_keep_native_label");
 }
}
