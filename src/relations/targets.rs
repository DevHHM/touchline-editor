//! Typed career target catalogs for the build gated by backend::validate_with.
//! Evidence: output/career_catalog_20261028/global-catalog-probe.json. These are
//! native collection globals, not offsets inside the person-only registry.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CareerTargetRef { pub object_type: u8, pub uid: u32 }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CareerTargetOption { pub object_type: u8, pub uid: u32, pub name: String }

fn specification(object_type: u8) -> Result<(u64, &'static str, &'static [u64])> {
    Ok(match object_type {
        3 => (0x4E49588, ".?AVCLUB@db@@", &[0xC0]),
        5 => (0x4E49598, ".?AVCONTINENT@db@@", &[0x18]),
        9 => (0x4E495A0, ".?AVNATION@db@@", &[0x20]),
        25 => (0x4E49590, ".?AVCOMP@db@@", &[0x40, 0x48]),
        _ => bail!("지원하는 커리어 대상은 구단·대륙·국가·대회입니다"),
    })
}
fn bounds(raw: &[u8]) -> Result<(u64, u64)> {
    ensure!(raw.len() == 16, "대상 목록 헤더 크기가 다릅니다");
    let (begin, end) = (q(raw, 0), q(raw, 8));
    ensure!(pointer(begin) && end >= begin && end - begin <= 1_600_000
        && (end - begin) % 8 == 0, "대상 목록 범위 검증 실패");
    Ok((begin, end))
}
struct Catalog { begin: u64, pointers: Vec<u8>, guards: Vec<Guard> }
fn catalog(p: &Process, session: &Session, object_type: u8) -> Result<Catalog> {
    crate::compatibility::require_legacy_feature(session, "커리어 대상 검색")?;
    let (rva, _, _) = specification(object_type)?;
    let mut guards = vec![];
    let collection = q(&guard(p, &mut guards, session.module_base + rva, 8)?, 0);
    ensure!(pointer(collection), "대상 컬렉션이 아직 로드되지 않았습니다");
    let mode = guard(p, &mut guards, session.module_base + 0x4E49C70, 1)?[0];
    ensure!(mode <= 1, "대상 컬렉션 모드가 다릅니다");
    let holder = if mode == 0 { collection + 0x60 } else {
        q(&guard(p, &mut guards, collection + 0x80, 8)?, 0)
    };
    ensure!(pointer(holder), "대상 목록 포인터 검증 실패");
    let (begin, end) = bounds(&guard(p, &mut guards, holder, 16)?)?;
    let pointers = p.read(begin, (end - begin) as usize)?;
    Ok(Catalog { begin, pointers, guards })
}
fn name(p: &impl RelationMemory, address: u64, offsets: &[u64], guards: &mut Vec<Guard>) -> Result<String> {
    for offset in offsets {
        let text = q(&guard(p, guards, address + offset, 8)?, 0);
        if text == 0 { continue; }
        ensure!(pointer(text), "대상 이름 포인터 검증 실패");
        let raw = p.read(text, 4 + 256)?;
        let end = raw[4..].iter().position(|&b| b == 0).context("대상 이름이 너무 깁니다")?;
        let value = std::str::from_utf8(&raw[4..4 + end]).context("대상 이름 UTF-8 검증 실패")?;
        ensure!(!value.chars().any(char::is_control), "대상 이름에 제어 문자가 있습니다");
        guards.push(Guard { address: text, expected: raw[..4 + end + 1].to_vec() });
        if !value.trim().is_empty() { return Ok(value.to_owned()); }
    }
    bail!("대상 이름을 확인할 수 없습니다")
}
fn validate_object(raw: &[u8], slot: usize, expected_uid: Option<u32>) -> Result<u32> {
    ensure!(raw.len() == 16, "대상 객체 헤더 크기가 다릅니다");
    let index = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    let uid = u32::from_le_bytes(raw[12..16].try_into().unwrap());
    ensure!(index as usize == slot && uid > 0 && expected_uid.is_none_or(|id| id == uid),
        "대상 인덱스 또는 ID가 변경됐습니다");
    Ok(uid)
}

/// Fresh bounded search. Only semantic IDs/names escape to the UI; a selection
/// is resolved again and fully guarded when the user prepares a mutation.
pub fn search(session: &Session, object_type: u8, query: &str, limit: usize) -> Result<Vec<CareerTargetOption>> {
    ensure!((1..=200).contains(&limit) && query.chars().count() <= 128, "대상 검색 범위가 다릅니다");
    let (_, expected_type, offsets) = specification(object_type)?;
    let p = Process::open(session.pid, false)?;
    backend::validate_with(&p, session)?;
    let catalog = catalog(&p, session, object_type)?;
    let needle = query.trim().to_lowercase();
    let exact_uid = needle.parse::<u32>().ok();
    let mut types = HashMap::new();
    let mut result = Vec::new();
    for (slot, bytes) in catalog.pointers.chunks_exact(8).enumerate() {
        let address = q(bytes, 0);
        if !pointer(address) { continue; }
        let Ok(raw) = p.read(address, 16) else { continue; };
        let Ok(uid) = validate_object(&raw, slot, None) else { continue; };
        if exact_uid.is_some_and(|expected| expected != uid) { continue; }
        let vt = q(&raw, 0);
        let typed = *types.entry(vt).or_insert_with(|| backend::type_name(&p, session, vt) == expected_type);
        if !typed { continue; }
        let mut checked = vec![];
        let Ok(value) = name(&p, address, offsets, &mut checked) else { continue; };
        if needle.is_empty() || exact_uid == Some(uid) || value.to_lowercase().contains(&needle) {
            // Verify just returned rows; do not retain address caches between queries.
            ensure!(p.u64(catalog.begin + slot as u64 * 8)? == address && p.read(address, 16)? == raw,
                "검색 중 대상이 변경됐습니다. 다시 검색하세요");
            validate_guards(&p, &checked)?;
            result.push(CareerTargetOption { object_type, uid, name: value });
        }
    }
    backend::validate_with(&p, session)?;
    validate_guards(&p, &catalog.guards)?;
    result.sort_by(|a,b|a.name.cmp(&b.name).then(a.uid.cmp(&b.uid)));
    result.dedup_by_key(|entry| entry.uid);
    result.truncate(limit);
    Ok(result)
}

pub fn resolve(session: &Session, object_type: u8, uid: u32, guards: &mut Vec<Guard>) -> Result<LinkedRecord> {
    ensure!(uid > 0, "대상 ID를 선택하세요");
    let (_, expected_type, offsets) = specification(object_type)?;
    let p = Process::open(session.pid, false)?;
    backend::validate_with(&p, session)?;
    let catalog = catalog(&p, session, object_type)?;
    let mut found = None;
    for (slot, bytes) in catalog.pointers.chunks_exact(8).enumerate() {
        let address = q(bytes, 0);
        if !pointer(address) { continue; }
        let Ok(raw) = p.read(address, 16) else { continue; };
        if u32::from_le_bytes(raw[12..16].try_into().unwrap()) != uid { continue; }
        validate_object(&raw, slot, Some(uid))?;
        ensure!(found.is_none(), "같은 종류의 대상 ID가 중복되어 선택할 수 없습니다");
        ensure!(backend::type_name(&p, session, q(&raw, 0)) == expected_type, "대상 객체 형식이 다릅니다");
        let mut checked = vec![];
        let value = name(&p, address, offsets, &mut checked)?;
        checked.push(Guard { address: catalog.begin + slot as u64 * 8, expected: bytes.to_vec() });
        checked.push(Guard { address, expected: raw });
        found = Some((LinkedRecord { object_type, uid, address, name: value }, checked));
    }
    let (record, mut checked) = found.context("현재 세이브의 대상 목록에서 선택한 ID를 찾지 못했습니다")?;
    checked.extend(catalog.guards);
    backend::validate_with(&p, session)?;
    validate_guards(&p, &checked)?;
    guards.extend(checked);
    Ok(record)
}
pub fn resolve_ref(session: &Session, target: &CareerTargetRef, guards: &mut Vec<Guard>) -> Result<LinkedRecord> {
    resolve(session, target.object_type, target.uid, guards)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_bounds_and_target_types_fail_closed() {
        assert!(bounds(&[0; 16]).is_err());
        for (begin, end) in [(0x10000u64, 0xffffu64), (0x10000, 0x10009), (0x10000, 0x200000)] {
            assert!(bounds(&[begin.to_le_bytes(), end.to_le_bytes()].concat()).is_err());
        }
        assert!(bounds(&[0x10000u64.to_le_bytes(), 0x10010u64.to_le_bytes()].concat()).is_ok());
        assert!(specification(8).is_err()); // Relation Nation=8 is NOT career Nation=9.
        assert_eq!(specification(5).unwrap().2, &[0x18]);
        assert_eq!(specification(9).unwrap().2, &[0x20]);
        assert_eq!(specification(25).unwrap().0, 0x4E49590);
    }
    #[test]
    fn catalog_identity_rejects_stale_slot_uid_and_truncation() {
        let raw = [0x11000u64.to_le_bytes().to_vec(), 4u32.to_le_bytes().to_vec(), 11u32.to_le_bytes().to_vec()].concat();
        assert_eq!(validate_object(&raw, 4, Some(11)).unwrap(), 11);
        assert!(validate_object(&raw, 3, Some(11)).is_err());
        assert!(validate_object(&raw, 4, Some(12)).is_err());
        assert!(validate_object(&raw[..15], 4, None).is_err());
    }
    #[test]
    #[ignore = "requires a running supported FM26 save; catalog reads only, never applies changes"]
    fn live_readonly_catalogs() {
        let backend::watch::Observation::Ready(ready) = backend::watch::probe(None) else {
            panic!("No stable supported FM26 save is ready")
        };
        let i = ready.identity;
        let session = Session { pid:i.pid,created:i.created,module_base:i.module_base,
            module_size:i.module_size,module_stamp:i.module_stamp,db:i.db,
            registry_begin:i.registry_begin,game_date:ready.game_date,
            profile:backend::PROFILE.into(),own_club_id:None };
        for (object_type, query, limit) in [(3,"1141",10),(5,"",20),(9,"",200),(25,"11",10)] {
            let started = std::time::Instant::now();
            let found = search(&session,object_type,query,limit).unwrap();
            assert!(!found.is_empty(), "Empty native catalog {object_type}");
            let selected = &found[0]; let mut guards = vec![];
            let resolved = resolve(&session,object_type,selected.uid,&mut guards).unwrap();
            assert_eq!(resolved.name,selected.name);assert_eq!(resolved.object_type,object_type);
            assert!(!guards.is_empty());
            eprintln!("{}",serde_json::json!({"read_only":true,"game_values_changed":false,
                "object_type":object_type,"returned":found.len(),"sample":selected,
                "guards":guards.len(),"elapsed_ms":started.elapsed().as_millis()}));
        }
    }
}
