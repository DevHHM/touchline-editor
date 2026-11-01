//! Further scalar fields for the fingerprint-gated FM26 26.3.2 adapter.
//! Sources and current-read evidence are recorded in reference/extended-fields.json.
//! These helpers never write memory. Every editable field goes through the normal
//! baseline, persistent backup, compare/readback, rollback and undo transaction.
use crate::{backend, fields, memory::Process, model::*};
use anyhow::{Context, Result, ensure};
mod contract_dates;

/// Parse the public numeric YYYYMMDD representation with Gregorian validation.
pub fn date_parts(value: f64) -> Result<(u32, u32, u32)> {
    ensure!(
        value.is_finite() && value.fract() == 0.,
        "날짜는 YYYYMMDD 형식의 정수로 입력하세요"
    );
    ensure!(
        (19_000_101. ..=21_001_231.).contains(&value),
        "날짜 범위는 1900-01-01–2100-12-31입니다"
    );
    let v = value as u32;
    let (year, month, day) = (v / 10_000, (v / 100) % 100, v % 100);
    let lengths = month_lengths(year);
    ensure!((1..=12).contains(&month), "존재하지 않는 월입니다");
    ensure!(
        (1..=lengths[(month - 1) as usize]).contains(&day),
        "존재하지 않는 날짜입니다"
    );
    Ok((year, month, day))
}

fn month_lengths(year: u32) -> [u32; 12] {
    [
        31,
        if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ]
}

pub fn parse_date_text(text: &str) -> Result<(u32, u32, u32)> {
    ensure!(
        text.len() == 10 && text.as_bytes()[4] == b'-' && text.as_bytes()[7] == b'-',
        "게임 날짜 형식 오류"
    );
    let numeric = text.replace('-', "").parse::<f64>()?;
    date_parts(numeric)
}

pub fn decode_date_value(raw: u32) -> Result<f64> {
    let (y, m, d) = backend::decode_date(raw).context("유효한 기존 날짜가 없습니다")?;
    Ok((y * 10_000 + m * 100 + d) as f64)
}

/// Only year/day bits change. FM's unknown seven date flag bits survive edits.
pub fn encode_date(value: f64, original: &[u8]) -> Result<u32> {
    ensure!(original.len() == 4, "날짜 원본 크기 불일치");
    let raw = u32::from_le_bytes(original.try_into().unwrap());
    decode_date_value(raw)?;
    let (year, month, day) = date_parts(value)?;
    let ordinal = month_lengths(year)[..(month - 1) as usize]
        .iter()
        .sum::<u32>()
        + day;
    Ok((year << 16) | (raw & 0xFE00) | ordinal)
}

fn optional(out: &mut Detail, result: Result<Field>, label: &str) {
    match result {
        Ok(field) => out.fields.push(field),
        Err(error) => out.notes.push(format!("{label} 필드 비활성: {error}")),
    }
}

fn guard(p: &Process, guards: &mut Vec<Guard>, address: u64, size: usize) -> Result<Vec<u8>> {
    let expected = p.read(address, size)?;
    guards.push(Guard {
        address,
        expected: expected.clone(),
    });
    Ok(expected)
}

fn readonly(key: &str, label: &str, value: f64, max: f64, hint: &str) -> Field {
    Field {
        key: key.into(),
        label: label.into(),
        section: "계약 상태".into(),
        value,
        min: 0.,
        max,
        step: 1.,
        hint: hint.into(),
        binding: None,
    }
}

/// The caller has already validated the exact game fingerprint and person class.
/// Optional families fail independently so an unusual contract does not hide attributes.
pub fn append(p: &Process, session: &Session, out: &mut Detail) {
    let person = out.entity.address;
    let base = out.entity.base;
    if let Ok((year, month, day)) = parse_date_text(&session.game_date) {
        optional(
            out,
            fields::scalar(
                p,
                "person.dob",
                "생년월일 (YYYYMMDD)",
                "인물 정보",
                person + 0x88,
                Storage::FmDate,
                1.,
                19_000_101.,
                (year * 10_000 + month * 100 + day) as f64,
                1.,
                "예: 20080229. 실제 달력 날짜를 검증합니다. 나이는 다시 계산되며 홈그로운·국적·경력 기록은 자동 변경되지 않습니다.",
            ),
            "생년월일",
        );
    }

    let reputations: &[(&str, &str, u64)] = match out.entity.kind {
        EntityKind::Player => &[
            ("home_reputation", "국내 명성", 0x25E),
            ("current_reputation", "현재 명성", 0x260),
            ("world_reputation", "세계 명성", 0x262),
        ],
        EntityKind::Staff => &[
            ("home_reputation", "국내 명성", 0xD4),
            ("current_reputation", "현재 명성", 0xD6),
        ],
        _ => &[],
    };
    for &(key, label, offset) in reputations {
        optional(
            out,
            fields::scalar(
                p,
                key,
                label,
                "명성",
                base + offset,
                Storage::U16,
                1.,
                0.,
                10_000.,
                1.,
                "명성 0–10,000. 이적·고용·계약 수락 여부를 보장하지 않습니다.",
            ),
            label,
        );
    }
    // FM27 contract/health/finance records are not certified by the common
    // person/core-attribute layout. Do not display plausible but unverified data.
    if !crate::compatibility::legacy_layout(session) { contract_dates::append(p, session, out); return; }
    if out.entity.kind == EntityKind::Player {
        for (key, label, offset) in [
            ("player.market_value", "기준 시장 가치 (£)", 0x234),
            ("player.asking_price", "요구 이적료 (£)", 0x238),
        ] {
            optional(
                out,
                fields::scalar(
                    p,
                    key,
                    label,
                    "이적",
                    base + offset,
                    Storage::OptionalU32,
                    1.,
                    -1.,
                    2_000_000_000.,
                    1.,
                    "영국 파운드(GBP). -1은 미설정입니다. 게임이 재평가할 수 있으며 실제 이적 합의나 거래를 생성하지 않습니다.",
                ),
                label,
            );
        }
        let injured = fields::scalar(p, "player.injured", "현재 부상 표시", "건강", base + 0x1E1,
            Storage::U8, 1., 0., 1., 1., "읽기 전용: 0 없음 / 1 있음. 부상 기록·회복 기간과 연동하므로 표시만 변경하지 않습니다.")
            .map(|mut f| { f.binding = None; f });
        optional(out, injured, "부상 표시");
    }

    match contract_fields(p, session, person) {
        Ok(Some((fields, guards))) => {
            out.fields.extend(fields);
            out.guards.extend(guards);
        }
        Ok(None) => out
            .notes
            .push("확인된 완전 계약이 없어 주급·계약 만료일을 편집하지 않습니다.".into()),
        Err(error) => out.notes.push(format!("계약 필드 비활성: {error}")),
    }
}

fn contract_fields(
    p: &Process,
    session: &Session,
    person: u64,
) -> Result<Option<(Vec<Field>, Vec<Guard>)>> {
    let contract = p.u64(person + 0xA8)?;
    if contract == 0 {
        return Ok(None);
    }
    ensure!(
        (0x10000..0x7fff_ffff_ffff).contains(&contract),
        "계약 포인터 범위 오류"
    );
    let kind = backend::type_name(p, session, p.u64(contract)?);
    if kind != ".?AVFULL_CONTRACT@db@@" {
        return Ok(None);
    }
    ensure!(
        p.u64(contract + 8)? == person,
        "계약의 인물 연결이 일치하지 않습니다"
    );
    let mut guards = Vec::new();
    guard(p, &mut guards, person + 0xA8, 8)?;
    // Guard the complete known record around its only editable cells; no writes
    // to team links, job, clauses, bonus pointers or transfer-status flags.
    guard(p, &mut guards, contract, 0x20)?;
    guard(p, &mut guards, contract + 0x24, 0x24)?;
    let tail = guard(p, &mut guards, contract + 0x4C, 0x1C)?;
    let mut fields = vec![fields::scalar(
        p,
        "contract.weekly_wage",
        "계약 주급 (£/주)",
        "계약",
        contract + 0x20,
        Storage::U32,
        1.,
        0.,
        100_000_000.,
        1.,
        "기존 기본 주급(GBP). 보너스·임대 분담률·구단 주급 예산은 자동 변경되지 않습니다.",
    )?];
    if backend::decode_date(p.u32(contract + 0x48)?).is_some() {
        fields.push(fields::scalar(p, "contract.expires", "계약 만료일 (YYYYMMDD)", "계약",
            contract + 0x48, Storage::FmDate, 1., 19_000_101., 21_001_231., 1.,
            "기존 계약 만료일만 변경합니다. 갱신 합의·약속·옵션·보너스를 새로 만들지 않습니다. 예: 20300630.")?);
    }
    // The transfer status byte is intentionally decoded for display, never
    // offered as an arbitrary byte that could destroy unknown flag bits.
    let flags = tail[0x57 - 0x4C];
    for (key, label, bit) in [
        ("listed", "이적 명단 등록", 0),
        ("listed_by_request", "요청으로 이적 명단 등록", 3),
        ("not_for_sale", "판매 불가", 4),
        ("set_for_release", "방출 예정", 5),
    ] {
        fields.push(readonly(
            &format!("contract.{key}"),
            label,
            f64::from((flags >> bit) & 1),
            1.,
            "읽기 전용: 0 아니오 / 1 예. 상태 간 연동 변경은 아직 지원하지 않습니다.",
        ));
    }
    let shirt = tail[0x5D - 0x4C];
    if shirt <= 99 {
        fields.push(readonly(
            "contract.squad_number",
            "계약 등번호",
            shirt as f64,
            99.,
            "읽기 전용. 대회별 등록·팀 내 중복 번호를 별도로 검증해야 합니다.",
        ));
    }
    Ok(Some((fields, guards)))
}

const CAPACITY_KEYS: [&str; 3] = [
    "club.stadium_capacity",
    "club.stadium_usable",
    "club.stadium_expansion",
];

/// Validate the final draft as a whole, including unchanged capacity members.
pub fn validate_edits(detail: &Detail, edits: &[Edit]) -> Result<()> {
    contract_dates::validate_edits(detail, edits)?;
    if !edits
        .iter()
        .any(|edit| CAPACITY_KEYS.contains(&edit.key.as_str()))
    {
        return Ok(());
    }
    let value = |key: &str| -> Result<f64> {
        edits
            .iter()
            .find(|edit| edit.key == key)
            .map(|edit| edit.value)
            .or_else(|| {
                detail
                    .fields
                    .iter()
                    .find(|field| field.key == key)
                    .map(|field| field.value)
            })
            .with_context(|| format!("구장 연동 필드가 없습니다: {key}"))
    };
    let capacity = value("club.stadium_capacity")?;
    let usable = value("club.stadium_usable")?;
    let seated = value("club.stadium_seated")?;
    let expansion = value("club.stadium_expansion")?;
    ensure!(
        usable <= capacity,
        "구장 사용 가능 인원은 총 수용 인원을 넘을 수 없습니다"
    );
    ensure!(
        seated <= capacity,
        "구장 수용 인원은 기존 좌석 수보다 작을 수 없습니다"
    );
    ensure!(
        expansion == 0. || capacity <= expansion,
        "구장 최대 확장 인원은 총 수용 인원보다 작을 수 없습니다 (0은 미설정)"
    );
    Ok(())
}

/// Persist the untouched capacity members as transaction dependencies. The seat
/// and adjacent auxiliary cell are guarded by extras when the detail is read.
pub fn dependency_guards(detail: &Detail, changes: &[Change]) -> Result<Vec<Guard>> {
    if !changes
        .iter()
        .any(|change| CAPACITY_KEYS.contains(&change.key.as_str()))
    {
        return Ok(contract_dates::dependency_guards(detail, changes));
    }
    let mut guards = contract_dates::dependency_guards(detail, changes);
    for key in CAPACITY_KEYS {
        if changes.iter().any(|change| change.key == key) {
            continue;
        }
        let binding = detail
            .fields
            .iter()
            .find(|field| field.key == key)
            .and_then(|field| field.binding.as_ref())
            .context("구장 연동 편집 바인딩이 없습니다")?;
        guards.push(Guard {
            address: binding.address,
            expected: binding.original.clone(),
        });
    }
    Ok(guards)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(storage: Storage, value: f64, original: Vec<u8>, min: f64, max: f64) -> Field {
        Field {
            key: "test".into(),
            label: "test".into(),
            section: String::new(),
            value,
            min,
            max,
            step: 1.,
            hint: String::new(),
            binding: Some(Binding {
                address: 0x10000,
                storage,
                scale: 1.,
                original,
            }),
        }
    }

    #[test]
    fn date_roundtrip_including_leap_centuries_preserves_flags() {
        for (numeric, ordinal) in [
            (19_000_301., 60),
            (20_000_229., 60),
            (20_280_229., 60),
            (21_001_231., 365),
        ] {
            let before = ((2027u32 << 16) | 0xBE00 | 300).to_le_bytes();
            let encoded = encode_date(numeric, &before).unwrap();
            assert_eq!(encoded & 0xFE00, 0xBE00);
            assert_eq!(encoded & 0x1FF, ordinal);
            assert_eq!(decode_date_value(encoded).unwrap(), numeric);
        }
    }

    #[test]
    fn rejects_invalid_dates_in_numeric_transaction_path() {
        let original = ((2027u32 << 16) | 300).to_le_bytes().to_vec();
        let f = field(
            Storage::FmDate,
            20_271_027.,
            original,
            19_000_101.,
            21_001_231.,
        );
        for bad in [
            20_270_229.,
            19_000_229.,
            21_000_229.,
            20_271_331.,
            20_270_400.,
            20_270_431.,
            20_270_101.5,
            f64::NAN,
        ] {
            assert!(crate::transaction::encode_value(&f, bad).is_err(), "{bad}");
        }
        assert!(crate::transaction::encode_value(&f, 20_280_229.).is_ok());
        assert!(encode_date(20_280_229., &[0; 4]).is_err());
    }

    #[test]
    fn optional_money_keeps_unset_distinct_from_zero() {
        let f = field(
            Storage::OptionalU32,
            -1.,
            u32::MAX.to_le_bytes().to_vec(),
            -1.,
            2_000_000_000.,
        );
        assert_eq!(
            crate::transaction::encode_value(&f, -1.).unwrap(),
            u32::MAX.to_le_bytes()
        );
        assert_eq!(
            crate::transaction::encode_value(&f, 0.).unwrap(),
            0u32.to_le_bytes()
        );
        assert_eq!(
            crate::transaction::encode_value(&f, 125_000_000.).unwrap(),
            125_000_000u32.to_le_bytes()
        );
        assert!(crate::transaction::encode_value(&f, -2.).is_err());
        assert!(crate::transaction::encode_value(&f, 0.5).is_err());
        assert!(crate::transaction::encode_value(&f, 2_000_000_001.).is_err());
    }

    #[test]
    fn birthdate_does_not_allow_future_and_readonly_flags_cannot_encode() {
        let f = field(
            Storage::FmDate,
            20_000_101.,
            (2000u32 << 16 | 1).to_le_bytes().to_vec(),
            19_000_101.,
            20_271_126.,
        );
        assert!(crate::transaction::encode_value(&f, 20_271_127.).is_err());
        assert!(
            crate::transaction::encode_value(&readonly("listed", "Listed", 0., 1., ""), 1.)
                .is_err()
        );
        assert!(parse_date_text("2027-11-26").is_ok());
        assert!(parse_date_text("2027-02-29").is_err());
    }

    #[test]
    fn validates_capacity_draft_and_guards_unchanged_members() {
        let mut d = Detail {
            entity: Entity {
                nationality: String::new(),
                nationality_id: None,
                football_category: crate::model::FootballCategory::Unknown,
                id: 1,
                kind: EntityKind::Club,
                name: "Fixture".into(),
                english_name: String::new(),
                club: String::new(),
                club_id: Some(1),
                age: None,
                ca: None,
                pa: None,
                role: String::new(),
                group: String::new(),
                address: 0x20000,
                base: 0x20000,
                index: None,
            },
            fields: Vec::new(),
            guards: Vec::new(),
            notes: Vec::new(),
        };
        for (n, (key, value)) in [
            ("club.stadium_capacity", 40000.),
            ("club.stadium_usable", 38000.),
            ("club.stadium_expansion", 50000.),
            ("club.stadium_seated", 30000.),
        ]
        .into_iter()
        .enumerate()
        {
            let mut f = field(
                Storage::U32,
                value,
                (value as u32).to_le_bytes().to_vec(),
                0.,
                500_000.,
            );
            f.key = key.into();
            f.binding.as_mut().unwrap().address += n as u64 * 4;
            if key.ends_with("seated") {
                f.binding = None;
            }
            d.fields.push(f);
        }
        let edits = |v| {
            vec![Edit {
                key: "club.stadium_capacity".into(),
                value: v,
            }]
        };
        assert!(validate_edits(&d, &edits(39000.)).is_ok());
        assert!(validate_edits(&d, &edits(37000.)).is_err());
        assert!(validate_edits(&d, &edits(51000.)).is_err());
        let changes = crate::transaction::prepare(&d, &edits(39000.)).unwrap();
        let guards = dependency_guards(&d, &changes).unwrap();
        assert_eq!(guards.len(), 2);
        assert_eq!(guards[0].expected, 38000u32.to_le_bytes());
        assert_eq!(guards[1].expected, 50000u32.to_le_bytes());
        let expansion = Edit {
            key: "club.stadium_expansion".into(),
            value: 60000.,
        };
        assert!(validate_edits(&d, &[edits(51000.).remove(0), expansion]).is_ok());
    }
}
