//! Person social relations. Inline16-byte layout and tags corroborated by
//! reference/relations-readonly-sample.json and FMRTE26.3.2 relation enums.
//! Registration/homegrown/country rivalry are separate system/owner concepts.
use super::*;

pub const SOCIAL_KINDS: &[(u8, &str)] = &[(1,"좋아하는 인물"),(2,"싫어하는 인물"),(3,"좋아하는 구단"),(4,"싫어하는 구단")];
pub fn social_reason_label(kind:u8, reason:u16)->&'static str {
    if kind==3 { return favourite_reason(reason); }
    if kind!=1 { return "없음"; }
    match reason {0=>"없음",1=>"부모",2=>"형제자매",3=>"자녀",4=>"친척",5=>"동료",6=>"감독",7=>"선수",8=>"스태프",9=>"우상",10=>"친구",11=>"유소년 스태프",_=>"미확인"}
}
fn object_type(kind:u8)->Result<u8> { match kind {1|2=>Ok(3),3|4=>Ok(1),_=>bail!("인물의 선호·비선호 관계 종류가 아닙니다")} }
fn valid_reason(kind:u8, reason:u16)->bool {match kind {1=>reason<=11,3=>reason<=4,2|4=>reason==0,_=>false}}
pub fn social_editable(row:&RelationRow)->bool {
    let Ok(typ)=object_type(row.kind) else {return false};
    row.raw.len()==16 && row.raw[10]==typ && row.object_type==typ
        && row.raw[11]==row.kind && row.raw[12]==row.level && row.level<=100
        && row.reason==u16::from_le_bytes([row.raw[8],row.raw[9]]) && valid_reason(row.kind,row.reason)
        && row.target.as_ref().is_some_and(|t|t.uid>0 && t.object_type==typ && t.address==q(&row.raw,0))
}
pub(super) fn target(p:&Process,s:&Session,entity:&Entity,kind:u8,guards:&mut Vec<Guard>)->Result<LinkedRecord> {
    let typ=object_type(kind)?;
    ensure!(if typ==1 {entity.kind==EntityKind::Club} else {matches!(entity.kind,EntityKind::Player|EntityKind::Staff|EntityKind::Person)},"관계 종류에 맞는 대상을 선택하세요");
    guards.extend(backend::detail(s,entity)?.guards);
    let out=linked(p,s,entity.address,typ,guards)?;
    ensure!(out.uid==entity.id,"관계 대상 ID가 변경됐습니다");
    Ok(out)
}
fn record(old:Option<&RelationRow>,target:&LinkedRecord,kind:u8,reason:u16,level:u8)->Result<Vec<u8>> {
    let typ=object_type(kind)?;
    ensure!(target.object_type==typ && pointer(target.address) && valid_reason(kind,reason) && level<=100,"관계 대상·이유·강도 범위를 확인하세요");
    let mut raw=if let Some(row)=old {
        ensure!(social_editable(row),"검증된 선호·비선호 관계가 아닙니다");
        ensure!(row.object_type==typ,"인물 관계와 구단 관계는 각각 추가해 주세요");
        row.raw.clone()
    } else {
        // FM's native relation editor (RVA 0x2CF7640) initializes types 1–4
        // with [permanence=0, opaque14=0, opaque15=255]. Byte13=79 denotes
        // permanent; 78 in some old specimens is not a type-specific default.
        // See reference/fm-native-lifecycle/social-record-audit/evidence.json.
        let mut v=vec![0;16];v[15]=255;v
    };
    raw[..8].copy_from_slice(&target.address.to_le_bytes());
    raw[8..10].copy_from_slice(&reason.to_le_bytes());raw[10]=typ;raw[11]=kind;raw[12]=level;
    Ok(raw)
}
pub(super) fn prepare(snapshot:&PersonRelations,request:&RelationRequest,target:Option<&LinkedRecord>,growth:&mut Option<RelationVectorGrowth>)->Result<(String,String,String,Vec<Change>)> {
    let (slot,kind,entity,reason,level)=match request {
        RelationRequest::AddSocialRelation{kind,target,reason,level}=>(None,*kind,target,*reason,*level),
        RelationRequest::SetSocialRelation{slot,kind,target,reason,level}=>(Some(*slot),*kind,target,*reason,*level),
        RelationRequest::DeleteSocialRelation{slot}=>{
            let old=row(snapshot,*slot)?;
            ensure!(social_editable(old),"이 항목은 인물의 선호·비선호 관계가 아닙니다");
            let raw=snapshot.rows.iter().flat_map(|r|r.raw.iter().copied()).collect::<Vec<_>>();
            return Ok((format!("{} 삭제",kind_label(old.kind)),old.target.as_ref().unwrap().name.clone(),"목록에서 삭제 · 다른 관계 유지".into(),delete_changes(snapshot.holder,snapshot.begin,snapshot.end,16,*slot,&raw)?));
        },
        _=>bail!("선호·비선호 관계 작업이 아닙니다")
    };
    let target=target.context("관계 대상 정보가 없습니다")?;
    let typ=object_type(kind)?;
    ensure!(target.uid==entity.id && target.address==entity.address && target.object_type==typ,"관계 대상 연결이 다릅니다");
    ensure!(if typ==1 {entity.kind==EntityKind::Club} else {matches!(entity.kind,EntityKind::Player|EntityKind::Staff|EntityKind::Person) && entity.id!=snapshot.entity.id},"관계 대상 종류를 확인하세요. 자신을 대상으로 추가할 수 없습니다");
    check_duplicate(snapshot,target.address,kind,slot)?;
    let old=slot.map(|s|row(snapshot,s)).transpose()?;
    let raw=record(old,target,kind,reason,level)?;
    let before=old.map(|r|format!("{} · {} · {}",kind_label(r.kind),r.target.as_ref().map(|t|t.name.as_str()).unwrap_or("미확인"),r.level)).unwrap_or_else(||format!("기존 {}개 관계 유지",snapshot.rows.len()));
    let changes=if let Some(old)=old {vec![change("relation_replace","선호·비선호 관계",snapshot.begin+old.slot as u64*16,old.raw.clone(),raw)?]}else {append_change(snapshot,raw,growth)?};
    Ok((format!("{} {}",kind_label(kind),if slot.is_some(){"수정"}else{"추가"}),before,format!("{} · {} · 강도 {}",target.name,social_reason_label(kind,reason),level),changes))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn social_record_preserves_opaque_bytes_and_validates_target_kind() {
        let (_,_,s)=fixtures::data();let club=s[0].rows[0].target.clone().unwrap();
        let p=LinkedRecord{uid:17,name:"합성 코치".into(),address:0x30000,object_type:3};
        for (kind,t,reason) in [(1,&p,9),(2,&p,0),(3,&club,1),(4,&club,0)] {
            let raw=record(None,t,kind,reason,85).unwrap();
            assert_eq!(&raw[13..], &[0,0,255], "new relation must use native editor defaults");
            let mut row=RelationRow{slot:0,kind,kind_label:kind_label(kind).into(),object_type:t.object_type,reason,level:85,target:Some(t.clone()),editable_favourite:false,editable_secondary_nationality:false,raw};
            assert!(social_editable(&row));row.raw[13..].copy_from_slice(&[79,0,255]);
            let edited=record(Some(&row),t,kind,reason,90).unwrap();assert_eq!(edited[13..],row.raw[13..]);
            assert!(record(Some(&row),t,kind,reason,101).is_err());
        }
        assert!(record(None,&p,3,1,90).is_err());assert!(record(None,&club,4,1,90).is_err());
        assert!(record(None,&p,1,12,90).is_err());assert!(record(None,&p,25,0,90).is_err());
    }
}
