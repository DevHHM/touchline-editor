use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    Player,
    Staff,
    Person,
    Club,
    City,
}
impl EntityKind {
    pub const ALL: [Self; 5] = [
        Self::Player,
        Self::Staff,
        Self::Club,
        Self::Person,
        Self::City,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Player => "선수",
            Self::Staff => "스태프",
            Self::Person => "기타 인물",
            Self::Club => "구단",
            Self::City => "도시",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub pid: u32,
    pub created: u64,
    pub module_base: u64,
    pub module_size: u32,
    pub module_stamp: u32,
    pub db: u64,
    pub registry_begin: u64,
    pub game_date: String,
    pub profile: String,
    pub own_club_id: Option<u32>,
}
/// Competition category, distinct from a staff member's own sex.
/// Old snapshots lack this evidence and must remain explicitly unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FootballCategory {
    Men,
    Women,
    #[default]
    Unknown,
}
impl FootballCategory {
    pub fn key(self) -> &'static str {
        match self { Self::Men => "men", Self::Women => "women", Self::Unknown => "unknown" }
    }
    pub fn label(self) -> &'static str {
        match self { Self::Men => "남자축구", Self::Women => "여자축구", Self::Unknown => "미확인" }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    pub id: u32,
    pub kind: EntityKind,
    pub name: String,
    pub english_name: String,
    /// Primary nationality only; old snapshots and unverified records remain unknown.
    #[serde(default)]
    pub nationality: String,
    #[serde(default)]
    pub nationality_id: Option<u32>,
    pub club: String,
    pub club_id: Option<u32>,
    pub age: Option<u32>,
    pub ca: Option<u16>,
    pub pa: Option<u16>,
    pub role: String,
    pub group: String,
    #[serde(default)]
    pub football_category: FootballCategory,
    pub address: u64,
    pub base: u64,
    pub index: Option<u32>,
}
pub const STAFF_AFFILIATION_UNKNOWN: &str = "소속 미확인";
impl Entity {
    /// None is a failed/unverified staff affiliation, not known unemployment.
    pub fn employment(&self) -> Option<bool> {
        if self.kind == EntityKind::Staff && self.group == STAFF_AFFILIATION_UNKNOWN { None }
        else { Some(self.club_id.is_some()) }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Index {
    pub session: Session,
    pub entities: Vec<Entity>,
    /// Confirmed first-team competition links captured with this index.
    /// Older snapshots preserve every club under the unclassified group.
    #[serde(default)]
    pub club_leagues: Vec<ClubLeague>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClubLeague {
    pub club_id: u32,
    #[serde(default)]
    pub league_id: Option<u32>,
    #[serde(default)]
    pub league: String,
    #[serde(default)]
    pub nation_id: Option<u32>,
    #[serde(default)]
    pub nation: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Storage {
    Bit(u8),
    U8,
    U16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
    /// Packed FM year/day-of-year date; display and edits use YYYYMMDD.
    FmDate,
    /// UInt32 money with 0xFFFFFFFF meaning no value; display uses -1.
    OptionalU32,
}
impl Storage {
    pub fn size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 | Self::I32 | Self::F32 | Self::FmDate | Self::OptionalU32 => 4,
            _ => 8,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub address: u64,
    pub storage: Storage,
    pub scale: f64,
    pub original: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub section: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub hint: String,
    pub binding: Option<Binding>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guard {
    pub address: u64,
    pub expected: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Detail {
    pub entity: Entity,
    pub fields: Vec<Field>,
    pub guards: Vec<Guard>,
    pub notes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub key: String,
    pub value: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub key: String,
    pub label: String,
    pub before: f64,
    pub after: f64,
    pub address: u64,
    pub old_bytes: Vec<u8>,
    pub new_bytes: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub time: String,
    pub session: Session,
    pub entity: Entity,
    pub guards: Vec<Guard>,
    pub changes: Vec<Change>,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchReceipt {
    pub id: String,
    pub time: String,
    pub session: Session,
    pub items: Vec<Receipt>,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_entity_snapshots_without_nationality_remain_readable() {
        let old = serde_json::json!({
            "id":1,"kind":"Player","name":"Example","english_name":"Example",
            "club":"Lecce","club_id":1141,"age":19,"ca":120,"pa":170,
            "role":"MC","group":"U20","address":65536,"base":65536,"index":null,
        });
        let mut entity: Entity = serde_json::from_value(old).unwrap();
        assert!(entity.nationality.is_empty());
        assert_eq!(entity.nationality_id, None);
        entity.nationality = "대한민국".into();
        entity.nationality_id = Some(101);
        let restored: Entity = serde_json::from_value(serde_json::to_value(&entity).unwrap()).unwrap();
        assert_eq!(restored.nationality, "대한민국");
        assert_eq!(restored.nationality_id, Some(101));
        assert_eq!(restored.club_id, Some(1141));
    }
}
