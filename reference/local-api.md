# Touchline local automation API

## 0.11.0 FM27 빌드 지원

`capabilities.build_profiles`에서 인식하는 빌드와 검증 범위를 확인하고, 연결 후에는 `status.build_capabilities`와 실제 `detail.fields[].editable`을 사용합니다. 인식된 FM 버전이라고 모든 기능을 허용하지 않습니다. FM27에서 관계/커리어 추가, 후원 이벤트 및 성장 참고치는 아직 지원하지 않으며 명확한 오류를 반환합니다. FM27 능력치·구단/도시의 검증된 수치에는 기존 preview/apply 경로를 사용합니다. 상세 범위와 근거는 [FM27 포팅 기록](fm27-port.md)에 있습니다.

`health.engine_version`은 실행 중인 엔진 버전입니다. 새 배포본을 복사해도 기존 API 프로세스가 자동 교체되는 것은 아니므로, 업데이트할 때 `stop` 후 새 엔진으로 `start`합니다.

Touchline의 Rust 엔진을 로컬 API로 실행해 검색·상세 조회·변경 검토·적용을 호출합니다. WPF 화면을 누르거나 별도의 메모리 주소를 찾을 필요가 없습니다. 브라우저, JavaScript, 외부 서버, AI API는 사용하지 않습니다. 에디터의 기존 검증·백업·되돌리기 경로를 그대로 사용합니다.

## 실행과 재사용

API 배포 엔진은 `dist/api/Touchline Engine.exe`입니다. 다음 명령은 실행 중인 정상 API가 있으면 그대로 재사용하고, 없을 때만 숨겨진 프로세스로 실행합니다. FM 자체는 실행하거나 조작하지 않습니다.

```powershell
python tools/touchline_api.py start
python tools/touchline_api.py connect
python tools/touchline_api.py status
python tools/touchline_api.py capabilities --output output/api-capabilities.json
```

API만 정상 종료하려면 `python tools/touchline_api.py stop`을 실행합니다. FM이나 WPF 편집기를 종료하지 않습니다. 자동 시작·상태 조회 중 다른 작업이 처리 중이면 두 번째 엔진을 만들지 않고 대기 후 확인하도록 알립니다.

또는 `tools/start-api.ps1 -Python python`를 사용할 수 있습니다. 기본 배포 경로가 다르면 `--engine`으로 API 기능이 포함된 엔진을 지정합니다. `--no-start`는 자동 실행을 막습니다. `--timeout` 기본값은 180초이며 최초 전체 연결에 시간을 줍니다. 한 번 연결한 뒤 검색 색인과 상세 기준값은 서버에 유지됩니다.

연결 정보는 `%LOCALAPPDATA%/TouchlineEditor/api.json`에 있습니다. `base_url`, `token`, `pid`, `protocol_version`이 들어 있습니다. 이 파일의 토큰을 채팅·로그·공유 자료에 붙이지 마세요. 클라이언트는 해당 파일을 직접 읽고, 127.0.0.1 이외 주소나 리디렉션으로 인증 정보를 보내지 않습니다. 프록시도 사용하지 않습니다.

## Python에서 조회

```python
import sys
from pathlib import Path
sys.path.insert(0, str(Path.cwd() / 'tools'))  # run from the repository root
from touchline_api import Client

api = Client()
print(api.request('connect'))
found = api.request('search', {'kind': 'Player', 'query': '김민수', 'limit': 20})
print(found)
player_id = found['data']['items'][0]['id']
detail = api.request('detail', {'kind': 'Player', 'id': player_id})
print(detail)
```

`Player`, `Staff`, `Person`, `Club`, `City`를 검색할 수 있습니다. 정확한 현재 필드 키·범위·편집 여부는 `detail`의 `fields` 또는 `field_catalog`에서 읽습니다. 최신 전체 명령·인수·편집 경로는 `capabilities` 응답을 기준으로 합니다. 검색 결과는 페이지 단위이며 `total`, `offset`, `limit`을 확인합니다.

선수·스태프·기타 인물 검색 행에는 기본 국적 `nationality`(이름)와 `nationality_id`(국가 ID)가 포함됩니다. 검증되지 않았거나 예전 스냅샷에 없는 국적은 빈 문자열과 `null`이며, 소속 구단 국가나 이중국적으로 추정하지 않습니다. 전체 연결 시 기본 국적 링크를 국가별로 한 번씩 읽어 캐시하므로 기본 국적 열 표시 때문에 검색 결과마다 상세 조회를 하지 않습니다. `search`와 `query_start` 모두 `sort:"nationality"`와 `descending`을 지원하며 미확인 국적은 양방향 모두 마지막입니다. 최신 정보를 다시 읽으려면 연결/새로 읽기를 사용합니다.

기본 인덱스 정렬 항목은 `capabilities.indexed_search_sorts`에서 확인할 수 있습니다. `query_start.fields`에는 `field_catalog`가 반환한 숫자 필드 키를 최대 64개 선택할 수 있습니다. 선수의 기술·정신·신체·골키퍼·히든, 양발, 포지션 숙련도, 스태프 능력·역할별 점수, 계약·가치·명성 등 실제 검증된 필드가 해당하며, 모든 대상에 없는 값은 `missing_values`로 표시합니다. 숫자 열은 상세 정보를 단계적으로 읽기 때문에 기본 인덱스 열보다 느릴 수 있습니다. 출전·득점·도움·평점 등 카탈로그에 없는 경기 통계는 지원하는 것으로 취급하지 않습니다.

`field_catalog.fields`와 `sample`은 대표 프로필 한 명의 실제 필드와 값입니다. 열 선택용 `column_fields`는 별도 메타데이터 목록으로, 표본에 계약 열이 없으면 동일 종류의 소속 있는 대상 상세 최대 16개에서 확인된 필드를 보충하고 주급·만료일을 확인하면 중단합니다. 이 목록에는 `key/label/section/min/max/step/hint`만 있고 개인별 값이나 편집 가능 여부가 없으며, 표본의 데이터와 섞어 취급하면 안 됩니다. 이미 선택한 표시 열이 검색 표본에서 빠졌을 때는 `query_start`도 다른 실제 상세 최대 16개에서 해당 필드의 메타데이터를 보충 검증합니다. 지원되지 않는 키를 무조건 허용하지 않습니다. 주급·계약 만료일 같은 표시 열이 개별 대상에 없는 것만으로 검색 행을 제외하지 않으며 빈 셀로 반환합니다. 반면 그 필드에 숫자 검색 조건을 걸면 값이 없는 대상은 조건 불일치입니다.

`search_facets`의 `nations`는 인물 종류별 주 국적 선택지 `[{id, name, count}]`입니다. `search`와 `query_start`의 `filter.nationality_id`에 국가 ID를 넣으면 정확히 그 주 국적을 가진 인물만 검색합니다. 누락·null은 전체이며 0·음수는 거절합니다. 국적을 확인하지 못한 인물은 특정 국적 검색에서 제외합니다. `Club`·`City`에는 주 국적이 없으므로 선택지는 빈 배열이고 특정 국적 조건에 일치하지 않습니다.

`league_catalog {}`는 `leagues`, `total_clubs`, `known_clubs`, `unclassified_clubs`, `live`, `warnings`를 반환합니다. 각 리그에는 `key`, `id`, `name`, `nation_id`, `nation`, `football_category`, `club_count`, `clubs`가 있습니다. `clubs`는 기존 구단 검색 행과 같은 의미의 레코드이며 UID로 구단 상세를 엽니다. 미확인 대회/국가는 `null` ID와 명시적인 그룹으로 보존하고, 같은 이름의 리그도 ID·국가·남녀 구분이 다르면 합치지 않습니다. 현재 1군 소속 대회 탐색이며 순위표는 아닙니다. 예전 스냅샷의 선택 필드 `club_leagues`가 없으면 구단을 누락시키지 않고 미확인으로 분류합니다.

도시는 구단 유무와 관계없이 전체 데이터베이스에서 읽습니다. `search`에 `{"kind":"City","group":"이탈리아"}`를 주면 실제 이탈리아 소속 도시만 반환합니다. 사용 가능한 국가명은 `search_facets`에서 확인합니다. 상세의 `city.nation_id`는 읽기 전용 국가 ID입니다. 도시 매력도는 인접 필드를 보존하는 1바이트 값입니다.

대량 편집은 최대 200개씩 검토·적용합니다. `add`는 상한을 자동 적용하지 않으므로, 예를 들어 매력도 +2·상한20은 현재18이하에 add2, 현재19에 set20, 현재20은 제외해서 처리합니다. 먼저 전체 대상과 예상 변경량을 기록하고 각 배치 결과·실패 여부를 확인하세요.

PowerShell의 따옴표 전달 문제를 피하려면 CLI 인수는 JSON 파일로 넘기는 것이 편합니다.

```powershell
@'
{"kind":"Staff","query":"","limit":20,"sort":"ca","descending":true}
'@ | Set-Content -Encoding utf8 output/api-search.json
python tools/touchline_api.py search --args-file output/api-search.json
```

## 검토한 변경 적용

일반 수치 변경은 `detail → preview → apply` 순서입니다. 첫 단계에서 받은 필드 키만 사용합니다. 미리보기 자체는 게임을 수정하지 않으며, 클라이언트가 자동으로 적용하지 않습니다.

```python
# detail에서 실제 editable=true인 key와 허용 범위를 확인한 뒤 작성합니다.
plan = api.request('preview', {
    'kind': 'Player', 'id': player_id,
    'edits': [{'key': editable_field_key, 'value': desired_value}],
})
print(plan)  # before, after, can_apply를 확인

# 사용자가 요청한 변경 범위와 일치할 때 별도로 실행합니다.
if plan['ok'] and plan['data']['can_apply']:
    receipt = api.request('apply', {'token': plan['data']['token']})
    print(receipt)
```

토큰은 해당 서버에 보관되는 일회용 검토 결과이며 만료됩니다. 재연결·새로 읽기·세이브 변경으로 기준 정보가 무효화되면 다시 조회하고 검토합니다. `apply`가 성공해도 FM 저장 버튼을 누른 것은 아닙니다. 세이브에 남기려면 게임에서 저장합니다.

`connection_poll`의 `session_generation`과 호환용 `generation`은 연결 교체 때만 증가합니다. `data_revision`은 변경 적용·되돌리기·새로 읽기로 상세 캐시와 미리보기가 폐기될 때도 증가합니다. 따라서 `data_revision` 증가를 세이브 변경으로 취급하면 안 됩니다. 정상 편집 후에는 같은 세이브 연결을 유지하며 새 상세 조회와 미리보기를 만들 수 있습니다.

- 여러 대상: `batch_preview → batch_apply`, 기록 `batch_history`.
- 커리어·선호 구단·국적: `relation_detail → relation_preview → relation_apply`, 기록 `relation_history`.
- 후원·구단·도시 이벤트: `event_context → event_preview → event_apply`, 기록 `event_history`.
- 선수명 한글화: `name_localization_context → name_localization_preview → name_localization_apply`.
- 필드별 스카우팅: `query_start → query_step`; 중지 `query_cancel`.
- 자동 재연결 상태: `connection_poll`, 명시적 재연결 `connection_reconnect`.

API의 기능 범위는 에디터가 구현·검증한 기능과 같습니다. 임의 주소·원시 바이트를 쓰는 통로가 아닙니다. API가 있다고 미구현 편집 기능이 생기지는 않습니다.

구단 상세의 `club.sugar_daddy`는 자금 지원 유형입니다. 0=없음, 1=적극 투자형(Front End), 2=후방 지원형, 3=손실 보전형, 4=수익을 기대하는 손실 보전형이며 기존 스칼라 검토·적용 경로를 사용합니다. 구단주 인물 교체·인수 이벤트·뉴스 생성과는 별도이고, 즉시 현금이나 특정 금액의 향후 투자를 보장하지 않습니다. 지원 여부는 연결된 어댑터의 기능 목록으로 확인합니다.

관계·커리어 작업 16종과 이벤트 초안의 정확한 JSON 필드는 [local-api-operations.json](local-api-operations.json)에 있습니다. 예제의 UID 0은 자리표시자이므로 검색에서 확인한 실제 ID로 바꿔야 합니다. 먼저 미리보기 응답을 확인하며 템플릿 자체가 변경을 적용하지는 않습니다.

## HTTP 계약

서버는 `127.0.0.1`에만 바인딩합니다. 포트 `0`은 빈 포트를 선택합니다.

```text
Touchline Engine.exe --api --port 0 --discovery C:\...\api.json
```

| 요청 | 인증 | 결과 |
|---|---|---|
| `GET /health` | 없음 | 서버 PID·프로토콜 버전 등 최소 생존 정보 |
| `GET /v1/capabilities` | `Authorization: Bearer <token>` | 지원 명령·인수·워크플로 |
| `POST /v1/command` | 같은 인증 | 기존 의미 기반 브리지 명령 실행 |
| `POST /v1/shutdown` | 같은 인증 | API 서버만 순차적으로 정상 종료 |

명령 본문과 응답은 UTF-8 JSON입니다. 서로 다른 호출도 같은 서버 상태를 공유하며 순서대로 처리됩니다.

```json
{"id":1,"command":"search","args":{"kind":"City","query":"레체"}}
```

```json
{"id":1,"ok":true,"data":{"items":[],"total":0,"offset":0,"limit":100}}
```

실패 응답은 `{"id":1,"ok":false,"error":"설명"}`입니다. HTTP 인증·형식 오류는 별도의 4xx 응답이 될 수 있습니다.

## 응답이 끊겼을 때

클라이언트는 제출한 명령을 자동 재시도하지 않습니다. 특히 적용 요청 뒤 타임아웃·연결 끊김이 발생하면 적용 여부가 불명확하므로 이력과 해당 대상 상세를 먼저 확인합니다. 동일 변경을 다시 제출하면 의도하지 않게 중복 적용될 수 있습니다. 서버 재시작 이후에는 이전 검토 토큰을 사용할 수 없습니다.

`api-startup.log`는 API 실행 문제 확인용입니다. 시작 중 클라이언트가 강제 종료되어 `.starting` 파일만 남았다면, 실제 시작 작업이 없는지 확인한 뒤 해당 파일만 제거할 수 있습니다. 이 도구는 FM이나 다른 에디터 프로세스를 강제로 종료하지 않습니다.
