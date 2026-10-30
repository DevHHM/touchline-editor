//! Owned career allocation. Records use FM's native pool, holders its native
//! size-24 context, vectors a verified CRT fallback capacity. No foreign pointer
//! is inserted into the pool. Detached storage is retained across undo.
use super::*;
use crate::native_alloc::{self, NativeAllocation};

const POOL_RVA: u64 = 0x4E4B580;
// Full static inventory: every direct pool reference and generic pool lifecycle
// routine, including teardown tail-call wrappers. Full byte hashes bind the
// no-escape/call-frame analysis to this exact native build.
const POOL_CODE: &[(u64,u64,u64)]=&[
    (0xDC1800,0xDC1ED0,0x1a712f927e6c2fd0),
    (0xF110F0,0xF11100,0xa937dec6484049b4),
    (0x170E9E0,0x17104C7,0xdb308418bf99c572),
    (0x1736B40,0x1736ED3,0x8dd8d990bf4b20d6),
    (0x173FB60,0x173FCC0,0x4bc7e708210e9280),
    (0x3917FD0,0x39192CC,0xb68666f9ae179f88),
    (0x3919B90,0x3919BA0,0xa224cb267cf5596a),
    (0xB82E110,0xB82E1C0,0x04146cf7b4961f20),
];
fn verify_pool_code(p:&impl RelationMemory,base:u64)->Result<()> {
    for (begin,end,expected) in POOL_CODE {
        let bytes=p.read(base+begin,(end-begin) as usize)?;
        let hash=bytes.iter().fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3));
        ensure!(hash==*expected,"커리어 메모리 풀 코드 검증 실패: 0x{begin:X}");
    }
    let epoch=p.read(base+POOL_RVA+0x50,4)?;
    let epoch=i32::from_le_bytes(epoch.try_into().unwrap());
    ensure!(epoch!=0&&epoch!=-1,"FM 커리어 메모리 풀이 아직 초기화 중입니다");
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerGrowth {
    pub holder: u64,
    pub begin: u64,
    pub end: u64,
    pub capacity: u64,
    pub pointers: Vec<u64>,
    pub types: Vec<u8>,
    pub new_record: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerStorage {
    pub record: NativeAllocation,
    pub vector: NativeAllocation,
    pub holder: Option<NativeAllocation>,
    pub original_vector: Option<NativeAllocation>,
}
impl CareerStorage {
    fn new_allocations(&self) -> impl Iterator<Item=&NativeAllocation> {
        [&self.record,&self.vector].into_iter().chain(self.holder.iter())
    }
    fn allocations(&self) -> impl Iterator<Item=&NativeAllocation> {
        self.new_allocations().chain(self.original_vector.iter())
    }
}

fn pool_plan(p: &impl RelationMemory, base: u64) -> Result<(u64,Vec<Guard>,Vec<Change>)> {
    let pool=base+POOL_RVA;
    let mut guards=vec![];
    let raw=guard(p,&mut guards,pool,0x50)?;
    let b=q(&raw,0x10); let e=q(&raw,0x18); let c=q(&raw,0x20);
    let head=q(&raw,0x28);
    let free=u32::from_le_bytes(raw[0x30..0x34].try_into().unwrap());
    let used=u32::from_le_bytes(raw[0x34..0x38].try_into().unwrap());
    let stride=u32::from_le_bytes(raw[0x38..0x3c].try_into().unwrap());
    let first=u32::from_le_bytes(raw[0x3c..0x40].try_into().unwrap());
    let next=u32::from_le_bytes(raw[0x40..0x44].try_into().unwrap());
    ensure!(pointer(b)&&b<e&&e<=c&&(e-b)%8==0&&c-b<=65536&&stride==16&&(1..=1_000_000).contains(&first)&&next==128, "커리어 전용 메모리 풀 구조를 검증할 수 없습니다");
    let blocks=guard(p,&mut guards,b,(e-b) as usize)?;
    // FM pre-sizes its first block when loading a database. Subsequent blocks
    // use +0x40; DC1CD0 selects +0x3c only while the block vector is empty.
    let allocated=u64::from(first)+(blocks.len()/8-1) as u64*u64::from(next);
    ensure!(free>0 && used>0 && free as u64+used as u64==allocated, "커리어 전용 메모리 풀의 여유 레코드가 없습니다");
    let belongs=|node:u64| blocks.chunks_exact(8).enumerate().any(|(index,block)|{ let block=q(block,0);let bytes=u64::from(if index==0{first}else{next})*16;pointer(block)&&node>=block&&node<block+bytes&&(node-block)%16==0 });
    ensure!(pointer(head)&&belongs(head), "커리어 여유 레코드가 원래 풀에 속하지 않습니다");
    let record=guard(p,&mut guards,head,16)?; let next_node=q(&record,0);
    ensure!(next_node!=head && (if free==1 { next_node==0 } else {pointer(next_node)&&belongs(next_node)}), "커리어 메모리 풀 다음 항목이 잘못되었습니다");
    let blank=[vec![0;8],vec![1,1,0,255,2,0,0,0]].concat();
    let changes=vec![
        change("career_pool_head","커리어 레코드 확보",pool+0x28,head.to_le_bytes().to_vec(),next_node.to_le_bytes().to_vec())?,
        change("career_pool_counts","커리어 풀 사용량",pool+0x30,raw[0x30..0x38].to_vec(),[(free-1).to_le_bytes(),used.checked_add(1).context("커리어 풀 개수 초과")?.to_le_bytes()].concat())?,
        change("career_pool_record","새 커리어 레코드 초기화",head,record,blank)?,
    ];
    Ok((head,guards,changes))
}

pub(super) fn available(s:&Session)->Result<()> {
    native_alloc::available(s)?;
    native_alloc::career_storage_minimum(s)?;
    let p=Process::open(s.pid,false)?;
    verify_pool_code(&p,s.module_base)?;
    pool_plan(&p,s.module_base).map(|_|())
}

fn reserve_record(s:&Session)->Result<NativeAllocation> {
    available(s)?;
    let p=Process::open(s.pid,true)?;
    let suspended=p.suspend()?;
    let result:Result<u64>=(|| {
        backend::validate_with(&p,s)?;
        verify_pool_code(&p,s.module_base)?;
        let code_ranges:Vec<_>=POOL_CODE.iter().map(|(begin,end,_)|(s.module_base+begin,s.module_base+end)).collect();
        suspended.assert_quiescent(&code_ranges)?;
        let (record,guards,changes)=pool_plan(&p,s.module_base)?;
        let path=transaction::data_dir().join("career-reservations").join(format!("{}-{}.json",SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),s.pid));
        std::fs::create_dir_all(path.parent().unwrap())?;
        transaction::persist_json(&path,&serde_json::json!({"state":"prepared","session":s,"record":record,"guards":guards,"changes":changes}))?;
        validate_guards(&p,&guards)?;
        write_batch(&p,&changes,&expected_guards(&guards,&changes))?;
        // Failure after this point only retains an allocated, detached node.
        transaction::persist_json(&path,&serde_json::json!({"state":"reserved_detached","session":s,"record":record,"guards":guards,"changes":changes}))?;
        Ok(record)
    })();
    let resumed=suspended.resume();
    let record=result?; resumed?;
    native_alloc::adopt_career_storage(s,record,16,s.module_base+POOL_RVA)
}

pub(super) fn intent(snapshot:&PersonRelations,new_record:Vec<u8>)->Result<CareerGrowth> {
    let career=snapshot.career.as_ref().context("커리어 계획 목록을 검증할 수 없습니다")?;
    ensure!(career.allocation_available && new_record.len()==16 && career.rows.len()<64, "커리어 새 항목 할당을 사용할 수 없습니다");
    ensure!(guarded_bytes(&snapshot.guards,snapshot.entity.address+0xC8,8)?==career.holder.to_le_bytes(),"커리어 소유자가 변경됐습니다");
    if career.holder!=0 {
        ensure!(career_vector_bounds(&guarded_bytes(&snapshot.guards,career.holder,24)?)?==(career.begin,career.end,career.capacity),"커리어 목록 경계가 다릅니다");
        if !career.rows.is_empty() { career_delete_check(snapshot,0)?; }
    } else { ensure!(career.rows.is_empty() && career.begin==0&&career.end==0&&career.capacity==0,"비어 있는 커리어 소유자 구조가 다릅니다"); }
    ensure!(!career.rows.iter().any(|r|r.raw[..12]==new_record[..12]),"같은 커리어 계획이 이미 있습니다");
    Ok(CareerGrowth{holder:career.holder,begin:career.begin,end:career.end,capacity:career.capacity,pointers:career.rows.iter().map(|r|r.record).collect(),types:career.rows.iter().map(|r|r.object_type).collect(),new_record})
}

fn materialize(p:&impl RelationMemory,plan:&RelationPlan,storage:CareerStorage)->Result<RelationPlan> {
    let intent=plan.career_growth.as_ref().context("커리어 할당 계획이 없습니다")?;
    ensure!(plan.changes.is_empty()&&plan.career_storage.is_none()&&intent.new_record.len()==16&&intent.pointers.len()==intent.types.len()&&intent.pointers.len()<64,"커리어 할당 계획이 잘못되었습니다");
    let original=career_snapshot(&GuardMemory(&plan.guards),&plan.entity,&mut vec![])?;
    ensure!((intent.holder,intent.begin,intent.end,intent.capacity)==(original.holder,original.begin,original.end,original.capacity)&&intent.pointers==original.rows.iter().map(|r|r.record).collect::<Vec<_>>()&&intent.types==original.rows.iter().map(|r|r.object_type).collect::<Vec<_>>(),"커리어 원본 목록 기록이 다릅니다");
    ensure!(intent.new_record[12..]==[2,0,0,0],"새 커리어 레코드 기본값이 다릅니다");
    match &plan.request {
        RelationRequest::AddCareerClub{club,condition,job}=>ensure!(club.kind==EntityKind::Club&&q(&intent.new_record,0)==club.address&&intent.new_record[8..12]==[*condition,1,*job,3],"커리어 생성 요청과 기록이 다릅니다"),
        RelationRequest::AddCareerTarget{target,condition,scope,job}=>{
            let address=q(&intent.new_record,0);let raw=guarded_bytes(&plan.guards,address,16)?;
            ensure!(u32::from_le_bytes(raw[12..16].try_into().unwrap())==target.uid&&intent.new_record[8..12]==[*condition,*scope,*job,target.object_type],"커리어 생성 대상과 원본 기록이 다릅니다");
        },
        _=>bail!("커리어 생성 요청이 아닙니다"),
    }
    for a in storage.allocations() { ensure!(serde_json::to_vec(&a.session)?==serde_json::to_vec(&plan.session)?&&pointer(a.address),"커리어 할당 세션이 다릅니다"); }
    ensure!(storage.record.len==16 && storage.vector.len>=((intent.pointers.len()+1)*8) && storage.vector.len<=MAX_VECTOR_BYTES as usize,"커리어 할당 크기가 다릅니다");
    match (&storage.original_vector,intent.begin) {
        (None,0)=>(),
        (Some(old),begin)=>ensure!(old.address==begin&&old.len as u64==intent.capacity-begin,"보존할 원래 커리어 배열이 다릅니다"),
        _=>bail!("원래 커리어 배열 보존 기록이 없습니다"),
    }
    let holder=if intent.holder==0 { let h=storage.holder.as_ref().context("새 커리어 목록 할당이 없습니다")?;ensure!(h.len==24,"커리어 목록 크기가 다릅니다");h.address } else {ensure!(storage.holder.is_none(),"기존 커리어 목록을 교체할 수 없습니다");intent.holder};
    let mut ranges=vec![(storage.record.address,storage.record.address+16),(storage.vector.address,storage.vector.address+storage.vector.len as u64),(holder,holder+24)];
    ranges.sort_unstable(); ensure!(ranges.windows(2).all(|v|v[0].1<=v[1].0),"커리어 할당 영역이 서로 겹칩니다");
    let mut owned=vec![(plan.entity.address,plan.entity.address+0xD0),(intent.begin,intent.capacity)];
    if intent.holder!=0 {owned.push((intent.holder,intent.holder+24));}
    owned.extend(intent.pointers.iter().map(|p|(*p,*p+16)));
    for a in [&storage.record,&storage.vector].into_iter().chain(storage.holder.iter()) {
        ensure!(owned.iter().all(|(b,e)|*b==*e||a.address+a.len as u64<=*b||a.address>=*e),"새 커리어 할당이 기존 객체와 겹칩니다");
    }
    ensure!(!intent.pointers.contains(&storage.record.address),"이미 연결된 커리어 레코드입니다");
    validate_guards(p,&plan.guards)?;
    let mut out=plan.clone();
    let old_record=guard(p,&mut out.guards,storage.record.address,16)?;
    ensure!(blank_career(&old_record),"새 커리어 레코드가 기본 상태가 아닙니다");
    out.changes.push(change("career_create_record","새 커리어 계획",storage.record.address,old_record,intent.new_record.clone())?);
    let mut ordered:Vec<_>=intent.pointers.iter().copied().zip(intent.types.iter().copied()).collect();
    ordered.push((storage.record.address,intent.new_record[11])); ordered.sort_by_key(|(_,t)|std::cmp::Reverse(career_type_rank(*t)));
    let used=ordered.len()*8; let mut vector:Vec<u8>=ordered.into_iter().flat_map(|(p,_)|p.to_le_bytes()).collect();vector.resize(storage.vector.len,0);
    let old=guard(p,&mut out.guards,storage.vector.address,storage.vector.len)?;
    for (i,(before,after)) in old.chunks(64).zip(vector.chunks(64)).enumerate(){if before!=after{out.changes.push(change("career_create_vector","커리어 계획 목록 준비",storage.vector.address+(i*64) as u64,before.to_vec(),after.to_vec())?);}}
    let header=[storage.vector.address.to_le_bytes(),(storage.vector.address+used as u64).to_le_bytes(),(storage.vector.address+storage.vector.len as u64).to_le_bytes()].concat();
    let old_header=guard(p,&mut out.guards,holder,24)?;
    if intent.holder==0 {ensure!(old_header==[0;24],"새 커리어 목록이 초기 상태가 아닙니다");}
    out.changes.push(change(if intent.holder==0 {"career_create_holder"}else{"career_create_publish"},"커리어 계획 목록 연결",holder,old_header,header)?);
    if intent.holder==0 {out.changes.push(change("career_create_publish","첫 커리어 계획 연결",plan.entity.address+0xC8,0u64.to_le_bytes().to_vec(),holder.to_le_bytes().to_vec())?);}
    out.career_storage=Some(storage);Ok(out)
}

pub(super) fn allocate_and_materialize(plan:&RelationPlan)->Result<RelationPlan> {
    let intent=plan.career_growth.as_ref().context("커리어 할당 계획이 없습니다")?;
    available(&plan.session)?;
    let p=Process::open(plan.session.pid,false)?;validate_guards(&p,&plan.guards)?;
    let requested=native_alloc::career_storage_minimum(&plan.session)?.max((intent.pointers.len()+1)*8);
    let vector=native_alloc::allocate(&plan.session,requested)?;
    let holder=if intent.holder==0 {Some(native_alloc::allocate_career_holder(&plan.session)?)}else{None};
    let original_vector=if intent.begin!=0 {Some(native_alloc::adopt_career_storage(&plan.session,intent.begin,(intent.capacity-intent.begin) as usize,intent.holder)?)}else{None};
    validate_guards(&p,&plan.guards)?;
    let record=reserve_record(&plan.session)?;
    let storage=CareerStorage{record,vector,holder,original_vector};
    let result=materialize(&p,plan,storage.clone());
    if result.is_err(){for a in storage.new_allocations(){let _=native_alloc::mark_detached(a);}}
    result
}

pub(super) fn verify_ownership(plan:&RelationPlan)->Result<()> {
    let storage=plan.career_storage.as_ref().context("커리어 할당 소유권 기록이 없습니다")?;
    for a in storage.allocations(){native_alloc::verify_retained(a)?;}Ok(())
}
pub(super) fn record_publication(plan:&RelationPlan,undo:bool) {
    if let Some(storage)=&plan.career_storage {
        for a in storage.new_allocations(){let _=if undo{native_alloc::mark_detached(a)}else{native_alloc::mark_published(a,plan.entity.address)};}
        if let Some(original)=&storage.original_vector {
            let _=if undo{native_alloc::mark_published(original,plan.entity.address)}else{native_alloc::mark_detached(original)};
        }
    }
}
pub(super) fn validate_undo(p:&impl RelationMemory,original:&RelationPlan)->Result<()> {
    let storage=original.career_storage.as_ref().context("커리어 할당 기록이 없습니다")?;
    let mut symbolic=original.clone();symbolic.changes.clear();symbolic.career_storage=None;
    let canonical=materialize(&GuardMemory(&original.guards),&symbolic,storage.clone())?;
    ensure!(serde_json::to_vec(&canonical.changes)?==serde_json::to_vec(&original.changes)?,"커리어 생성 변경 기록이 변조됐습니다");
    validate_guards(p,&expected_guards(&original.guards,&original.changes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell,collections::BTreeMap};
    #[derive(Default)]
    struct Mem { bytes:RefCell<BTreeMap<u64,u8>>, writes:RefCell<usize>, fail:RefCell<Option<usize>> }
    impl Mem {
        fn put(&self,at:u64,raw:&[u8]) {for (i,b) in raw.iter().enumerate(){self.bytes.borrow_mut().insert(at+i as u64,*b);}}
        fn from_guards(guards:&[Guard])->Self {let p=Self::default();for g in guards{p.put(g.address,&g.expected);}p}
    }
    impl RelationMemory for Mem {
        fn read(&self,at:u64,len:usize)->Result<Vec<u8>>{(0..len).map(|i|self.bytes.borrow().get(&(at+i as u64)).copied().context("missing fixture byte")).collect()}
        fn write(&self,at:u64,raw:&[u8])->Result<()>{let n=*self.writes.borrow()+1;*self.writes.borrow_mut()=n;if *self.fail.borrow()==Some(n){self.fail.replace(None);bail!("injected write failure");}self.put(at,raw);Ok(())}
    }
    fn pool_fixture()->Mem {
        let p=Mem::default();let pool=0x10000+POOL_RVA;let mut raw=vec![0;0x50];
        for (offset,value) in [(0x10,0x20000u64),(0x18,0x20008),(0x20,0x20008),(0x28,0x30100)]{raw[offset..offset+8].copy_from_slice(&value.to_le_bytes());}
        for (offset,value) in [(0x30,112u32),(0x34,16),(0x38,16),(0x3c,128),(0x40,128)]{raw[offset..offset+4].copy_from_slice(&value.to_le_bytes());}
        p.put(pool,&raw);p.put(0x20000,&0x30000u64.to_le_bytes());p.put(0x30100,&[0x30110u64.to_le_bytes(),[0xff;8]].concat());p
    }
    #[test]
    fn native_career_pool_reservation_preserves_ownership_and_rolls_back_each_failure() {
        let p=pool_fixture();let baseline=p.bytes.borrow().clone();let (node,guards,changes)=pool_plan(&p,0x10000).unwrap();assert_eq!(node,0x30100);
        for fail in 1..=changes.len(){let m=pool_fixture();m.fail.replace(Some(fail));assert!(write_batch(&m,&changes,&expected_guards(&guards,&changes)).is_err());assert_eq!(*m.bytes.borrow(),baseline);}
        write_batch(&p,&changes,&expected_guards(&guards,&changes)).unwrap();assert!(blank_career(&p.read(node,16).unwrap()));
        assert_eq!(q(&p.read(0x10000+POOL_RVA+0x28,8).unwrap(),0),0x30110);
        for mutation in 0..4 {let p=pool_fixture();let pool=0x10000+POOL_RVA;match mutation{0=>p.put(pool+0x28,&0x90000u64.to_le_bytes()),1=>p.put(pool+0x38,&32u32.to_le_bytes()),2=>p.put(pool+0x30,&113u32.to_le_bytes()),_=>p.put(0x30100,&0x30100u64.to_le_bytes())};assert!(pool_plan(&p,0x10000).is_err());}
    }
    #[test]
    fn career_pool_supports_native_presized_first_block_and_normal_following_blocks() {
        let p=pool_fixture();let pool=0x10000+POOL_RVA;
        p.put(pool+0x3c,&1024u32.to_le_bytes());p.put(pool+0x30,&1008u32.to_le_bytes());
        p.put(pool+0x28,&0x33f00u64.to_le_bytes());p.put(0x33f00,&[0x33f10u64.to_le_bytes(),[0;8]].concat());
        assert_eq!(pool_plan(&p,0x10000).unwrap().0,0x33f00);
        p.put(pool+0x18,&0x20010u64.to_le_bytes());p.put(pool+0x20,&0x20010u64.to_le_bytes());p.put(0x20008,&0x40000u64.to_le_bytes());
        p.put(pool+0x30,&1136u32.to_le_bytes());p.put(pool+0x28,&0x40100u64.to_le_bytes());p.put(0x40100,&[0x40110u64.to_le_bytes(),[0;8]].concat());
        assert_eq!(pool_plan(&p,0x10000).unwrap().0,0x40100);
        p.put(pool+0x28,&0x41000u64.to_le_bytes());assert!(pool_plan(&p,0x10000).is_err());
    }
    fn creation_fixture(empty:bool)->(Mem,RelationPlan,CareerStorage) {
        let (index,_,snapshots)=fixtures::data();let mut snapshot=snapshots[0].clone();let career=snapshot.career.as_mut().unwrap();
        if empty {
            let owner=snapshot.entity.address+0xC8;snapshot.guards.retain(|g|g.address<career.holder||g.address>career.capacity+0x1000);
            snapshot.guards.iter_mut().find(|g|g.address==owner).unwrap().expected=0u64.to_le_bytes().to_vec();
            career.holder=0;career.begin=0;career.end=0;career.capacity=0;career.rows.clear();
        }
        career.allocation_available=true;
        let old_begin=career.begin;let old_capacity=career.capacity;
        let club=index.entities.iter().find(|e|e.kind==EntityKind::Club&&e.id==45).unwrap().clone();
        let target=LinkedRecord{uid:club.id,name:club.name.clone(),address:club.address,object_type:1};
        let plan=prepare_snapshot(&index.session,&snapshot,&RelationRequest::AddCareerClub{club,condition:1,job:1},Some(&target),None,vec![]).unwrap();
        let m=Mem::from_guards(&plan.guards);
        let allocation=|address,len| NativeAllocation{id:format!("{address}-1"),address,len,session:index.session.clone()};
        let storage=CareerStorage{record:allocation(0x120000,16),vector:allocation(0x110000,2048),holder:empty.then(||allocation(0x130000,24)),original_vector:(!empty).then(||allocation(old_begin,(old_capacity-old_begin) as usize))};
        m.put(storage.record.address,&[vec![0;8],vec![1,1,0,255,2,0,0,0]].concat());m.put(storage.vector.address,&vec![0;2048]);if empty{m.put(0x130000,&[0;24]);}
        (m,plan,storage)
    }
    #[test]
    fn career_creation_handles_first_holder_growth_and_guarded_undo_without_freeing() {
        for empty in [false,true] {
            let (m,symbolic,storage)=creation_fixture(empty);let original=m.bytes.borrow().clone();
            let plan=materialize(&m,&symbolic,storage.clone()).unwrap();assert_eq!(*m.writes.borrow(),0);
            for fail in 1..=plan.changes.len(){let (p,_,_)=creation_fixture(empty);p.fail.replace(Some(fail));assert!(write_batch(&p,&plan.changes,&expected_guards(&plan.guards,&plan.changes)).is_err());assert_eq!(*p.bytes.borrow(),original);}
            write_batch(&m,&plan.changes,&expected_guards(&plan.guards,&plan.changes)).unwrap();
            let current=career_snapshot(&m,&plan.entity,&mut vec![]).unwrap();assert_eq!(current.rows.len(),if empty{1}else{2});assert_eq!(current.capacity-current.begin,2048);
            validate_undo(&m,&plan).unwrap();let retained=m.read(storage.vector.address,2048).unwrap();let reverse=reverse_plan(&plan);
            assert!(reverse.changes.iter().all(|c|c.key=="career_create_publish"));write_batch(&m,&reverse.changes,&expected_guards(&reverse.guards,&reverse.changes)).unwrap();
            assert_eq!(m.read(storage.vector.address,2048).unwrap(),retained);let restored=career_snapshot(&m,&plan.entity,&mut vec![]).unwrap();assert_eq!(restored.rows.len(),if empty{0}else{1});
        }
    }
    #[test]
    fn career_creation_rejects_changed_baselines_foreign_storage_and_forged_intent() {
        let (m,plan,storage)=creation_fixture(false);
        let mut wrong=storage.clone();wrong.record.address=plan.entity.address;assert!(materialize(&m,&plan,wrong).is_err());
        let mut wrong=plan.clone();wrong.career_growth.as_mut().unwrap().new_record[10]=16;assert!(materialize(&m,&wrong,storage.clone()).is_err());
        let mut wrong=plan.clone();wrong.career_growth.as_mut().unwrap().pointers.clear();assert!(materialize(&m,&wrong,storage.clone()).is_err());
        m.put(plan.entity.address+0xC8,&0x90010u64.to_le_bytes());assert!(materialize(&m,&plan,storage).is_err());
    }
}
