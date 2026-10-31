//! Exact build identities and independently verified read/write capabilities.
//! Recognizing an FM release never grants permission to reuse another layout.
use crate::model::Session;
use anyhow::{Result, anyhow, Context};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct ReadLayout {
    pub database_rva: u64,
    pub registry_mode_rva: u64,
    pub date_rva: u64,
    pub person_vector_offset: u64,
    pub person_indirect_vector_offset: u64,
    /// Verified FM27 accessor falls back to the indirect table when the inline
    /// table has no slots, even while the global registry mode remains zero.
    pub person_empty_inline_fallback: bool,
    pub city_database_rva: u64,
    pub nation_database_rva: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct CodeSignature {
    pub name: &'static str,
    pub rva: u64,
    pub bytes: &'static [u8],
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct BuildProfile {
    pub id: &'static str,
    pub label: &'static str,
    pub game: &'static str,
    pub timestamp: u32,
    pub image_size: u32,
    pub read_verified: bool,
    pub scalar_write_verified: bool,
    pub native_allocate_verified: bool,
    #[serde(skip)]
    pub layout: Option<ReadLayout>,
    #[serde(skip)]
    pub code_signatures: &'static [CodeSignature],
}
pub const FM26: BuildProfile = BuildProfile {
    id:"fm26-26.3.2-6a229e8a-1eb41000",
    label:"FM26 26.3.2 / 6A229E8A-1EB41000", game:"FM26",
    timestamp:0x6A229E8A,image_size:0x1EB41000,
    read_verified:true,scalar_write_verified:true,native_allocate_verified:true,
    layout:Some(ReadLayout { database_rva:0x4E495E0,registry_mode_rva:0x4E49C70,date_rva:0x4DF3C18,
        person_vector_offset:0x60,person_indirect_vector_offset:0x80,
        person_empty_inline_fallback:false,
        city_database_rva:0x4E49580,nation_database_rva:0x4E495A0 }),
    code_signatures:&[],
};
pub const FM27: BuildProfile = BuildProfile {
    id:"fm27-6adfb11c-19e69000",
    label:"FM27 / 6ADFB11C-19E69000",game:"FM27",
    timestamp:0x6ADFB11C,image_size:0x19E69000,
    // Native code + bounded live RTTI samples verified independently. This is
    // NOT a grant to reuse FM26 extended/relationship/allocation structures.
    read_verified:true,scalar_write_verified:true,native_allocate_verified:false,
    layout:Some(ReadLayout { database_rva:0x5703738,registry_mode_rva:0x5703DE0,date_rva:0x56AD378,
        person_vector_offset:0x60,person_indirect_vector_offset:0x80,
        person_empty_inline_fallback:true,
        city_database_rva:0x57036D8,nation_database_rva:0x57036F8 }),
    // output/fm27_port_backend/verified-native-signatures.json. All ranges are
    // outside the user's existing staff-filter skin patch sites.
    code_signatures:&[
        CodeSignature {name:"person registry accessor",rva:0xA71ED4,
            bytes:&[0x48,0x8B,0x0D,0x5D,0x18,0xC9,0x04,0x48,0x85,0xC9,0x74,0x7C,0x80,0x3D,0xF9,0x1E,0xC9,0x04,0x00,0x75,0x1A,0x48,0x8B,0x51,0x60,0x4C,0x8B,0x41,0x68,0x49,0x29,0xD0]},
        // The inline-empty fallback is enabled only with this independently
        // verified branch and indirect-vector accessor (also checked on disk).
        CodeSignature {name:"person registry indirect fallback",rva:0xA71EF4,
            bytes:&[0x49,0xC1,0xF8,0x03,0x49,0x39,0xC0,0x76,0x06,0x48,0x8D,0x04,0xC2,0xEB,0x22,0x48,0x8B,0x91,0x80,0x00,0x00,0x00,0x48,0x8B,0x0A,0x48,0x8B,0x52,0x08,0x48,0x29,0xCA,0x74,0x46,0x48,0xC1,0xFA,0x03,0x89,0xC0,0x48,0x39,0xC2,0x76,0x3B,0x48,0x8D,0x04,0xC1]},
        CodeSignature {name:"current date accessor",rva:0x4A0114,bytes:&[0x8B,0x0D,0x5E,0xD2,0x20,0x05]},
        CodeSignature {name:"player current ability",rva:0x15D4F70,bytes:&[0x0F,0xB7,0x81,0x64,0x02,0x00,0x00,0xC3]},
        CodeSignature {name:"player potential ability",rva:0x15F7060,bytes:&[0x0F,0xB7,0x81,0x66,0x02,0x00,0x00,0xC3]},
        // Independent native Pmor branch and exact byte store. The dispatch
        // receiver is Player base + 0x288; [rbx - 0x1C] is base + 0x26C.
        // See reference/fm27-compatibility/player-morale.json.
        CodeSignature {name:"player morale base",rva:0x160B31C,
            bytes:&[0x48,0x89,0xCB,0x48,0x8D,0xB9,0x78,0xFD,0xFF,0xFF]},
        CodeSignature {name:"player morale dispatch",rva:0x160B556,
            bytes:&[0x41,0x81,0xFF,0x72,0x6F,0x6D,0x50,0x0F,0x84,0x12,0x1C,0x00,0x00]},
        CodeSignature {name:"player morale setter",rva:0x160D175,
            bytes:&[0x48,0x8B,0x0E,0x48,0x8B,0x01,0x48,0x89,0xF2,0xFF,0x50,0x40,0x41,0xB6,0x01,0x48,0x89,0xF9,0x89,0xC2,0x41,0xB0,0x01,0xE8,0x0F,0xC2,0xFC,0xFF,0x88,0x43,0xE4,0xE9,0x05,0xE2,0xFF,0xFF]},
        CodeSignature {name:"staff current ability",rva:0x17E1541,bytes:&[0x0F,0xBF,0x73,0xDA]},
        CodeSignature {name:"staff potential ability",rva:0x17E04CB,bytes:&[0x0F,0xBF,0x73,0xDC]},
        // Independent FM27 property-dispatch evidence: reference/fm27-compatibility/league-navigation.json.
        CodeSignature {name:"team current division",rva:0x11FDCB8,bytes:&[0x48,0x8B,0x4F,0x50]},
        CodeSignature {name:"competition nation",rva:0x11AF492,bytes:&[0x48,0x8B,0x4F,0x60]},
        CodeSignature {name:"competition full name",rva:0x11B391F,bytes:&[0x48,0x8B,0x4F,0x40]},
        CodeSignature {name:"competition alternate name",rva:0x11B3E49,bytes:&[0x48,0x8B,0x4F,0x48]},
    ],
};
pub const ALL: &[BuildProfile] = &[FM26, FM27];
pub fn identify(timestamp:u32,image_size:u32)->Option<&'static BuildProfile> {
    ALL.iter().find(|p|p.timestamp==timestamp && p.image_size==image_size)
}
pub fn profile_for_session(session:&Session)->Option<&'static BuildProfile> {
    identify(session.module_stamp,session.module_size).filter(|p|p.label==session.profile)
}
pub fn can_write(session:&Session)->bool {
    profile_for_session(session).is_some_and(|p|p.read_verified && p.scalar_write_verified)
}
pub fn can_allocate(session:&Session)->bool {
    profile_for_session(session).is_some_and(|p|p.read_verified && p.native_allocate_verified)
}
#[derive(Debug, Clone)]
pub struct UnsupportedBuild {
    pub timestamp:u32,
    pub image_size:u32,
    pub profile:Option<&'static BuildProfile>,
}
impl std::fmt::Display for UnsupportedBuild {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        match self.profile {
            Some(p)=>write!(f,"{} 감지됨: 이 빌드의 데이터 구조가 아직 검증되지 않아 조회·수정을 연결하지 않았습니다",p.label),
            None=>write!(f,"지원하지 않는 FM 빌드입니다 ({:08X}/{:08X}). PC Game Pass/Microsoft Store판도 별도 빌드 검증이 필요합니다. 오류의 빌드 코드와 구입처를 알려 주세요",self.timestamp,self.image_size),
        }
    }
}
impl std::error::Error for UnsupportedBuild {}
impl BuildProfile {
    pub fn verify_code_with(&self, base:u64, mut read:impl FnMut(u64,usize)->Result<Vec<u8>>)->Result<()> {
        for signature in self.code_signatures {
            let actual=read(base+signature.rva,signature.bytes.len())?;
            if actual!=signature.bytes {
                return Err(anyhow!(UnsupportedBuild {timestamp:self.timestamp,image_size:self.image_size,
                    profile:identify(self.timestamp,self.image_size)}))
                    .with_context(||format!("{} 코드 검증 불일치: {}. 데이터 연결을 중단했습니다",self.game,signature.name));
            }
        }
        Ok(())
    }
    pub fn require_read_layout(&self)->Result<&ReadLayout> {
        if !self.read_verified || self.layout.is_none() {
            return Err(anyhow!(UnsupportedBuild {timestamp:self.timestamp,image_size:self.image_size,
                profile:identify(self.timestamp,self.image_size)}));
        }
        Ok(self.layout.as_ref().expect("checked verified layout"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_not_layout_permission() {
        assert_eq!(identify(0x6ADFB11C,0x19E69000).unwrap().game,"FM27");
        assert_eq!(FM27.require_read_layout().unwrap().database_rva,0x5703738);
        let mut unverified=FM27;unverified.read_verified=false;
        assert!(unverified.require_read_layout().is_err());
        assert!(FM27.scalar_write_verified && !FM27.native_allocate_verified);
        assert_eq!(FM26.require_read_layout().unwrap().database_rva,0x4E495E0);
        assert!(identify(0x6ADFB11D,0x19E69000).is_none());
        assert!(identify(0x6ADFB11C,0x1EB41000).is_none());
    }
    #[test]
    fn native_code_mismatch_cannot_pass_an_identical_pe_identity() {
        let base=0x180000000;
        assert!(FM27.verify_code_with(base,|a,n| {
            let s=FM27.code_signatures.iter().find(|s|base+s.rva==a).unwrap();
            assert_eq!(s.bytes.len(),n);Ok(s.bytes.to_vec())
        }).is_ok());
        let error=FM27.verify_code_with(base,|_,n|Ok(vec![0;n])).unwrap_err();
        assert!(error.downcast_ref::<UnsupportedBuild>().is_some());
    }
    #[test]
    fn morale_identity_or_store_mismatch_rejects_the_fm27_adapter() {
        for changed in FM27.code_signatures.iter().filter(|s|s.name.starts_with("player morale")) {
            let error=FM27.verify_code_with(0,|address,_| {
                let signature=FM27.code_signatures.iter().find(|s|s.rva==address).unwrap();
                let mut bytes=signature.bytes.to_vec();
                if address==changed.rva { bytes[0]^=1; }
                Ok(bytes)
            }).unwrap_err();
            assert!(error.downcast_ref::<UnsupportedBuild>().is_some());
            assert!(format!("{error:#}").contains(changed.name));
        }
    }
    #[test]
    fn session_capabilities_cannot_be_forged_by_label() {
        let mut s=Session{pid:1,created:1,module_base:0,module_size:FM26.image_size,module_stamp:FM26.timestamp,
            db:0,registry_begin:0,game_date:String::new(),profile:FM26.label.into(),own_club_id:None};
        assert!(can_write(&s));assert!(can_allocate(&s));
        s.profile=FM27.label.into();assert!(!can_write(&s));assert!(!can_allocate(&s));
        s.module_stamp=FM27.timestamp;s.module_size=FM27.image_size;assert!(can_write(&s));assert!(!can_allocate(&s));
    }
}
