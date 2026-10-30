namespace Touchline.Wpf;

internal static class DetailPresentation
{
 internal static bool IsTrainingScore(string key) => key is
  "score_technical_attack" or "score_technical_defense" or "score_technical_mental" or
  "score_tactical_attack" or "score_tactical_defense" or "score_tactical_mental" or
  "score_fitness" or "score_gk_shot" or "score_gk_distribution" or "score_set_pieces";

 internal static bool BasicPersonOnly(DetailRecord detail) => detail.Entity.Kind == "Person" && detail.Fields.Count == 0;
 internal static IEnumerable<string> BasicPersonMessages(DetailRecord detail) => detail.Notes.Count > 0
  ? detail.Notes
  : ["현재 게임 버전에서 이 인물의 기본 정보만 확인할 수 있습니다. 세부 정보와 편집은 아직 지원되지 않습니다."];
}
