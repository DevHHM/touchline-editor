//! Narrow FM27 FULL_CONTRACT date support, independently verified on disk.
//! See reference/contract-dates-20261102.md. No native code is executed here.
use crate::{backend::{self, profiles}, memory::Process, model::*};
use anyhow::{Result, ensure};

const START: &str = "contract.starts";
const END: &str = "contract.expires";
const UNSET: u32 = (1900 << 16) | 1;

// The property branches prove Csda/Ceda semantics, their virtual routes and the
// exact ordinary expiry store. The complete start setter proves why nonempty
// clause vectors cannot be edited as a single scalar.
const SIGNATURES: &[(u64, &[u8])] = &[
 (0x301A50, &[0x48,0x8d,0x41,0x44,0xc3]),
 (0x1A733A8, &[0x81,0xfb,0x61,0x64,0x73,0x43,0x0f,0x85,0x47,0x12,0x00,0x00,0x48,0x8b,0x07,0x48,0x89,0xf9,0xff,0x90,0xe0,0x00,0x00,0x00]),
 (0x1A73116, &[0x81,0xfb,0x61,0x64,0x65,0x43,0x0f,0x85,0xd9,0x14,0x00,0x00,0x48,0x8b,0x07,0x48,0x8d,0x55,0x30,0x48,0x89,0xf9,0xff,0x90,0xe8,0x00,0x00,0x00]),
 (0x234EFA0, &[0x48,0x89,0xd0,0xf6,0x41,0x2b,0x08,0x75,0x06,0x8b,0x49,0x48,0x89,0x08,0xc3]),
 (0x1A7A699, &[0x81,0xfa,0x61,0x64,0x73,0x43,0x0f,0x85,0x5b,0x0a,0x00,0x00]),
 (0x1A7A899, &[0x81,0xfa,0x61,0x64,0x65,0x43,0x0f,0x85,0x5b,0x08,0x00,0x00]),
 (0x1A7B0CF, &[0x8b,0x45,0xf4,0x89,0x41,0x48]),
 (0x1A7B0F0, &[0x48,0x8b,0x01,0x48,0x8d,0x55,0xf4,0xff,0x90,0xd8,0x00,0x00,0x00]),
 (0x23424A0, &[0x41,0x56,0x56,0x57,0x53,0x48,0x83,0xec,0x28,0x48,0x89,0xd6,0x48,0x8d,0x79,0x44,0x66,0x81,0x79,0x46,0x6c,0x07,0x75,0x1d,0x0f,0xb7,0x41,0x44,0x25,0xff,0x01,0x00,0x00,0x66,0x83,0xf8,0x01,0x75,0x0e,0x8b,0x06,0x89,0x07,0x48,0x83,0xc4,0x28,0x5b,0x5f,0x5e,0x41,0x5e,0xc3,0x48,0x8b,0x59,0x68,0x4c,0x8b,0x71,0x70,0x4c,0x39,0xf3,0x74,0xe5,0x66,0x2e,0x0f,0x1f,0x84,0x00,0x00,0x00,0x00,0x00,0x0f,0x1f,0x40,0x00,0x48,0x89,0xd9,0x48,0x89,0xfa,0x49,0x89,0xf0,0xe8,0xa2,0xe7,0x73,0xff,0x48,0x83,0xc3,0x08,0x4c,0x39,0xf3,0x75,0xe9,0xeb,0xbe]),
];

fn verify_with(session: &Session, mut read: impl FnMut(u64, usize) -> Result<Vec<u8>>) -> Result<()> {
 ensure!(profiles::profile_for_session(session).is_some_and(|profile| profile.id == profiles::FM27.id), "이 FM 빌드의 계약 날짜는 지원하지 않습니다");
 for &(rva, expected) in SIGNATURES {
  ensure!(read(session.module_base + rva, expected.len())? == expected, "계약 날짜 코드 검증이 일치하지 않습니다");
 }
 for (slot,target) in [(0xD8,0x23424A0u64),(0xE0,0x301A50),(0xE8,0x234EFA0)] {
  ensure!(read(session.module_base+0x4B00AE8+slot,8)?==(session.module_base+target).to_le_bytes(),"계약 날짜 접근 경로가 일치하지 않습니다");
 }
 Ok(())
}

fn date_field(key: &str, label: &str, address: u64, raw: &[u8], hint: &str) -> Option<Field> {
 let packed = u32::from_le_bytes(raw.try_into().ok()?);
 // 1900-01-01 is the native absent-date sentinel, not a real contract period.
 if packed & !0xFE00 == UNSET { return None; }
 let value = super::decode_date_value(packed).ok()?;
 Some(Field { key:key.into(), label:label.into(), section:"계약".into(), value,
  min:19_000_102., max:21_001_231., step:1., hint:hint.into(),
  binding:Some(Binding {address,storage:Storage::FmDate,scale:1.,original:raw.to_vec()}) })
}

fn from_record(contract: u64, record: &[u8]) -> Result<(Vec<Field>, Vec<Guard>, Vec<String>)> {
 ensure!(record.len() == 0x80, "계약 날짜 기록 크기가 올바르지 않습니다");
 let mut fields = Vec::new(); let mut notes = Vec::new();
 let mut start = date_field(START, "계약 시작일 (YYYYMMDD)", contract+0x44, &record[0x44..0x48],
  "기존 계약 시작일입니다. 날짜에 연동된 계약 항목이 없는 경우에만 변경합니다. 새 계약·갱신·이적을 만들지 않습니다.");
 let end = if record[0x2B] & 8 == 0 {
  date_field(END, "계약 만료일 (YYYYMMDD)", contract+0x48, &record[0x48..0x4C],
   "기존 고정 계약 만료일만 변경합니다. 갱신 합의·약속·옵션·보너스를 새로 만들지 않습니다.")
 } else {
  notes.push("이 계약의 만료일은 별도 규칙으로 계산되어 날짜 조회·편집을 제공하지 않습니다.".into()); None
 };
 let begin = u64::from_le_bytes(record[0x68..0x70].try_into().unwrap());
 let finish = u64::from_le_bytes(record[0x70..0x78].try_into().unwrap());
 if let Some(field) = &mut start {
  if begin != finish || end.is_none() {
   field.binding = None;
   field.hint = if begin != finish {"읽기 전용: 시작일에 연동된 계약 항목이 있어 개별 날짜만 변경할 수 없습니다."}
    else {"읽기 전용: 유효한 고정 만료일을 확인할 수 없어 계약 기간을 검증할 수 없습니다."}.into();
  }
 } else { notes.push("유효한 기존 계약 시작일이 없어 표시·편집할 수 없습니다.".into()); }
 if end.is_none() && record[0x2B] & 8 == 0 { notes.push("유효한 기존 계약 만료일이 없어 표시·편집할 수 없습니다.".into()); }
 // Keep all identity, contract flags, links, clause bounds and noneditable date
 // bytes guarded. Editable dates use the normal scalar baseline/dependency path.
 let mut guards = vec![Guard {address:contract,expected:record[..0x44].to_vec()},
  Guard {address:contract+0x4C,expected:record[0x4C..].to_vec()}];
 for (offset, field) in [(0x44, &start), (0x48, &end)] {
  if field.as_ref().is_none_or(|field| field.binding.is_none()) {
   guards.push(Guard {address:contract+offset as u64,expected:record[offset..offset+4].to_vec()});
  }
 }
 fields.extend(start); fields.extend(end);
 Ok((fields,guards,notes))
}

pub(super) fn append(p: &Process, session: &Session, out: &mut Detail) {
 if !matches!(out.entity.kind,EntityKind::Player|EntityKind::Staff) {return;}
 let read = || -> Result<Option<(Vec<Field>,Vec<Guard>,Vec<String>)>> {
  verify_with(session, |address,size|p.read(address,size))?;
  let original = p.read(out.entity.address+0xA8,8)?;
  let contract = u64::from_le_bytes(original.clone().try_into().unwrap());
  if contract == 0 {return Ok(None);}
  ensure!((0x10000..0x7fff_ffff_ff00).contains(&contract), "계약 포인터 범위 오류");
  let record = p.read(contract,0x80)?;
  let vtable = u64::from_le_bytes(record[..8].try_into().unwrap());
  ensure!(vtable == session.module_base+0x4B00AE8 && backend::type_name(p,session,vtable)==".?AVFULL_CONTRACT@db@@", "지원하는 기존 완전 계약이 아닙니다");
  ensure!(u64::from_le_bytes(record[8..16].try_into().unwrap())==out.entity.address, "계약의 인물 연결이 일치하지 않습니다");
  let (fields,mut guards,notes)=from_record(contract,&record)?;
  guards.push(Guard {address:out.entity.address+0xA8,expected:original});
  Ok(Some((fields,guards,notes)))
 };
 match read() {
  Ok(Some((fields,guards,notes)))=>{out.fields.extend(fields);out.guards.extend(guards);out.notes.extend(notes);},
  Ok(None)=>out.notes.push("기존 주계약이 없어 계약 시작일·만료일을 편집하지 않습니다.".into()),
  Err(error)=>out.notes.push(format!("계약 날짜 조회·편집을 사용할 수 없습니다: {error}")),
 }
}

pub(super) fn validate_edits(detail: &Detail, edits: &[Edit]) -> Result<()> {
 if !edits.iter().any(|edit|matches!(edit.key.as_str(),START|END)) {return Ok(());}
 let value=|key:&str|edits.iter().find(|edit|edit.key==key).map(|edit|edit.value)
  .or_else(||detail.fields.iter().find(|field|field.key==key).map(|field|field.value));
 if let (Some(start),Some(end))=(value(START),value(END)) {
  ensure!(start<=end,"계약 시작일은 만료일보다 늦을 수 없습니다");
 }
 Ok(())
}

pub(super) fn dependency_guards(detail: &Detail, changes: &[Change]) -> Vec<Guard> {
 if !changes.iter().any(|change|matches!(change.key.as_str(),START|END)) {return vec![];}
 [START,END].iter().filter(|key|!changes.iter().any(|change|change.key==**key))
  .filter_map(|key|detail.fields.iter().find(|field|field.key==*key)?.binding.as_ref())
  .map(|binding|Guard {address:binding.address,expected:binding.original.clone()}).collect()
}

#[cfg(test)]
mod tests {
 use super::*;
 fn record() -> Vec<u8> {
  let mut bytes=vec![0u8;0x80];
  bytes[0x44..0x48].copy_from_slice(&((2026u32<<16)|0xBE00|182).to_le_bytes());
  bytes[0x48..0x4C].copy_from_slice(&((2029u32<<16)|0xA600|181).to_le_bytes()); bytes
 }
 #[test]
 fn fixed_dates_decode_and_edit_without_destroying_flags() {
  let (fields,guards,_)=from_record(0x10000,&record()).unwrap();
  assert_eq!(fields.iter().map(|field|(&*field.key,field.value)).collect::<Vec<_>>(),vec![(START,20260701.),(END,20290630.)]);
  for field in &fields {
   let edited=crate::transaction::encode_value(field,20280229.).unwrap();
   assert_eq!(u32::from_le_bytes(edited.try_into().unwrap())&0xFE00,u32::from_le_bytes(field.binding.as_ref().unwrap().original.clone().try_into().unwrap())&0xFE00);
   assert!(crate::transaction::encode_value(field,20290229.).is_err());
   assert!(!guards.iter().any(|g|g.address<=field.binding.as_ref().unwrap().address&&g.address+g.expected.len() as u64>field.binding.as_ref().unwrap().address));
  }
 }
 #[test]
 fn clause_dependent_starts_and_dynamic_expiry_are_not_scalar_targets() {
  let mut bytes=record();bytes[0x70..0x78].copy_from_slice(&0x80008u64.to_le_bytes());
  let (fields,guards,_)=from_record(0x10000,&bytes).unwrap();
  assert!(fields[0].binding.is_none());assert!(fields[1].binding.is_some());
  assert!(guards.iter().any(|guard|guard.address==0x10044));
  assert!(crate::transaction::encode_value(&fields[0],20250101.).is_err());
  bytes[0x2B]|=8;
  let (fields,_,notes)=from_record(0x10000,&bytes).unwrap();
  assert_eq!(fields.len(),1);assert_eq!(fields[0].key,START);assert!(fields[0].binding.is_none());assert!(!notes.is_empty());
 }
 #[test]
 fn absent_and_invalid_dates_are_not_fabricated() {
  let mut bytes=record();bytes[0x44..0x48].copy_from_slice(&(UNSET|0xBE00).to_le_bytes());bytes[0x48..0x4C].fill(0);
  let (fields,guards,notes)=from_record(0x10000,&bytes).unwrap();assert!(fields.is_empty());assert_eq!(notes.len(),2);
  assert!(guards.iter().any(|guard|guard.address==0x10044));assert!(guards.iter().any(|guard|guard.address==0x10048));
 }
 #[test]
 fn exact_build_and_every_date_signature_are_required() {
  let mut session=Session {pid:0,created:0,module_base:0,module_size:profiles::FM27.image_size,module_stamp:profiles::FM27.timestamp,db:0,registry_begin:0,game_date:String::new(),profile:profiles::FM27.label.into(),own_club_id:None};
  for changed in 0..SIGNATURES.len() {
   assert!(verify_with(&session,|address,_|{let n=SIGNATURES.iter().position(|(rva,_)|*rva==address).unwrap();let mut bytes=SIGNATURES[n].1.to_vec();if n==changed{bytes[0]^=1;}Ok(bytes)}).is_err());
  }
  let bytes=|address:u64|SIGNATURES.iter().find(|(rva,_)|*rva==address).map(|(_,bytes)|bytes.to_vec()).unwrap_or_else(||
   [(0xD8,0x23424A0u64),(0xE0,0x301A50),(0xE8,0x234EFA0)].iter().find(|(slot,_)|0x4B00AE8+slot==address).unwrap().1.to_le_bytes().to_vec());
  assert!(verify_with(&session,|address,_|Ok(bytes(address))).is_ok());
  assert!(verify_with(&session,|address,_|{let mut value=bytes(address);if address==0x4B00AE8+0xE0 {value[0]^=1;}Ok(value)}).is_err());
  session.module_stamp+=1;assert!(verify_with(&session,|_,_|panic!("unverified build must not read offsets")).is_err());
 }
 #[test]
 fn player_and_staff_drafts_validate_period_and_guard_the_unchanged_date() {
  for kind in [EntityKind::Player,EntityKind::Staff] {
   let (fields,guards,_)=from_record(0x10000,&record()).unwrap();
   let detail=Detail {entity:Entity {id:7,kind,name:"Fixture".into(),english_name:String::new(),nationality:String::new(),nationality_id:None,football_category:FootballCategory::Unknown,club:String::new(),club_id:None,age:None,ca:None,pa:None,role:String::new(),group:String::new(),address:0x20000,base:0x20000,index:None},fields,guards,notes:vec![]};
   let start=Edit {key:START.into(),value:20270101.};
   let changes=crate::transaction::prepare(&detail,&[start.clone()]).unwrap();
   let deps=crate::transaction::dependency_guards(&detail,&changes).unwrap();
   assert!(deps.iter().any(|guard|guard.address==0x10048&&guard.expected==record()[0x48..0x4C]));
   assert!(crate::transaction::prepare(&detail,&[Edit {key:START.into(),value:20300101.}]).is_err());
   assert!(crate::transaction::prepare(&detail,&[Edit {key:END.into(),value:20250101.}]).is_err());
   let edits=[start,Edit {key:END.into(),value:20280101.}];
   let changes=crate::transaction::prepare(&detail,&edits).unwrap();
   assert_eq!(changes.len(),2);assert!(crate::transaction::dependency_guards(&detail,&changes).unwrap().is_empty());
  }
 }
}
