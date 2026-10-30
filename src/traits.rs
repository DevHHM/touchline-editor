//! Verified FM26 preferred moves and shared bit-edit plumbing. Unknown bits are preserved.
use crate::{memory::Process,model::*};
use anyhow::{Result,ensure};
pub const NAMES:&[(u8,&str,&str)]=&[
(1,"왼쪽 측면으로 공을 몰고 감","Runs With Ball Down Left"),(2,"오른쪽 측면으로 공을 몰고 감","Runs With Ball Down Right"),(3,"중앙으로 공을 몰고 감","Runs With Ball Through Centre"),
(4,"페널티 박스 안으로 침투","Gets Into Opposition Area"),(5,"수비수 사이 공간으로 침투","Moves Into Channels"),(6,"기회가 되면 최전방으로 침투","Gets Forward Whenever Possible"),
(7,"짧고 간단한 패스","Plays Short Simple Passes"),(8,"결정적인 패스를 자주 시도","Tries Killer Balls Often"),(9,"중거리 슛 선호","Shoots From Distance"),(10,"강력한 슛 선호","Shoots With Power"),(11,"정확한 슛 선호","Places Shots"),(12,"공을 휘어 참","Curls Ball"),
(13,"골키퍼를 제침","Rounds Keeper"),(14,"오프사이드 트랩 돌파","Likes To Beat Offside Trap"),(15,"발 바깥쪽 사용","Uses Outside Of Foot"),(16,"상대를 밀착 마크","Marks Opponent Tightly"),(17,"상대 선수를 자극","Winds Up Opponents"),(18,"심판과 논쟁","Argues With Officials"),
(19,"골문을 등지고 플레이","Plays With Back To Goal"),(20,"공을 받으러 내려옴","Comes Deep To Get Ball"),(21,"2대1 패스 선호","Plays One-Twos"),(22,"골키퍼 위로 띄워 참","Lobs Keeper"),(23,"템포 조절","Dictates Tempo"),(24,"오버헤드 킥 시도","Attempts Overhead Kicks"),
(25,"득점보다 패스 선호","Looks For Pass Rather Than Score"),(26,"스루 패스를 자제","Avoids Through Balls"),(27,"플레이를 멈춰 세움","Stops Play"),(28,"공을 차놓고 상대를 제침","Knocks Ball Past Opponent"),(29,"오른발로 공을 옮김","Moves Ball To Right Foot"),(30,"왼발로 공을 옮김","Moves Ball To Left Foot"),
(31,"공을 오래 소유","Dwells On Ball"),(32,"페널티 박스에 늦게 침투","Arrives Late In Opposition Area"),(33,"압박에서 공을 지켜 나옴","Plays Out Of Trouble"),(34,"항상 후방에 머무름","Stays Back At All Times"),(35,"약한 발 사용을 피함","Avoids Using Weaker Foot"),(36,"개인기 시도","Tries Tricks"),
(37,"장거리 프리킥 시도","Tries Long Range Free Kicks"),(38,"과감한 태클","Dives Into Tackles"),(39,"성급한 태클을 하지 않음","Does Not Dive Into Tackles"),(40,"안쪽으로 파고듦","Cuts Inside"),(41,"터치라인을 따라 움직임","Hugs Line"),(42,"관중의 호응을 유도","Gets Crowd Going"),
(43,"논스톱 슛 시도","Hits First Time Shots"),(44,"긴 패스 시도","Tries Long Range Passes"),(45,"발밑으로 공을 받기 선호","Likes Ball Played Into Feet"),(46,"강력한 프리킥","Hits Free Kicks With Power"),(47,"같은 선수를 반복해서 제침","Likes To Beat Man Repeatedly"),(48,"반대편 측면으로 전환","Switches Ball To Other Flank"),
(49,"전성기에 은퇴","Retires At Peak"),(50,"최대한 오래 선수 생활","Plays As Long As Possible"),(51,"낮고 긴 스로인","Possesses Long Flat Throw"),(52,"공을 자주 드리블","Runs With Ball Often"),(53,"공을 거의 드리블하지 않음","Runs With Ball Rarely"),(54,"약한 발을 개발","Develops Weaker Foot"),
(55,"정적인 타깃맨","Static Target Man"),(56,"골키퍼의 긴 스로우 역습","GK Long Throw Counter"),(57,"중거리 슛을 자제","Refrains From Long Shots"),(58,"왼쪽에서 안쪽으로 침투","Cuts Inside From Left"),(59,"오른쪽에서 안쪽으로 침투","Cuts Inside From Right"),(60,"이른 크로스","Crosses Early"),(61,"수비 진영에서 공을 몰고 전진","Brings Ball Out Of Defence"),(64,"발로 공을 제어","Uses Foot Control")];

pub fn append(p:&Process,d:&mut Detail)->Result<()> {
    if d.entity.kind!=EntityKind::Player {return Ok(())}
    let metadata=p.u64(d.entity.base+8)?;
    ensure!(metadata>0x10000,"선수 특성 메타데이터 없음");
    let displacement=p.i32(metadata+8)?;
    ensure!((0x200..=0x400).contains(&displacement),"선수 특성 메타데이터 범위 오류");
    let target=d.entity.base+displacement as u64+0xA0;
    ensure!(target==d.entity.address+0xC0,"선수 특성 구조가 현재 프로필과 다릅니다");
    let original=p.read(target,8)?;let mask=u64::from_le_bytes(original.clone().try_into().unwrap());
    d.guards.push(Guard{address:d.entity.base+8,expected:metadata.to_le_bytes().to_vec()});
    d.guards.push(Guard{address:metadata+8,expected:displacement.to_le_bytes().to_vec()});
    for &(id,label,english) in NAMES {d.fields.push(Field{key:format!("trait_{id}"),label:label.into(),section:"선수 특성".into(),value:((mask>>(id-1))&1)as f64,min:0.,max:1.,step:1.,hint:english.into(),binding:Some(Binding{address:target,storage:Storage::Bit(id-1),scale:1.,original:original.clone()})});}
    Ok(())
}
pub fn encode(bit:u8,original:&[u8],value:f64)->Result<Vec<u8>> {
    ensure!(bit<64&&original.len()==8&&(value==0.||value==1.),"비트 필드 값 오류");
    let old=u64::from_le_bytes(original.try_into().unwrap());let flag=1u64<<bit;
    Ok((if value==1.{old|flag}else{old&!flag}).to_le_bytes().to_vec())
}
pub fn merge_changes(d:&Detail,changes:&mut [Change])->Result<()> {
    // Merge all bitfields, but preferred-move contradictions apply only to
    // verified trait keys at the player's preferred-move storage address.
    let mut groups=std::collections::HashMap::<u64,(Vec<u8>,u64,u64,u64)>::new();
    for c in changes.iter(){let f=d.fields.iter().find(|f|f.key==c.key).unwrap();if let Some(Binding{storage:Storage::Bit(bit),..})=&f.binding {
        let old=u64::from_le_bytes(c.old_bytes.as_slice().try_into()?);let group=groups.entry(c.address).or_insert((c.old_bytes.clone(),old,0,0));
        ensure!(group.0==c.old_bytes&&(group.2&(1u64<<bit))==0,"같은 비트가 중복되거나 원본이 다릅니다");group.2|=1u64<<bit;
        if d.entity.kind==EntityKind::Player && c.address==d.entity.address+0xC0
            && c.key==format!("trait_{}",bit+1) && NAMES.iter().any(|(id,_,_)|*id==bit+1) {
            group.3|=1u64<<bit;
        }
        group.1=if c.after==1.{group.1|(1u64<<bit)}else{group.1&!(1u64<<bit)};
    }}
    for c in changes.iter_mut(){if let Some((_,mask,_,_))=groups.get(&c.address){ensure!(d.fields.iter().find(|f|f.key==c.key).and_then(|f|f.binding.as_ref()).is_some_and(|b|matches!(b.storage,Storage::Bit(_))),"비트 영역과 일반 필드가 겹칩니다");c.new_bytes=mask.to_le_bytes().to_vec();}}
    for (_,(_,mask,_,ppm_changed)) in groups {for (a,b) in [(8u8,26u8),(9,57),(6,34),(38,39),(49,50),(52,53)] {let pair=(1u64<<(a-1))|(1u64<<(b-1));ensure!(ppm_changed&pair==0||mask&pair!=pair,"서로 반대되는 선수 특성을 동시에 설정할 수 없습니다 ({a}, {b})");}}
    Ok(())
}
pub fn shared_bit_storage(d:&Detail,a:&Change,b:&Change)->bool {
    a.address==b.address&&a.old_bytes==b.old_bytes&&a.new_bytes==b.new_bytes&&[a,b].iter().all(|c|d.fields.iter().find(|f|f.key==c.key).and_then(|f|f.binding.as_ref()).is_some_and(|x|matches!(x.storage,Storage::Bit(_))))
}
#[cfg(test)] mod tests {use super::*;
#[test]fn highest_bit_is_lossless(){let old=0xF123456789ABCDEFu64.to_le_bytes();let b=encode(63,&old,0.).unwrap();assert_eq!(u64::from_le_bytes(b.try_into().unwrap()),0x7123456789ABCDEF);}
#[test]fn preserves_unknown_bits(){let old=(1u64<<62).to_le_bytes();let b=encode(41,&old,1.).unwrap();assert_eq!(u64::from_le_bytes(b.try_into().unwrap()),(1u64<<62)|(1u64<<41));}
#[test]fn rejects_fractional_bit(){assert!(encode(0,&[0;8],0.5).is_err());}
}
