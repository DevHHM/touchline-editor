namespace Touchline.Wpf;

/// <summary>Guidance for reviewed live scalar edits, not file-based localization.</summary>
public static class EditApplicationGuidance
{
 public const string BeforeApply = "원본 백업과 현재 값 검증 후 적용됩니다. 일반 수치 수정에는 FM 재시작이 필요 없습니다. 적용 뒤 FM에서 저장하세요.";
 public const string Applied = "적용 완료 · 일반 수치는 FM 재시작 없이 반영됩니다. FM 화면을 다시 열어 확인하고 게임에서 저장하세요.";
 public const string Detail = "이 안내는 라이브 수치 편집 기준입니다. 이름 한글화·초상화 파일 설치는 해당 기능의 재로딩 안내를 따르세요.";
}
