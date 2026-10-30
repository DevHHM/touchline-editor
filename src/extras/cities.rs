//! Full CITY/NATION table routing for a fingerprint-verified build profile.
//! Evidence: output/italy_city_attraction_20261028/field_evidence.json.
//! Attraction is one byte: +0x55 is a separate value and must be preserved.
use super::*;
use std::collections::{HashMap, HashSet};

const CITY_ATTRACTION: u64 = 0x54;
const CITY_ATTRACTION_STORAGE: Storage = Storage::U8;

struct Table {
    begin: u64,
    count: usize,
    slots: Vec<u8>,
    guards: Vec<Guard>,
}
fn table(
    p: &Process,
    s: &Session,
    rva: u64,
    kind: &str,
    max: usize,
    include_slots: bool,
) -> Result<Table> {
    let mut guards = Vec::new();
    let address = q(&guard(p, &mut guards, s.module_base + rva, 8)?, 0);
    expect_type(
        p,
        s,
        address,
        &format!(".?AV?$DATABASE_TABLE@V{kind}@db@@@db@@"),
    )?;
    guard(p, &mut guards, address, 8)?;
    let layout = crate::backend::profiles::profile_for_session(s)
        .context("도시 목록의 FM 빌드가 확인되지 않았습니다")?.require_read_layout()?;
    let mode = guard(p, &mut guards, s.module_base + layout.registry_mode_rva, 1)?[0];
    ensure!(mode <= 1, "도시 데이터베이스 모드 오류");
    let vector = if mode == 0 {
        address + 0x60
    } else {
        q(&guard(p, &mut guards, address + 0x80, 8)?, 0)
    };
    ensure!(pointer(vector), "도시 데이터베이스 목록 주소 오류");
    let header = guard(p, &mut guards, vector, 24)?;
    let (begin, end, capacity) = (q(&header, 0), q(&header, 8), q(&header, 16));
    ensure!(
        pointer(begin)
            && begin < end
            && end <= capacity
            && (end - begin) % 8 == 0
            && (end - begin) / 8 <= max as u64
            && capacity - begin <= (max as u64) * 8,
        "도시 데이터베이스 목록 범위 오류"
    );
    let slots = if include_slots {
        p.read(begin, (end - begin) as usize)?
    } else {
        Vec::new()
    };
    let mut seen = HashSet::new();
    ensure!(
        slots
            .chunks_exact(8)
            .all(|v| pointer(q(v, 0)) && seen.insert(q(v, 0))),
        "도시 데이터베이스 중복/무효 항목"
    );
    Ok(Table {
        begin,
        count: ((end - begin) / 8) as usize,
        slots,
        guards,
    })
}

fn city_values(raw: &[u8]) -> Result<(u32, u32)> {
    ensure!(raw.len() == 0x58, "도시 레코드 크기 오류");
    let lat = f32::from_le_bytes(raw[0x48..0x4c].try_into().unwrap());
    let lon = f32::from_le_bytes(raw[0x4c..0x50].try_into().unwrap());
    ensure!(
        d(raw, 12) > 0
            && lat.is_finite()
            && lon.is_finite()
            && (-90.0..=90.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon)
            && raw[0x52] <= 15
            && raw[0x54] <= 20,
        "도시 레코드 값 오류"
    );
    Ok((d(raw, 8), d(raw, 12)))
}

pub(super) fn enumerate(
    p: &Process,
    s: &Session,
    linked: &HashMap<u32, Entity>,
) -> Result<Vec<Entity>> {
    let layout = crate::backend::profiles::profile_for_session(s)
        .context("도시 목록의 FM 빌드가 확인되지 않았습니다")?.require_read_layout()?;
    let nt = table(p, s, layout.nation_database_rva, "NATION", 4096, true)?;
    let mut nations = HashMap::new();
    for (i, slot) in nt.slots.chunks_exact(8).enumerate() {
        let address = q(slot, 0);
        expect_type(p, s, address, ".?AVNATION@db@@")?;
        let raw = p.read(address, 0x20)?;
        ensure!(
            d(&raw, 8) == i as u32 && d(&raw, 12) > 0,
            "국가 레코드 식별자 오류"
        );
        nations.insert(address, text(p, address + 0x18)?);
    }
    let ct = table(p, s, layout.city_database_rva, "CITY", 150_000, true)?;
    let mut rows = Vec::with_capacity(ct.count);
    let mut seen = HashSet::new();
    let mut vtables = HashSet::new();
    for (i, slot) in ct.slots.chunks_exact(8).enumerate() {
        let address = q(slot, 0);
        let raw = p.read(address, 0x58)?;
        if vtables.insert(q(&raw, 0)) {
            expect_type(p, s, address, ".?AVCITY@db@@")?;
        }
        let (index, id) = city_values(&raw)?;
        ensure!(
            index == i as u32 && seen.insert(id),
            "도시 레코드 식별자 오류"
        );
        let nation = q(&raw, 0x20);
        ensure!(
            nation == 0 || nations.contains_key(&nation),
            "도시의 소속 국가가 목록과 일치하지 않습니다"
        );
        let country = nations
            .get(&nation)
            .cloned()
            .unwrap_or_else(|| "국가 미확인".into());
        let name = text(p, address + 0x18).unwrap_or_else(|_| format!("도시 #{id}"));
        let source = linked.get(&id).filter(|e| e.address == address);
        rows.push(Entity {
            nationality: String::new(),
            nationality_id: None,
            id,
            kind: EntityKind::City,
            english_name: name.clone(),
            name,
            club: source.map(|e| e.club.clone()).unwrap_or_default(),
            club_id: source.and_then(|e| e.club_id),
            age: None,
            ca: None,
            pa: None,
            role: "도시".into(),
            group: country,
            football_category: crate::model::FootballCategory::Unknown,
            // Native city identity does not depend on an arbitrary source club.
            address,
            base: 0,
            index: Some(index),
        });
    }
    for g in ct.guards.iter().chain(nt.guards.iter()) {
        ensure!(
            p.read(g.address, g.expected.len())? == g.expected,
            "도시 목록을 읽는 중 세이브가 변경되었습니다"
        );
    }
    ensure!(
        p.read(ct.begin, ct.slots.len())? == ct.slots,
        "도시 목록을 읽는 중 항목이 변경되었습니다"
    );
    Ok(rows)
}

pub(super) fn detail(p: &Process, s: &Session, e: &Entity) -> Result<Detail> {
    let layout = crate::backend::profiles::profile_for_session(s)
        .context("도시 정보의 FM 빌드가 확인되지 않았습니다")?.require_read_layout()?;
    let ct = table(p, s, layout.city_database_rva, "CITY", 150_000, false)?;
    let index = e
        .index
        .context("도시 인덱스가 없습니다. 다시 연결해 주세요")? as usize;
    ensure!(
        index < ct.count && p.u64(ct.begin + (index * 8) as u64)? == e.address,
        "도시 데이터베이스 항목이 변경되었습니다"
    );
    expect_type(p, s, e.address, ".?AVCITY@db@@")?;
    let mut out = Detail {
        entity: e.clone(),
        fields: Vec::new(),
        guards: ct.guards,
        notes: Vec::new(),
    };
    // Event contexts (and older snapshots) retain an explicit club route. Keep
    // its identity guards so a club's stadium move cannot redirect an event.
    if e.base != 0 {
        expect_type(p, s, e.base, ".?AVCLUB@db@@")?;
        let club_head = guard(p, &mut out.guards, e.base, 16)?;
        ensure!(
            Some(d(&club_head, 12)) == e.club_id,
            "도시를 연결한 구단이 변경되었습니다"
        );
        let route = first_team(p, e.base)?;
        out.guards.extend(route.guards);
        let stadium = q(&guard(p, &mut out.guards, route.team + 0x78, 8)?, 0);
        expect_type(p, s, stadium, ".?AVSTADIUM@db@@")?;
        guard(p, &mut out.guards, stadium, 16)?;
        let city = q(&guard(p, &mut out.guards, stadium + 0x60, 8)?, 0);
        ensure!(city == e.address, "구단 홈 구장의 도시가 변경되었습니다");
    }
    guard(p, &mut out.guards, ct.begin + (index * 8) as u64, 8)?;
    let raw = p.read(e.address, 0x58)?;
    let (current_index, id) = city_values(&raw)?;
    ensure!(
        id == e.id && current_index == index as u32,
        "도시 식별자 불일치"
    );
    guard(p, &mut out.guards, e.address, 16)?;
    guard(p, &mut out.guards, e.address + 0x18, 16)?;
    guard(p, &mut out.guards, e.address + 0x48, 8)?;
    // +55 is not part of the attraction. Guard it so adjacent data cannot drift
    // unnoticed while a reviewed scalar edit is pending.
    guard(p, &mut out.guards, e.address + 0x55, 3)?;
    let nation = q(&raw, 0x20);
    if nation != 0 {
        let nt = table(p, s, layout.nation_database_rva, "NATION", 4096, false)?;
        let slot = p.u32(nation + 8)? as usize;
        ensure!(
            slot < nt.count && p.u64(nt.begin + (slot * 8) as u64)? == nation,
            "도시의 국가 항목을 찾지 못했습니다"
        );
        expect_type(p, s, nation, ".?AVNATION@db@@")?;
        out.guards.extend(nt.guards);
        guard(p, &mut out.guards, nt.begin + (slot * 8) as u64, 8)?;
        guard(p, &mut out.guards, nation, 0x20)?;
        let name = text(p, nation + 0x18)?;
        out.entity.group = name.clone();
        out.fields.push(scalar(
            p,
            "city.nation_id",
            "소속 국가 ID",
            "위치",
            nation + 12,
            Storage::U32,
            1.0,
            u32::MAX as f64,
            &name,
            false,
        )?);
        out.notes.push(format!("소속 국가: {name}"));
    } else {
        out.entity.group = "국가 미확인".into();
        out.notes
            .push("이 도시는 데이터베이스에 소속 국가가 지정되어 있지 않습니다.".into());
    }
    out.fields.push(scalar(
        p,
        "city.population_band",
        "인구 등급",
        "도시",
        e.address + 0x52,
        Storage::U8,
        0.0,
        15.0,
        "정확한 인구수가 아닌 FM의 인구 구간입니다. 7: 50,001–100,000명 · 8: 100,001–250,000명.",
        true,
    )?);
    out.fields.push(scalar(
        p,
        "city.attraction",
        "도시 매력도",
        "도시",
        e.address + CITY_ATTRACTION,
        CITY_ATTRACTION_STORAGE,
        0.0,
        20.0,
        "0–20. 도시 전체에 적용되므로 같은 도시를 연고로 둔 다른 구단도 영향을 받습니다.",
        true,
    )?);
    out.fields.push(scalar(
        p,
        "city.latitude",
        "위도",
        "위치",
        e.address + 0x48,
        Storage::F32,
        -90.0,
        90.0,
        "도시 좌표 · 읽기 전용",
        false,
    )?);
    out.fields.push(scalar(
        p,
        "city.longitude",
        "경도",
        "위치",
        e.address + 0x4c,
        Storage::F32,
        -180.0,
        180.0,
        "도시 좌표 · 읽기 전용",
        false,
    )?);
    out.notes.push("도시 부유도는 검증된 독립 필드를 찾지 못해 편집하지 않습니다. 인구 등급은 실제 인구수와 구분됩니다.".into());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn city_attraction_is_one_byte_and_preserves_adjacent_value() {
        let mut raw = vec![0; 0x58];
        raw[12] = 1;
        raw[0x54] = 8;
        raw[0x55] = 2;
        assert!(city_values(&raw).is_ok());
        let f = Field {
            key: "city.attraction".into(),
            label: "도시 매력도".into(),
            section: "도시".into(),
            value: 8.0,
            min: 0.0,
            max: 20.0,
            step: 1.0,
            hint: String::new(),
            binding: Some(Binding {
                address: 0x10000 + CITY_ATTRACTION,
                storage: CITY_ATTRACTION_STORAGE,
                scale: 1.0,
                original: vec![8],
            }),
        };
        let encoded = crate::transaction::encode_value(&f, 10.0).unwrap();
        assert_eq!(encoded, vec![10]);
        raw[0x54..0x54 + encoded.len()].copy_from_slice(&encoded);
        assert_eq!(&raw[0x54..0x58], &[10, 2, 0, 0]);
        assert!(crate::transaction::encode_value(&f, 21.0).is_err());
    }
    #[test]
    fn city_record_rejects_out_of_range_scalar_or_coordinates() {
        let mut raw = vec![0; 0x58];
        raw[12] = 1;
        assert!(city_values(&raw).is_ok());
        raw[0x54] = 21;
        assert!(city_values(&raw).is_err());
        raw[0x54] = 20;
        raw[0x48..0x4c].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(city_values(&raw).is_err());
    }
}
