namespace Touchline.Wpf;

public sealed record ConnectionPresentation(string Title, string Label, string Guidance)
{
 public static bool IsProblem(string state) => state is "error" or "unsupported_build" or "engine_error";

 public static ConnectionPresentation For(string state, string reason)
 {
  if (state == "engine_error") return new("에디터 엔진을 시작하지 못했습니다", "엔진 실행 확인 필요", "app·engine 폴더를 포함해 ZIP 전체를 압축 해제했는지 확인하세요. 파일을 복구한 뒤 아래의 다시 연결을 누르면 에디터를 종료하지 않고 재시도할 수 있습니다.");
  if (state == "unsupported_build") return new("FM 빌드 지원 확인이 필요합니다", "지원 빌드 확인 필요", "현재 FM 빌드의 데이터 구조가 검증되지 않아 연결을 중단했습니다. 에디터의 최신 버전을 확인하세요. 계속 발생하면 진단 내용을 복사해 전달해 주세요.");
  if (SharedDiagnosticReport.IsAccessDenied(reason))
   return new("FM 접근 권한을 확인해 주세요", "FM 접근 권한 확인 필요", "FM과 에디터를 둘 다 일반 권한으로 실행해 보세요. FM을 관리자 권한으로 실행해야 하는 경우에만 에디터도 같은 권한으로 다시 실행하세요.");
  return new("FM 연결을 완료하지 못했습니다", "연결 오류 · 원인 확인 필요", "FM에서 세이브를 열고 로딩이 끝났는지 확인해 주세요. 아래 원인을 확인한 뒤 다시 연결할 수 있습니다. 반복되면 진단 내용을 복사해 전달해 주세요. 검증되지 않은 빌드는 연결하지 않습니다.");
 }

 public static string Report(string version, string state, string reason)
 {
  // Share a useful error, without raw memory addresses, local account paths or tokens.
  string safe = DiagnosticsLog.Sanitize(reason);
  return $"Touchline Editor {version}\n연결 상태: {state}\n원인: {safe}\n\nFM 버전과 세이브 로딩 완료 여부를 함께 알려주세요.";
 }
}
