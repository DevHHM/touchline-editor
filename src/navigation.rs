//! Entity links are UID based. No name matching or guessed relationships.
use crate::model::{Detail, Entity, EntityKind, Index};
use std::collections::HashMap;

/// Build once when a snapshot is replaced, then reuse for all detail navigation.
pub struct RelationIndex<'a> {
    index: &'a Index,
    by_key: HashMap<(EntityKind, u32), usize>,
    by_club: HashMap<(u32, EntityKind), Vec<usize>>,
}

impl<'a> RelationIndex<'a> {
    pub fn new(index: &'a Index) -> Self {
        let mut by_key = HashMap::new();
        let mut by_club: HashMap<_, Vec<_>> = HashMap::new();
        for (n, entity) in index.entities.iter().enumerate() {
            by_key.insert((entity.kind, entity.id), n);
            if let Some(club) = entity.club_id {
                by_club.entry((club, entity.kind)).or_default().push(n);
            }
        }
        Self {
            index,
            by_key,
            by_club,
        }
    }
    pub fn entity(&self, kind: EntityKind, id: u32) -> Option<&'a Entity> {
        self.by_key
            .get(&(kind, id))
            .map(|i| &self.index.entities[*i])
    }
    pub fn club_for(&self, entity: &Entity) -> Option<&'a Entity> {
        let id = if entity.kind == EntityKind::Club {
            entity.id
        } else {
            entity.club_id?
        };
        self.entity(EntityKind::Club, id)
    }
    /// Detail contains the exact club→home stadium→city link. Legacy snapshots
    /// retain only one source club per city, so that fallback is deliberately narrow.
    pub fn city_for(&self, club: &Entity, detail: Option<&Detail>) -> Option<&'a Entity> {
        if club.kind != EntityKind::Club {
            return None;
        }
        if let Some(id) = detail
            .filter(|d| d.entity.kind == EntityKind::Club && d.entity.id == club.id)
            .and_then(|d| d.fields.iter().find(|f| f.key == "club.city_id"))
            .filter(|f| {
                f.value.is_finite()
                    && f.value > 0.
                    && f.value <= u32::MAX as f64
                    && f.value.fract() == 0.
            })
            .map(|f| f.value as u32)
        {
            return self.entity(EntityKind::City, id);
        }
        self.by_club
            .get(&(club.id, EntityKind::City))
            .and_then(|rows| {
                if rows.len() == 1 {
                    Some(&self.index.entities[rows[0]])
                } else {
                    None
                }
            })
    }
    pub fn members(&self, club: u32, kind: EntityKind) -> Vec<&'a Entity> {
        self.by_club
            .get(&(club, kind))
            .into_iter()
            .flatten()
            .map(|i| &self.index.entities[*i])
            .collect()
    }
}

/// One-off click handlers may use these without rebuilding a whole relation map.
pub fn entity(index: &Index, kind: EntityKind, id: u32) -> Option<&Entity> {
    index.entities.iter().find(|e| e.kind == kind && e.id == id)
}
pub fn club_for<'a>(index: &'a Index, source: &Entity) -> Option<&'a Entity> {
    entity(
        index,
        EntityKind::Club,
        if source.kind == EntityKind::Club {
            source.id
        } else {
            source.club_id?
        },
    )
}
pub fn city_for<'a>(
    index: &'a Index,
    club: &Entity,
    detail: Option<&Detail>,
) -> Option<&'a Entity> {
    if club.kind != EntityKind::Club {
        return None;
    }
    if let Some(id) = detail
        .filter(|d| d.entity.kind == EntityKind::Club && d.entity.id == club.id)
        .and_then(|d| d.fields.iter().find(|f| f.key == "club.city_id"))
        .filter(|f| {
            f.value.is_finite()
                && f.value > 0.
                && f.value <= u32::MAX as f64
                && f.value.fract() == 0.
        })
        .map(|f| f.value as u32)
    {
        return entity(index, EntityKind::City, id);
    }
    let mut found = index
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::City && e.club_id == Some(club.id));
    let city = found.next()?;
    if found.next().is_none() {
        Some(city)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Field, Session};
    fn item(kind: EntityKind, id: u32, club_id: Option<u32>) -> Entity {
        Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id,
            kind,
            name: "same name".into(),
            english_name: String::new(),
            club: String::new(),
            club_id,
            age: None,
            ca: None,
            pa: None,
            role: String::new(),
            group: String::new(),
            address: 0,
            base: 0,
            index: None,
        }
    }
    fn snapshot() -> Index {
        Index {
            club_leagues: Vec::new(),
            session: Session {
                pid: 0,
                created: 0,
                module_base: 0,
                module_size: 0,
                module_stamp: 0,
                db: 0,
                registry_begin: 0,
                game_date: String::new(),
                profile: String::new(),
                own_club_id: Some(1),
            },
            warnings: vec![],
            entities: vec![
                item(EntityKind::Club, 1, None),
                item(EntityKind::Club, 2, None),
                item(EntityKind::Player, 3, Some(1)),
                item(EntityKind::City, 4, Some(2)),
            ],
        }
    }
    #[test]
    fn links_are_uid_based_and_exact_city_detail_beats_legacy_source() {
        let index = snapshot();
        let links = RelationIndex::new(&index);
        let player = links.entity(EntityKind::Player, 3).unwrap();
        let club = links.club_for(player).unwrap();
        assert_eq!(club.id, 1);
        assert!(links.city_for(club, None).is_none());
        let detail = Detail {
            entity: club.clone(),
            guards: vec![],
            notes: vec![],
            fields: vec![Field {
                key: "club.city_id".into(),
                label: "City".into(),
                section: "Links".into(),
                value: 4.,
                min: 0.,
                max: u32::MAX as f64,
                step: 1.,
                hint: String::new(),
                binding: None,
            }],
        };
        assert_eq!(links.city_for(club, Some(&detail)).unwrap().id, 4);
        assert_eq!(city_for(&index, club, Some(&detail)).unwrap().id, 4);
        assert_eq!(links.members(1, EntityKind::Player).len(), 1);
        let wrong = item(EntityKind::Club, 2, None);
        assert_eq!(links.city_for(&wrong, Some(&detail)).unwrap().id, 4);
    }
}
