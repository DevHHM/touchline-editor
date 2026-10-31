//! Only scalar offsets verified in the existing project adapters are exposed here.
use crate::{memory::Process, model::*};
use anyhow::{Result, ensure};
use std::collections::HashMap;

// Recommendation weights for a loan manager entrusted with negotiations.
// Negotiating affects some native loan terms, but these weights are our rubric,
// not coefficients recovered from FM. Keep the shared 300-point display scale.
pub const LOAN_MANAGER_WEIGHTS: &[(&str, f64)] = &[
    ("judging_potential", 5.),
    ("judging_ability", 5.),
    ("analysis", 4.),
    ("management", 1.),
    ("negotiation", 4.),
];
pub fn loan_manager_score_scale() -> f64 {
    300. / (20. * LOAN_MANAGER_WEIGHTS.iter().map(|(_, w)| w).sum::<f64>())
}
const LOAN_MANAGER_HINT: &str = "협상 위임용 자체 추천식 · [(잠재력 판단 + 능력 판단) × 5 + 데이터 분석 × 4 + 인력 관리 + 협상 × 4] × 300 / 380 · 가중치는 FM 내부 계수가 아닙니다. 실제 업무 배정은 별도로 확인하세요";

pub fn scalar(
    p: &Process,
    key: &str,
    label: &str,
    section: &str,
    address: u64,
    storage: Storage,
    scale: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: &str,
) -> Result<Field> {
    let original = p.read(address, storage.size())?;
    let raw = match storage {
        Storage::Bit(bit) => {
            ((u64::from_le_bytes(original[..].try_into().unwrap()) >> bit) & 1) as f64
        }
        Storage::U8 => original[0] as f64,
        Storage::U16 => u16::from_le_bytes(original[..].try_into().unwrap()) as f64,
        Storage::U32 => u32::from_le_bytes(original[..].try_into().unwrap()) as f64,
        Storage::I32 => i32::from_le_bytes(original[..].try_into().unwrap()) as f64,
        Storage::U64 => u64::from_le_bytes(original[..].try_into().unwrap()) as f64,
        Storage::I64 => i64::from_le_bytes(original[..].try_into().unwrap()) as f64,
        Storage::F32 => f32::from_le_bytes(original[..].try_into().unwrap()) as f64,
        Storage::F64 => f64::from_le_bytes(original[..].try_into().unwrap()),
        Storage::FmDate => crate::extended::decode_date_value(u32::from_le_bytes(
            original[..].try_into().unwrap(),
        ))?,
        Storage::OptionalU32 => {
            let v = u32::from_le_bytes(original[..].try_into().unwrap());
            if v == u32::MAX { -1. } else { v as f64 }
        }
    };
    let value = raw / scale;
    ensure!(
        value.is_finite() && value >= min && value <= max,
        "{label} 값이 검증 범위를 벗어났습니다 ({value})"
    );
    Ok(Field {
        key: key.into(),
        label: label.into(),
        section: section.into(),
        value,
        min,
        max,
        step,
        hint: hint.into(),
        binding: Some(Binding {
            address,
            storage,
            scale,
            original,
        }),
    })
}
fn number(key: &str, label: &str, section: &str, value: f64, max: f64, hint: &str) -> Field {
    Field {
        key: key.into(),
        label: label.into(),
        section: section.into(),
        value,
        min: 0.,
        max,
        step: 0.5,
        hint: hint.into(),
        binding: None,
    }
}

const HIDDEN: [(&str, &str); 8] = [
    ("adaptability", "적응력"),
    ("ambition", "야망"),
    ("loyalty", "의리"),
    ("pressure", "압박감 대처"),
    ("professionalism", "프로의식"),
    ("sportsmanship", "스포츠맨십"),
    ("temperament", "참을성"),
    ("controversy", "논쟁성"),
];
const PLAYER: [(&str, &str, &str); 54] = [
    ("crossing", "크로스", "기술"),
    ("dribbling", "드리블", "기술"),
    ("finishing", "골 결정력", "기술"),
    ("heading", "헤더", "기술"),
    ("long_shots", "중거리 슛", "기술"),
    ("marking", "일대일 마크", "기술"),
    ("off_ball", "오프 더 볼", "정신"),
    ("passing", "패스", "기술"),
    ("penalties", "페널티킥", "기술"),
    ("tackling", "태클", "기술"),
    ("vision", "시야", "정신"),
    ("handling", "볼 핸들링", "골키퍼"),
    ("aerial_reach", "공중 장악력", "골키퍼"),
    ("command_of_area", "페널티 박스 장악력", "골키퍼"),
    ("communication", "수비 조율", "골키퍼"),
    ("kicking", "골킥", "골키퍼"),
    ("throwing", "공 던지기", "골키퍼"),
    ("anticipation", "예측력", "정신"),
    ("decisions", "판단력", "정신"),
    ("one_on_ones", "일대일 방어", "골키퍼"),
    ("positioning", "수비 위치 선정", "정신"),
    ("reflexes", "반사 신경", "골키퍼"),
    ("first_touch", "퍼스트 터치", "기술"),
    ("technique", "개인기", "기술"),
    ("left_foot", "왼발", "기본"),
    ("right_foot", "오른발", "기본"),
    ("flair", "천재성", "정신"),
    ("corners", "코너킥", "기술"),
    ("teamwork", "팀워크", "정신"),
    ("work_rate", "활동량", "정신"),
    ("long_throws", "장거리 스로인", "기술"),
    ("eccentricity", "기행", "골키퍼"),
    ("rushing_out", "돌진 빈도", "골키퍼"),
    ("punching", "펀칭 빈도", "골키퍼"),
    ("acceleration", "가속도", "신체"),
    ("free_kicks", "프리킥", "기술"),
    ("strength", "몸싸움", "신체"),
    ("stamina", "지구력", "신체"),
    ("pace", "주력", "신체"),
    ("jumping_reach", "점프 거리", "신체"),
    ("leadership", "리더십", "정신"),
    ("dirtiness", "비열함", "경기 히든"),
    ("balance", "균형 감각", "신체"),
    ("bravery", "대담성", "정신"),
    ("consistency", "일관성", "경기 히든"),
    ("aggression", "적극성", "정신"),
    ("agility", "민첩성", "신체"),
    ("important_matches", "중요 경기", "경기 히든"),
    ("injury_proneness", "부상 빈도", "경기 히든"),
    ("versatility", "다재다능", "경기 히든"),
    ("natural_fitness", "타고난 체력", "신체"),
    ("determination", "승부욕", "정신"),
    ("composure", "침착성", "정신"),
    ("concentration", "집중력", "정신"),
];
const POS: [(&str, &str); 15] = [
    ("gk", "골키퍼"),
    ("sw", "스위퍼"),
    ("dl", "왼쪽 풀백"),
    ("dc", "센터백"),
    ("dr", "오른쪽 풀백"),
    ("dm", "수비형 미드필더"),
    ("ml", "왼쪽 미드필더"),
    ("mc", "중앙 미드필더"),
    ("mr", "오른쪽 미드필더"),
    ("aml", "왼쪽 공격형 미드필더"),
    ("amc", "중앙 공격형 미드필더"),
    ("amr", "오른쪽 공격형 미드필더"),
    ("st", "스트라이커"),
    ("wbl", "왼쪽 윙백"),
    ("wbr", "오른쪽 윙백"),
];
// Offsets relative to non-player data+0x10. Discipline/youth are direct 0..20 bytes.
const STAFF: [(&str, &str, u64, f64); 23] = [
    ("discipline", "기강", 0x04, 1.),
    ("youth", "유소년 육성", 0x0C, 1.),
    ("determination", "승부욕", 0x0D, 5.),
    ("gk_shot", "골키퍼 슈팅 방어", 0x1B, 5.),
    ("judging_ability", "선수 능력 판단", 0x1C, 5.),
    ("judging_potential", "선수 잠재력 판단", 0x1D, 5.),
    ("management", "인력 관리", 0x1E, 5.),
    ("motivation", "동기 부여", 0x1F, 5.),
    ("physiotherapy", "치료 능력", 0x20, 5.),
    ("knowledge", "전술 이해도", 0x21, 5.),
    ("attack", "공격 훈련", 0x22, 5.),
    ("defense", "수비 훈련", 0x23, 5.),
    ("fitness", "체력 훈련", 0x24, 5.),
    ("mental", "심리 훈련", 0x25, 5.),
    ("technical", "기술 훈련", 0x26, 5.),
    ("tactical", "전술 훈련", 0x27, 5.),
    ("gk_handling", "골키퍼 볼 핸들링", 0x29, 5.),
    ("gk_distribution", "골키퍼 배급", 0x2A, 5.),
    ("analysis", "데이터 분석", 0x2C, 5.),
    ("sports_science", "스포츠 과학", 0x2F, 5.),
    ("negotiation", "협상", 0x31, 5.),
    ("judging_staff", "스태프 능력 판단", 0x32, 5.),
    ("set_pieces", "세트피스 훈련", 0x33, 5.),
];

pub fn populate(p: &Process, d: &mut Detail, legacy: bool) -> Result<()> {
    let person = d.entity.address;
    let base = d.entity.base;
    // Shared PersonFlags, not the player's preferred moves or career-plan list.
    // Backend detail/apply validates the supported build and person identity.
    // See reference/retire-after-current-club-static-20261029.json.
    if legacy { d.fields.push(scalar(
        p,
        "retire_after_current_club",
        "현재 구단 생활 후 은퇴 예정",
        "커리어 상태",
        person + 0x18,
        Storage::Bit(15),
        1.,
        0.,
        1.,
        1.,
        "현재 구단에서의 생활을 마친 뒤 은퇴할 예정이라는 상태입니다. 이적·계약 거절을 보장하지 않습니다. 일반 은퇴 상태·은퇴 날짜·커리어 계획과 다른 플래그이며 나머지 상태 비트는 보존합니다",
    )?); }
    for (i, (key, label)) in HIDDEN.iter().enumerate() {
        d.fields.push(scalar(
            p,
            key,
            label,
            "성격",
            person + 0x70 + i as u64,
            Storage::U8,
            1.,
            0.,
            20.,
            1.,
            if *key == "controversy" {
                "높을수록 논쟁적입니다"
            } else {
                "0은 미설정 값일 수 있습니다"
            },
        )?);
    }
    match d.entity.kind {
        EntityKind::Player => {
            // Player.Morale is an unscaled byte, independently verified in the
            // FM26 managed accessor and FM27 native Pmor property setter.
            // Unknown/sentinel values disable this optional field, not the profile.
            // See reference/fm27-compatibility/player-morale.json and
            // reference/player-morale-field-20261029.json.
            match scalar(
                p, "morale", "사기", "현재 상태", base + 0x26C,
                Storage::U8, 1., 0., 20., 1.,
                "현재 선수 사기 0–20입니다. 경기·대화·구단 상황에 따라 다시 변하며 불만 사유를 삭제하거나 값을 고정하지 않습니다",
            ) {
                Ok(field) => d.fields.push(field),
                Err(_) => d.notes.push("선수 사기가 검증 범위를 벗어나 이 항목의 편집을 비활성화했습니다".into()),
            }
            d.fields.push(scalar(
                p,
                "ca",
                "현재 능력 CA",
                "기본",
                base + 0x264,
                Storage::U16,
                1.,
                1.,
                200.,
                1.,
                "능력치 편집에 맞춰 자동 계산하지 않습니다",
            )?);
            d.fields.push(scalar(
                p,
                "pa",
                "잠재 능력 PA",
                "기본",
                base + 0x266,
                Storage::U16,
                1.,
                1.,
                200.,
                1.,
                "성장 상한이며 실제 성장을 보장하지 않습니다",
            )?);
            d.fields.push(scalar(
                p,
                "height",
                "키 (cm)",
                "기본",
                base + 0x22E,
                Storage::U16,
                1.,
                100.,
                230.,
                1.,
                "키와 점프 능력은 별도 수치입니다",
            )?);
            for (i, (key, label, section)) in PLAYER.iter().enumerate() {
                let hint = if *key == "injury_proneness" {
                    "낮을수록 부상에 강합니다. 현재 부상을 제거하는 기능은 아닙니다"
                } else if *key == "dirtiness" {
                    "높을수록 거친 반칙 성향입니다"
                } else {
                    "내부 수치를 1–20 척도로 표시합니다"
                };
                d.fields.push(scalar(
                    p,
                    key,
                    label,
                    section,
                    base + 0x15F + i as u64,
                    Storage::U8,
                    5.,
                    0.,
                    20.,
                    0.2,
                    hint,
                )?);
            }
            for (i, (key, label)) in POS.iter().enumerate() {
                d.fields.push(scalar(
                    p,
                    &format!("position_{key}"),
                    label,
                    "포지션",
                    base + 0x150 + i as u64,
                    Storage::U8,
                    1.,
                    0.,
                    20.,
                    1.,
                    "숙련도 0–20. 역할 적합도 점수와는 다릅니다",
                )?);
            }
            d.notes.push("CA·PA와 개별 능력치는 각각 수정됩니다. FM의 정확한 권장 CA 자동 계산은 구현하지 않았습니다".into());
        }
        EntityKind::Staff => {
            d.fields.push(scalar(
                p,
                "ca",
                "현재 능력 CA",
                "기본",
                base + 0xDA,
                Storage::U16,
                1.,
                0.,
                200.,
                1.,
                "",
            )?);
            d.fields.push(scalar(
                p,
                "pa",
                "잠재 능력 PA",
                "기본",
                base + 0xDC,
                Storage::U16,
                1.,
                0.,
                200.,
                1.,
                "",
            )?);
            d.fields.push(scalar(
                p,
                "world_reputation",
                "세계 명성",
                "기본",
                base + 0xD8,
                Storage::U16,
                1.,
                0.,
                10000.,
                1.,
                "구직 관심이나 협상 수락을 보장하지 않습니다",
            )?);
            for (key, label, offset, scale) in
                STAFF.iter().filter(|(k, _, _, _)| !k.starts_with('_'))
            {
                // FM27's UI/native property map has one GoalkeepingCoaching
                // attribute. The old handling/distribution bytes are unverified.
                if !legacy && matches!(*key, "gk_handling" | "gk_distribution") { continue; }
                d.fields.push(scalar(
                    p,
                    &format!("staff_{key}"),
                    if !legacy && *key == "gk_shot" { "골키퍼 훈련" } else { label },
                    "스태프 능력",
                    base + 0x10 + offset,
                    Storage::U8,
                    *scale,
                    0.,
                    20.,
                    1. / scale,
                    "사용자가 정한 분야별 점수 계산에 사용됩니다",
                )?);
            }
            // NonTacticalAttributes.BuyingPlayers is a direct byte at +0x1E,
            // separate from training attributes and tactical Attacking.
            // See reference/buying-players-field-evidence.json.
            if legacy { d.fields.push(scalar(
                p,
                "staff_buying_players",
                "선수 영입 성향",
                "비전술 성향",
                base + 0x1E,
                Storage::U8,
                1.,
                0.,
                20.,
                1.,
                "FM의 Buying Players 항목입니다. 훈련·전술 공격성이나 코치 점수에 포함되지 않으며, 특정 선수 영입·지출액·영입 횟수를 보장하지 않습니다. CA는 자동으로 변경하지 않습니다",
            )?); }
            append_scores(d);
            if !legacy {
                d.fields.retain(|f| !matches!(f.key.as_str(), "score_gk_distribution" | "score_youth_goalkeeper"));
                if let Some(f) = d.fields.iter_mut().find(|f| f.key == "score_gk_shot") { f.label = "골키퍼 코치".into(); }
                d.notes.push("FM27은 골키퍼 훈련 능력치가 하나입니다. 미검증 배급·핸들링 수치 및 이를 쓰는 별도 추천 점수는 표시하지 않습니다".into());
            }
            d.notes.push("분야별 점수는 사용자 추천 기준입니다. 임대 매니저는 협상을 반영한 보완식을 사용하며, 실제 훈련 별점·지원 의사·업무 배정을 대신하지 않습니다".into());
        }
        EntityKind::Person => {
            d.notes.push("이 인물 유형은 공통 성격 수치만 검증되어 있습니다. 선수·스태프 전용 필드는 노출하지 않습니다".into());
        }
        _ => {}
    }
    Ok(())
}
pub fn append_scores(d: &mut Detail) {
    let values = d
        .fields
        .iter()
        .map(|f| (f.key.clone(), f.value.round()))
        .collect::<HashMap<_, _>>();
    let a = |key: &str| *values.get(&format!("staff_{key}")).unwrap_or(&0.);
    let ddm = a("discipline") + a("determination") + a("motivation");
    let five = a("attack") + a("defense") + a("mental") + a("technical") + a("tactical");
    let mut scores = vec![];
    for (k, kl) in [("technical", "기술"), ("tactical", "전술")] {
        for (j, jl) in [("attack", "공격"), ("defense", "수비"), ("mental", "심리")] {
            scores.push((
                format!("{k}_{j}"),
                format!("{kl} · {jl}"),
                a(k) * 5. + a(j) * 4. + ddm * 2.,
            ));
        }
    }
    scores.extend([
        (
            "fitness".into(),
            "체력 코치".into(),
            a("fitness") * 9. + ddm * 2.,
        ),
        (
            "gk_shot".into(),
            "골키퍼 슈팅".into(),
            a("gk_shot") * 9. + ddm * 2.,
        ),
        (
            "gk_distribution".into(),
            "골키퍼 배급".into(),
            (a("gk_distribution") + a("gk_handling")) * 4.5 + ddm * 2.,
        ),
        (
            "set_pieces".into(),
            "세트피스 코치".into(),
            a("set_pieces") * 5. + a("knowledge") * 4. + ddm * 2.,
        ),
        (
            "assistant".into(),
            "1군 수석 코치".into(),
            (ddm + a("management")) * 2.
                + a("knowledge")
                + five
                + (a("judging_potential") + a("judging_ability")) / 2.,
        ),
        (
            "general".into(),
            "일반 코치".into(),
            a("management") + ddm * 3. + five,
        ),
        (
            "reserve_manager".into(),
            "2군 감독".into(),
            (a("management") + a("judging_potential") + a("judging_ability")) * 2.
                + ddm
                + five
                + a("knowledge"),
        ),
        (
            "youth_manager".into(),
            "유소년 감독".into(),
            ddm + (a("youth") + a("management")) * 2.
                + five
                + a("judging_potential")
                + a("judging_ability")
                + a("knowledge"),
        ),
        (
            "youth_assistant".into(),
            "유소년 수석 코치".into(),
            (ddm + a("youth")) * 2.
                + five
                + a("knowledge")
                + (a("judging_potential") + a("judging_ability")) / 2.,
        ),
        (
            "youth_coach".into(),
            "유소년 코치".into(),
            a("youth") + ddm * 3. + five,
        ),
        (
            "youth_goalkeeper".into(),
            "유소년 골키퍼 코치".into(),
            (a("gk_shot") + a("gk_handling") + a("gk_distribution")) * 3. + ddm * 2.,
        ),
        (
            "head_youth".into(),
            "수석 유소년 육성가".into(),
            a("youth") * 5.
                + (a("judging_potential") + a("judging_ability")) * 4.
                + a("discipline")
                + a("determination"),
        ),
        (
            "dof".into(),
            "풋볼 디렉터".into(),
            a("negotiation") * 7.
                + (a("judging_potential") + a("judging_ability")) * 3.
                + a("discipline")
                + a("determination"),
        ),
        (
            "td".into(),
            "테크니컬 디렉터".into(),
            (a("judging_staff") + a("negotiation")) * 5.
                + (a("discipline") + a("determination")) * 2.
                + a("management"),
        ),
        (
            "loan_manager".into(),
            "임대 매니저".into(),
            LOAN_MANAGER_WEIGHTS
                .iter()
                .map(|(key, weight)| a(key) * weight)
                .sum::<f64>()
                * loan_manager_score_scale(),
        ),
        (
            "scout".into(),
            "스카우트".into(),
            (a("judging_potential") + a("judging_ability")) * 5.
                + values.get("adaptability").copied().unwrap_or(0.) * 3.
                + a("discipline")
                + a("determination"),
        ),
        (
            "head_analyst".into(),
            "수석 전력 분석가".into(),
            a("analysis") * 6.
                + (a("knowledge") + a("judging_ability")) * 2.5
                + a("management") * 2.
                + a("discipline")
                + a("determination"),
        ),
        (
            "performance_analyst".into(),
            "전력 분석가".into(),
            a("analysis") * 7. + a("knowledge") * 6. + a("discipline") + a("determination"),
        ),
        (
            "recruitment_analyst".into(),
            "영입 분석가".into(),
            a("analysis") * 7. + a("judging_ability") * 6. + a("discipline") + a("determination"),
        ),
        (
            "physio".into(),
            "물리치료사 / 의사".into(),
            a("physiotherapy") * 9. + ddm * 2.,
        ),
        (
            "scientist".into(),
            "스포츠 과학자".into(),
            a("sports_science") * 9. + ddm * 2.,
        ),
    ]);
    for (key, label, value) in scores {
        d.fields.push(number(
            &format!("score_{key}"),
            &label,
            "직무 점수",
            value,
            300.,
            if key == "loan_manager" {
                LOAN_MANAGER_HINT
            } else {
                "사용자 계산식 · 현재 조회된 능력치 기준 / 300"
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attribute_layout_unique() {
        assert_eq!(PLAYER.len(), 54);
        let k = PLAYER
            .iter()
            .map(|x| x.0)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(k.len(), 54);
        assert_eq!(PLAYER[44].0, "consistency");
        assert_eq!(PLAYER[47].0, "important_matches");
        assert_eq!(PLAYER[48].0, "injury_proneness");
    }
    #[test]
    fn maximum_staff_scores() {
        let entity = Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id: 1,
            kind: EntityKind::Staff,
            name: String::new(),
            english_name: String::new(),
            club: String::new(),
            club_id: None,
            age: None,
            ca: None,
            pa: None,
            role: String::new(),
            group: String::new(),
            address: 0,
            base: 0,
            index: None,
        };
        let mut d = Detail {
            entity,
            fields: STAFF
                .iter()
                .filter(|(k, _, _, _)| !k.starts_with('_'))
                .map(|(k, _, _, _)| number(&format!("staff_{k}"), "", "", 20., 20., ""))
                .collect(),
            guards: vec![],
            notes: vec![],
        };
        d.fields.push(number("adaptability", "", "", 20., 20., ""));
        append_scores(&mut d);
        for f in d.fields.iter().filter(|f| f.key.starts_with("score_")) {
            assert_eq!(f.value, 300., "{}", f.key);
        }
    }
}
