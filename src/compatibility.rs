//! Build capabilities are independent of a successful read-only connection.
//! Only the field families certified for each adapter are exposed by detail.
use crate::{backend::profiles, model::{Detail, EntityKind, Session}};
use anyhow::{Result, ensure};

pub fn legacy_layout(session: &Session) -> bool {
    profiles::profile_for_session(session).is_some_and(|p| p.id == profiles::FM26.id)
}

pub fn require_scalar_write(session: &Session) -> Result<()> {
    ensure!(profiles::can_write(session),
        "이 FM 빌드의 수치 수정은 아직 검증되지 않았습니다. 조회만 가능합니다");
    Ok(())
}

pub fn require_legacy_feature(session: &Session, feature: &str) -> Result<()> {
    ensure!(legacy_layout(session),
        "{feature}: 이 FM 빌드에서는 아직 구조 검증이 끝나지 않았습니다");
    Ok(())
}

pub fn constrain_detail(session: &Session, detail: &mut Detail) {
    if !legacy_layout(session) {
        detail.fields.retain(|field| allowed_fm27_field(detail.entity.kind, &field.key));
        for field in &mut detail.fields {
            // Derived recommendation scores never become native write targets.
            if field.key.starts_with("score_") { field.binding = None; }
        }
    }
    if !profiles::can_write(session) {
        for field in &mut detail.fields { field.binding = None; }
        detail.notes.push("이 빌드는 조회만 검증되어 수치 변경을 제공하지 않습니다".into());
    }
}

// A closed inventory, independent of fields.rs: adding an FM26 field does not
// silently enable it for FM27. See reference/fm27-compatibility for evidence.
fn listed(keys: &str, key: &str) -> bool { keys.split_ascii_whitespace().any(|v| v == key) }
pub fn allowed_fm27_field(kind: EntityKind, key: &str) -> bool {
    if matches!(kind, EntityKind::Player | EntityKind::Staff)
        && listed("adaptability ambition loyalty pressure professionalism sportsmanship temperament controversy person.dob", key) {
        return true;
    }
    match kind {
        EntityKind::Player => listed(
            "ca pa height home_reputation current_reputation world_reputation
            crossing dribbling finishing heading long_shots marking off_ball passing penalties tackling vision
            handling aerial_reach command_of_area communication kicking throwing anticipation decisions one_on_ones
            positioning reflexes first_touch technique left_foot right_foot flair corners teamwork work_rate long_throws
            eccentricity rushing_out punching acceleration free_kicks strength stamina pace jumping_reach leadership
            dirtiness balance bravery consistency aggression agility important_matches injury_proneness versatility
            natural_fitness determination composure concentration
            position_gk position_sw position_dl position_dc position_dr position_dm position_ml position_mc position_mr
            position_aml position_amc position_amr position_st position_wbl position_wbr", key),
        EntityKind::Staff => (key.starts_with("score_") && !matches!(key,"score_gk_distribution"|"score_youth_goalkeeper")) || listed(
            "ca pa home_reputation current_reputation world_reputation
            staff_discipline staff_youth staff_determination staff_gk_shot staff_judging_ability staff_judging_potential
            staff_management staff_motivation staff_physiotherapy staff_knowledge staff_attack staff_defense staff_fitness
            staff_mental staff_technical staff_tactical staff_analysis
            staff_sports_science staff_negotiation staff_judging_staff staff_set_pieces", key),
        EntityKind::Club => listed(
            "club.cash club.transfer_base club.transfer_current club.weekly_wage club.player_wage_max club.staff_wage_max
            club.reputation club.city_id club.corporate_facilities club.stadium_condition club.stadium_capacity
            club.stadium_usable club.stadium_seated club.stadium_expansion", key),
        EntityKind::City => listed("city.nation_id city.population_band city.attraction city.latitude city.longitude", key),
        EntityKind::Person => false,
    }
}

pub fn capabilities(session: &Session) -> serde_json::Value {
    let core = profiles::can_write(session);
    let legacy = legacy_layout(session);
    serde_json::json!({
        "scalar_edit": core,
        "extended_fields": legacy,
        "relations": legacy,
        "career_plans": legacy,
        "sponsor_events": legacy,
        "growth_estimates": legacy,
        "native_allocation": profiles::can_allocate(session),
        "name_localization": profiles::profile_for_session(session).is_some_and(|p| p.read_verified),
        "game_writes_tested_in_this_port": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> Session {
        Session {pid:0,created:0,module_base:0,module_size:profiles::FM26.image_size,
            module_stamp:profiles::FM26.timestamp,db:0,registry_begin:0,
            game_date:"2026-07-25".into(),profile:profiles::FM26.label.into(),own_club_id:None}
    }
    #[test]
    fn unknown_and_cross_edition_sessions_never_inherit_write_capabilities() {
        let mut s = session();
        assert!(require_scalar_write(&s).is_ok());
        assert!(require_legacy_feature(&s,"관계").is_ok());
        s.profile = profiles::FM27.label.into();
        assert!(require_scalar_write(&s).is_err());
        assert!(require_legacy_feature(&s,"관계").is_err());
        s.module_stamp = profiles::FM27.timestamp;
        s.module_size = profiles::FM27.image_size;
        assert!(require_legacy_feature(&s,"관계").is_err());
        s.module_stamp += 1;
        assert!(require_scalar_write(&s).is_err());
    }
    #[test]
    fn field_families_are_explicit_and_kind_qualified() {
        assert!(allowed_fm27_field(EntityKind::Player,"passing"));
        assert!(!allowed_fm27_field(EntityKind::Staff,"passing"));
        assert!(allowed_fm27_field(EntityKind::Staff,"staff_youth"));
        assert!(!allowed_fm27_field(EntityKind::Player,"staff_youth"));
        for key in ["trait_1","morale","retire_after_current_club","contract.wage","staff_buying_players","future_field"] {
            for kind in EntityKind::ALL { assert!(!allowed_fm27_field(kind,key)); }
        }
        assert!(!allowed_fm27_field(EntityKind::Club,"club.sugar_daddy"));
        assert!(allowed_fm27_field(EntityKind::City,"city.attraction"));
        for key in ["loyalty","person.dob"] { assert!(!allowed_fm27_field(EntityKind::Person,key)); }
        for key in ["staff_gk_handling","staff_gk_distribution","score_gk_distribution","score_youth_goalkeeper"] {
            assert!(!allowed_fm27_field(EntityKind::Staff,key));
        }
    }
}
