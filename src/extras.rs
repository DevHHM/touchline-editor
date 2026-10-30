//! Bounded club/city access for the build accepted by backend::connect.
//! Field access is guarded by the build adapter and validated against native layouts.
//! No process-wide scan, allocation, native call or write occurs in this module.
use anyhow::{Context, Result, bail, ensure};
use std::collections::HashMap;

use crate::memory::Process;
use crate::model::{Binding, Detail, Entity, EntityKind, Field, Guard, Index, Session, Storage};
mod cities;

fn q(raw: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}
fn d(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
fn pointer(p: u64) -> bool {
    (0x10000..0x0000_8000_0000_0000).contains(&p)
}
fn guard(p: &Process, out: &mut Vec<Guard>, address: u64, size: usize) -> Result<Vec<u8>> {
    let expected = p.read(address, size)?;
    // Transaction guards are bounded to 256 bytes each; a club can have a larger
    // team-slot vector without forcing its otherwise valid scalar edits to fail.
    for (i, chunk) in expected.chunks(256).enumerate() {
        out.push(Guard {
            address: address + (i * 256) as u64,
            expected: chunk.to_vec(),
        });
    }
    Ok(expected)
}
fn text(p: &Process, at: u64) -> Result<String> {
    let address = p.u64(at)?;
    ensure!(pointer(address), "이름 포인터가 유효하지 않습니다");
    let raw = p.read(address + 4, 256)?;
    let end = raw
        .iter()
        .position(|&v| v == 0)
        .context("이름이 너무 깁니다")?;
    let result = std::str::from_utf8(&raw[..end])
        .context("이름 인코딩이 유효하지 않습니다")?
        .to_owned();
    ensure!(
        !result.is_empty() && !result.chars().any(char::is_control),
        "이름이 비어 있거나 손상되었습니다"
    );
    Ok(result)
}

fn expect_type(p: &Process, s: &Session, address: u64, expected: &str) -> Result<()> {
    ensure!(pointer(address), "객체 주소가 유효하지 않습니다");
    let vt = p.u64(address)?;
    let end = s
        .module_base
        .checked_add(s.module_size as u64)
        .context("모듈 범위 오류")?;
    ensure!(
        (s.module_base + 8..end).contains(&vt),
        "객체 형식이 바뀌었습니다"
    );
    let locator = p.u64(vt - 8)?;
    ensure!(
        (s.module_base..end.saturating_sub(24)).contains(&locator),
        "객체 RTTI가 유효하지 않습니다"
    );
    let col = p.read(locator, 24)?;
    ensure!(
        d(&col, 0) == 1 && locator.checked_sub(d(&col, 20) as u64) == Some(s.module_base),
        "객체 RTTI 모듈 불일치"
    );
    let descriptor = s
        .module_base
        .checked_add(d(&col, 12) as u64)
        .context("RTTI 오버플로")?;
    ensure!(
        (s.module_base..end.saturating_sub(144)).contains(&descriptor),
        "형식 이름 범위 오류"
    );
    let name = p.read(descriptor + 16, 128)?;
    let terminator = name.iter().position(|&v| v == 0).unwrap_or(name.len());
    ensure!(
        &name[..terminator] == expected.as_bytes(),
        "예상한 {} 객체가 아닙니다",
        expected
    );
    Ok(())
}

struct TeamRoute {
    team: u64,
    guards: Vec<Guard>,
}
fn first_team(p: &Process, club: u64) -> Result<TeamRoute> {
    let mut guards = Vec::new();
    let header = guard(p, &mut guards, club + 0x18, 16)?;
    let (begin, end) = (q(&header, 0), q(&header, 8));
    ensure!(
        pointer(begin) && end > begin && end - begin <= 512 && (end - begin) % 8 == 0,
        "1군 팀 목록을 확인할 수 없습니다"
    );
    let slots = guard(p, &mut guards, begin, (end - begin) as usize)?;
    let mut first = Vec::new();
    for chunk in slots.chunks_exact(8) {
        let team = q(chunk, 0);
        if !pointer(team) {
            continue;
        }
        let Ok(raw) = p.read(team + 0x28, 16) else {
            continue;
        };
        if raw[0] == 0 && q(&raw, 8) == club {
            first.push(team);
        }
    }
    ensure!(first.len() == 1, "1군 팀 연결이 모호합니다");
    let team = first[0];
    guard(p, &mut guards, team, 16)?;
    guard(p, &mut guards, team + 0x28, 1)?;
    guard(p, &mut guards, team + 0x30, 8)?;
    Ok(TeamRoute { team, guards })
}

// Indexing needs no transaction guard allocations. Batch the same verified
// offsets; the selected entity is fully re-resolved by detail before editing.
fn city_for_index(p: &Process, club: &Entity) -> Result<u64> {
    let raw = p.read(club.address, 0x28)?;
    ensure!(d(&raw, 12) == club.id, "구단 ID 변경");
    let (begin, end) = (q(&raw, 0x18), q(&raw, 0x20));
    ensure!(
        pointer(begin) && end > begin && end - begin <= 512 && (end - begin) % 8 == 0,
        "팀 목록 범위 오류"
    );
    let slots = p.read(begin, (end - begin) as usize)?;
    let mut stadium = None;
    for chunk in slots.chunks_exact(8) {
        let team = q(chunk, 0);
        if !pointer(team) {
            continue;
        }
        let Ok(raw) = p.read(team + 0x28, 0x58) else {
            continue;
        };
        if raw[0] == 0 && q(&raw, 8) == club.address {
            ensure!(stadium.is_none(), "1군 팀 연결 중복");
            stadium = Some(q(&raw, 0x50));
        }
    }
    let stadium = stadium.context("1군 구장 없음")?;
    ensure!(pointer(stadium), "구장 주소 오류");
    let city = p.u64(stadium + 0x60)?;
    ensure!(pointer(city), "도시 주소 오류");
    Ok(city)
}
/// Preserve known club navigation, then enumerate the complete typed city table.
pub fn append_clubs_and_cities(p: &Process, index: &mut Index) -> Result<()> {
    let clubs: Vec<Entity> = index
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Club)
        .cloned()
        .collect();
    let mut cities: HashMap<u32, Entity> = HashMap::new();
    let mut by_address: HashMap<u64, u32> = HashMap::new();
    let mut unavailable = 0usize;
    for club in &clubs {
        let row = (|| -> Result<Option<Entity>> {
            let city = city_for_index(p, club)?;
            if let Some(id) = by_address.get(&city) {
                if Some(club.id) == index.session.own_club_id {
                    if let Some(row) = cities.get_mut(id) {
                        row.base = club.address;
                        row.club_id = Some(club.id);
                        row.club = club.name.clone();
                    }
                }
                return Ok(None);
            }
            let raw = p.read(city, 0x58)?;
            let id = d(&raw, 12);
            let lat = f32::from_le_bytes(raw[0x48..0x4c].try_into().unwrap());
            let lon = f32::from_le_bytes(raw[0x4c..0x50].try_into().unwrap());
            ensure!(
                id > 0
                    && lat.is_finite()
                    && lon.is_finite()
                    && (-90.0..=90.0).contains(&lat)
                    && (-180.0..=180.0).contains(&lon)
                    && raw[0x52] <= 20
                    && raw[0x54] <= 20,
                "도시 레코드 범위 오류"
            );
            let name = text(p, city + 0x18)?;
            by_address.insert(city, id);
            Ok(Some(Entity {
                nationality: String::new(),
                nationality_id: None,
                football_category: crate::model::FootballCategory::Unknown,
                id,
                kind: EntityKind::City,
                english_name: name.clone(),
                name,
                club: club.name.clone(),
                club_id: Some(club.id),
                age: None,
                ca: None,
                pa: None,
                role: "홈 구장 소재지".into(),
                group: "도시".into(),
                address: city,
                base: club.address,
                index: Some(d(&raw, 8)),
            }))
        })();
        match row {
            Ok(Some(city)) => {
                let replace =
                    !cities.contains_key(&city.id) || city.club_id == index.session.own_club_id;
                if replace {
                    cities.insert(city.id, city);
                }
            }
            Ok(None) => {}
            Err(_) => unavailable += 1,
        }
    }
    let mut rows = cities::enumerate(p, &index.session, &cities)?;
    rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    index.entities.extend(rows);
    if unavailable > 0 {
        index.warnings.push(format!("구단 {unavailable}곳의 홈 구장 연결은 확인하지 못했습니다. 도시 목록은 전체 도시 데이터베이스에서 별도로 읽었습니다."));
    }
    index
        .warnings
        .push("도시는 전체 도시 데이터베이스 기준입니다. 도시의 분류(group)는 실제 소속 국가이며, 국가 미확인은 별도로 표시합니다.".into());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scalar(
    p: &Process,
    key: &str,
    label: &str,
    section: &str,
    address: u64,
    storage: Storage,
    min: f64,
    max: f64,
    hint: &str,
    editable: bool,
) -> Result<Field> {
    let raw = p.read(address, storage.size())?;
    let value = match storage {
        Storage::U8 => raw[0] as f64,
        Storage::U16 => u16::from_le_bytes(raw[..2].try_into().unwrap()) as f64,
        Storage::U32 => u32::from_le_bytes(raw[..4].try_into().unwrap()) as f64,
        Storage::I32 => i32::from_le_bytes(raw[..4].try_into().unwrap()) as f64,
        Storage::F32 => f32::from_le_bytes(raw[..4].try_into().unwrap()) as f64,
        _ => bail!("지원하지 않는 구단 필드 저장 형식"),
    };
    ensure!(
        value.is_finite() && value >= min && value <= max,
        "{} 값이 검증 범위를 벗어났습니다",
        label
    );
    Ok(Field {
        key: key.into(),
        label: label.into(),
        section: section.into(),
        value,
        min,
        max,
        step: 1.0,
        hint: hint.into(),
        binding: editable.then_some(Binding {
            address,
            storage,
            scale: 1.0,
            original: raw,
        }),
    })
}

type FinanceFieldSpec = (&'static str, &'static str, u64, f64);

fn finance_field_specs(legacy: bool) -> &'static [FinanceFieldSpec] {
    if legacy {
        &[
            ("club.cash", "구단 잔고 (£)", 0x14, -2_000_000_000.0),
            ("club.transfer_base", "기본 이적 예산 (£)", 0x7cc, -2_000_000_000.0),
            ("club.transfer_current", "현재 이적 예산 (£)", 0x7d0, -2_000_000_000.0),
            ("club.weekly_wage", "주급 예산 (£/주)", 0x810, 0.0),
            ("club.player_wage_max", "선수 최대 주급 (£/주)", 0x830, 0.0),
            ("club.staff_wage_max", "스태프 최대 주급 (£/주)", 0x834, 0.0),
        ]
    } else {
        // FM27 CLUB_FINANCE virtual getters, corroborated against 14 typed
        // live records. The old +8 club backlink no longer exists. Cash Ccba
        // resolves virtual +0x1a8 to I32 +0x0c in BASE and FULL finance alike.
        // Sugar-daddy is deliberately absent until independently mapped.
        // Evidence: reference/fm27-compatibility/club-city-static/.
        &[
            ("club.cash", "구단 잔고 (£)", 0x0c, -2_000_000_000.0),
            ("club.transfer_base", "기본 이적 예산 (£)", 0x7c4, -2_000_000_000.0),
            ("club.transfer_current", "현재 이적 예산 (£)", 0x7c8, -2_000_000_000.0),
            ("club.weekly_wage", "주급 예산 (£/주)", 0x808, 0.0),
            ("club.player_wage_max", "선수 최대 주급 (£/주)", 0x828, 0.0),
            ("club.staff_wage_max", "스태프 최대 주급 (£/주)", 0x82c, 0.0),
        ]
    }
}

pub fn detail(p: &Process, session: &Session, entity: &Entity) -> Result<Detail> {
    ensure!(
        matches!(entity.kind, EntityKind::Club | EntityKind::City),
        "구단/도시 대상이 아닙니다"
    );
    if entity.kind == EntityKind::City {
        return cities::detail(p, session, entity);
    }
    let profile = crate::backend::profiles::profile_for_session(session)
        .context("구단 상세의 FM 빌드가 확인되지 않았습니다")?;
    profile.require_read_layout()?;
    let legacy = profile.game == "FM26";
    let writable = crate::backend::profiles::can_write(session);
    let mut out = Detail {
        entity: entity.clone(),
        fields: Vec::new(),
        guards: Vec::new(),
        notes: Vec::new(),
    };
    let club = entity.address;
    expect_type(p, session, club, ".?AVCLUB@db@@")?;
    let club_head = guard(p, &mut out.guards, club, 16)?;
    ensure!(
        d(&club_head, 12) == entity.id,
        "구단 ID가 변경되었습니다. 다시 연결해 주세요"
    );
    if let Some(i) = entity.index {
        ensure!(d(&club_head, 8) == i, "구단 인덱스 불일치");
    }
    guard(p, &mut out.guards, club + 0xc0, 8)?;

    let finance = p.u64(club + 0x150)?;
    if pointer(finance) {
        let finance_fields = (|| -> Result<(Vec<Field>, Vec<Guard>)> {
            expect_type(p, session, finance, ".?AVCLUB_FINANCE@db@@")?;
            if legacy {
                ensure!(
                    p.u64(finance + 8)? == club,
                    "재정 객체의 구단 연결이 일치하지 않습니다"
                );
            }
            let mut guards = Vec::new();
            guard(p, &mut guards, club + 0x150, 8)?;
            guard(p, &mut guards, finance, if legacy { 16 } else { 8 })?;
            let mut fields = Vec::new();
            for &(key, label, offset, min) in finance_field_specs(legacy) {
                fields.push(scalar(p,key,label,"재정",finance+offset,Storage::I32,min,2_000_000_000.0,
                    "게임 내부 기준 영국 파운드(GBP). 잔고와 예산은 별도 항목이며 서로 자동 합산되지 않습니다.",writable)?);
            }
            // FMRTE BasicFinances.get/set_SugarDaddy uses Afx.Aie as a byte.
            // Its full-finance layout is +0x3C (not the basic-layout +0x34).
            // Both static layout anchors and this build's typed live records
            // were corroborated; see reference/sugar-daddy-field-evidence.json.
            if legacy { fields.push(scalar(p,"club.sugar_daddy","구단주 자금 지원 유형","재정",
                finance+0x3c,Storage::U8,0.0,4.0,
                "0 없음 · 1 적극 투자형(Front End) · 2 후방 지원형 · 3 손실 보전형 · 4 수익을 기대하는 손실 보전형. 향후 자금 지원 성향 설정이며, 구단주 명의 변경·인수 뉴스 생성·즉시 현금 입금은 아닙니다.",writable)?); }
            Ok((fields, guards))
        })();
        match finance_fields {
            Ok((fields, guards)) => {
                out.fields.extend(fields);
                out.guards.extend(guards);
            }
            Err(e) => out.notes.push(format!("재정 필드 비활성: {e}")),
        }
    } else {
        out.notes
            .push("이 구단에는 확인할 수 있는 재정 객체가 없습니다.".into());
    }

    match first_team(p, club) {
        Ok(route) => {
            out.guards.extend(route.guards);
            match scalar(
                p,
                "club.reputation",
                "구단 명성",
                "구단",
                route.team + 0xa8,
                Storage::U16,
                0.0,
                10000.0,
                "1군 팀의 명성 · 범위 0–10,000",
                writable,
            ) {
                Ok(f) => out.fields.push(f),
                Err(e) => out.notes.push(format!("명성 필드 비활성: {e}")),
            }
            let stadium_fields = (|| -> Result<(Vec<Field>, Vec<Guard>, String)> {
                let mut guards = Vec::new();
                let stadium = q(&guard(p, &mut guards, route.team + 0x78, 8)?, 0);
                expect_type(p, session, stadium, ".?AVSTADIUM@db@@")?;
                guard(p, &mut guards, stadium, 16)?;
                let owner = q(&guard(p, &mut guards, stadium + 0x90, 8)?, 0);
                let name = text(p, stadium + 0x40).unwrap_or_else(|_| "홈 구장".into());
                let mut fields = Vec::new();
                // Resolve from the selected club's actual stadium. A city can
                // be shared by several clubs, so its index row's club_id is
                // only one discovery source and must not be used as the sole
                // navigation relationship.
                if let Ok(city) = p.u64(stadium + 0x60) {
                    if pointer(city) && expect_type(p, session, city, ".?AVCITY@db@@").is_ok() {
                        if let Ok(city_id) = scalar(
                            p,
                            "club.city_id",
                            "연고 도시 ID",
                            "연결",
                            city + 12,
                            Storage::U32,
                            1.0,
                            u32::MAX as f64,
                            "구단 홈 구장의 실제 도시 연결",
                            false,
                        ) {
                            fields.push(city_id);
                        }
                    }
                }
                fields.push(scalar(
                    p,
                    "club.corporate_facilities",
                    "구장 기업 시설",
                    "구장",
                    stadium + 0xae,
                    Storage::U8,
                    0.0,
                    20.0,
                    if owner == club {
                        "구단 소유 홈 구장의 기업 시설 수준 · 0–20"
                    } else {
                        "구단 소유가 아닌 홈 구장 · 읽기 전용"
                    },
                    writable && owner == club,
                )?);
                fields.push(scalar(
                    p,
                    "club.stadium_condition",
                    "구장 상태",
                    "구장",
                    stadium + 0xa8,
                    if legacy { Storage::U8 } else { Storage::U16 },
                    0.0,
                    200.0,
                    if owner == club {
                        "구단 소유 홈 구장 상태 · 0–200. 잔디 종류·지붕·시설 플래그는 별도입니다."
                    } else {
                        "구단 소유가 아닌 홈 구장 · 읽기 전용"
                    },
                    writable && owner == club,
                )?);
                // Capacity edits preserve seated/auxiliary values. These bytes
                // participate in the capacity invariant and must remain stable.
                guard(p, &mut guards, stadium + 0x84, 8)?;
                for (key, label, offset, editable) in [
                    (
                        "club.stadium_capacity",
                        "구장 수용 인원",
                        0x7c,
                        writable && owner == club,
                    ),
                    (
                        "club.stadium_usable",
                        "구장 사용 가능 인원",
                        0x80,
                        writable && owner == club,
                    ),
                    ("club.stadium_seated", "좌석 수", 0x84, false),
                    (
                        "club.stadium_expansion",
                        "구장 최대 확장 인원",
                        0x8c,
                        writable && owner == club,
                    ),
                ] {
                    fields.push(scalar(
                        p,
                        key,
                        label,
                        "구장",
                        stadium + offset,
                        Storage::U32,
                        0.0,
                        500_000.0,
                        if editable {
                            "사용 가능 인원·좌석 수 ≤ 수용 인원 ≤ 최대 확장 인원(0은 미설정). 새 구장·공사·구단 소유권은 변경하지 않습니다."
                        } else {
                            "읽기 전용. 구단 비소유 구장 또는 독립 변경이 검증되지 않은 좌석 항목입니다."
                        },
                        editable,
                    )?);
                }
                Ok((fields, guards, name))
            })();
            match stadium_fields {
                Ok((fields, guards, name)) => {
                    out.fields.extend(fields);
                    out.guards.extend(guards);
                    out.notes.push(format!("홈 구장: {name}"));
                }
                Err(e) => out.notes.push(format!("구장 필드 비활성: {e}")),
            }
        }
        Err(e) => out.notes.push(format!("1군 팀 세부 정보 미확인: {e}")),
    }

    // Existing contract amounts are visible; adding/replacing contracts needs a
    // dedicated operation to validate dates, total/annual amounts and categories.
    let sponsors = (|| -> Result<Vec<Field>> {
        if !legacy { return Ok(Vec::new()); }
        let wrapper = p.u64(club + 0x130)?;
        ensure!(pointer(wrapper), "스폰서 목록 없음");
        let header = p.read(wrapper, 24)?;
        let (begin, end, capacity) = (q(&header, 0), q(&header, 8), q(&header, 16));
        ensure!(
            pointer(begin)
                && begin <= end
                && end <= capacity
                && end - begin <= 2048
                && (end - begin) % 8 == 0,
            "스폰서 목록 범위 오류"
        );
        if begin == end {
            return Ok(Vec::new());
        }
        let slots = p.read(begin, (end - begin) as usize)?;
        let mut fields = Vec::new();
        for (n, slot) in slots.chunks_exact(8).enumerate() {
            let address = q(slot, 0);
            if !pointer(address) {
                continue;
            }
            let raw = p.read(address, 24)?;
            let total = i32::from_le_bytes(raw[8..12].try_into().unwrap());
            let annual = i32::from_le_bytes(raw[12..16].try_into().unwrap());
            if annual < 0 || total < annual {
                continue;
            }
            for (suffix, label, offset) in
                [("annual", "연간 후원금", 12), ("total", "계약 총액", 8)]
            {
                fields.push(scalar(p,&format!("sponsor.{n}.{suffix}"),&format!("계약 {} {label} (£)",n+1),"스폰서",
                    address+offset,Storage::I32,0.0,i32::MAX as f64,"기존 스폰서 계약 금액 · 읽기 전용. 회사명과 계약 종류는 아직 해석하지 않습니다.",false)?);
            }
        }
        Ok(fields)
    })();
    if let Ok(fields) = sponsors {
        out.fields.extend(fields);
    }
    if !legacy {
        out.notes.push("FM27: 검증된 잔고·이적·급료 예산, 명성, 홈 구장, 연고 도시를 조회합니다. 구단주 지원 유형·후원 계약은 구조 검증 전까지 표시하지 않습니다.".into());
    }
    out.notes.push(
        "금액은 원화가 아닌 GBP입니다. FM 화면의 표시 통화 환율을 추정해 변환하지 않습니다.".into(),
    );
    if legacy { out.notes.push("기존 후원 계약의 금액·기간과 도시 개발은 이벤트 생성기에서 함께 검토할 수 있습니다. 훈련·유소년 시설은 아직 편집하지 않습니다.".into()); }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_pointer_bounds() {
        assert!(!pointer(0));
        assert!(!pointer(0xffff));
        assert!(pointer(0x10000));
        assert!(!pointer(0x8000_0000_0000));
    }
    #[test]
    fn little_endian_record_header() {
        let raw = [0x34, 0x12, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0x75, 4, 0, 0];
        assert_eq!(q(&raw, 0), 0x1234);
        assert_eq!(d(&raw, 8), 9);
        assert_eq!(d(&raw, 12), 1141);
    }
    #[test]
    fn fm27_finances_use_verified_offsets_including_native_cash_getter() {
        let old = finance_field_specs(true);
        let new = finance_field_specs(false);
        assert_eq!(old.len(), 6);
        assert_eq!(new.len(), 6);
        assert!(old.iter().any(|(key, _, offset, _)| *key == "club.cash" && *offset == 0x14));
        assert!(new.iter().any(|(key, _, offset, _)| *key == "club.cash" && *offset == 0x0c));
        for &(key, _, offset, _) in new {
            let previous = old.iter().find(|field| field.0 == key).unwrap();
            assert_eq!(offset + 8, previous.2, "{key}");
        }
        assert_eq!(new.iter().find(|f| f.0 == "club.weekly_wage").unwrap().2, 0x808);
    }
}
