TOUCHLINE EDITOR {{VERSION}} — Windows x64

1. ZIP 전체를 원하는 새 폴더에 압축 해제하세요.
2. 최상위의 Touchline Editor.exe를 실행하세요.
3. 실행 중인 FM에 연결하거나, 저장해 둔 Touchline 스냅샷을 여세요.

처음 실행하면 짧은 4단계 안내가 표시됩니다. 완료하거나 건너뛴 뒤에도
오른쪽 위 ? → 처음 시작하기에서 다시 볼 수 있습니다.

상단 CA·PA 표시 메뉴에서 현재 능력과 잠재 능력을 각각 숨길 수 있습니다.
설정은 자동 저장되며 원래 게임 수치는 바뀌지 않습니다.

별도 .NET, Chrome, WebView 설치는 필요 없습니다.
Windows 10/11 x64용이며 ARM 네이티브 배포판은 아닙니다.

폴더 안내
  Touchline Editor.exe   실행기
  app/                  화면, 글꼴, .NET/WPF 런타임
  engine/               Rust 엔진과 필요한 VC 런타임
  api/                  선택 사용: Python 로컬 API 클라이언트
  docs/                 사용 안내 및 지원 범위
  licenses/             타사 라이선스와 고지

app/ 내부 DLL은 서로 함께 있어야 합니다. DLL을 임의로 옮기거나 EXE만
복사하지 마세요. 업데이트는 새 폴더에 압축 해제하는 방식으로 해 주세요.

자세한 안내: docs/QUICKSTART.ko.md
검증 방법:   docs/Verify-Package.ps1
무결성 목록: package-manifest.json

이 배포판에는 개인 세이브, 선수 데이터, 초상화 팩, 스냅샷, 변경 기록,
API 인증 토큰이 포함되어 있지 않습니다.
