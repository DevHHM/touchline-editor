//! Local shortlist edits. Validate a copy before committing; never touch FM memory.
use super::*;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShortlistAction {
    Create { name: String, members: Vec<Ref> },
    Rename { name: String, new_name: String },
    Delete { name: String },
    Add { name: String, members: Vec<Ref> },
    Remove { name: String, members: Vec<Ref> },
}

impl Library {
    pub fn ensure_expected(&self, expected: &Library) -> Result<()> {
        ensure!(serde_json::to_value(self)? == serde_json::to_value(expected)?,
            "다른 창에서 관심명단 또는 프리셋이 변경되었습니다. 다시 읽은 뒤 작업하세요.");
        Ok(())
    }
    pub fn update_shortlist(&self, action: ShortlistAction) -> Result<Self> {
        let mut next = self.clone();
        let list = |name: &str| self.lists.iter().position(|l| l.name == name)
            .with_context(|| format!("관심명단을 찾을 수 없습니다: {name}"));
        match action {
            ShortlistAction::Create { name, members } => {
                next.lists.push(Shortlist { name: name.trim().to_owned(), members: unique(members)? });
            }
            ShortlistAction::Rename { name, new_name } => next.lists[list(&name)?].name = new_name.trim().to_owned(),
            ShortlistAction::Delete { name } => { next.lists.remove(list(&name)?); }
            ShortlistAction::Add { name, members } => {
                let i = list(&name)?;
                next.lists[i].members = unique(next.lists[i].members.iter().cloned().chain(members).collect())?;
            }
            ShortlistAction::Remove { name, members } => {
                let i = list(&name)?;
                let remove: HashSet<_> = unique(members)?.into_iter().collect();
                next.lists[i].members.retain(|m| !remove.contains(m));
            }
        }
        next.validate()?;
        Ok(next)
    }
    pub fn export_shortlist(&self, name: &str) -> Result<Self> {
        Ok(Self { lists: vec![self.lists.iter().find(|l| l.name == name)
            .context("내보낼 관심명단이 없습니다")?.clone()], presets: vec![] })
    }
}
fn unique(members: Vec<Ref>) -> Result<Vec<Ref>> {
    ensure!(members.len() <= 20000, "한 번에 처리할 대상이 너무 많습니다");
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for member in members {
        ensure!(member.id > 0, "명단 ID가 없습니다");
        if seen.insert(member.clone()) { result.push(member); }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn member(kind: EntityKind, id: u32) -> Ref { Ref { kind, id } }
    fn create() -> Library { Library::default().update_shortlist(ShortlistAction::Create {
        name: " 후보 ".into(), members: vec![member(EntityKind::Player, 10)] }).unwrap() }
    #[test] fn shortlist_create_add_remove_preserves_identity_and_order() {
        let l = create().update_shortlist(ShortlistAction::Add { name: "후보".into(), members: vec![
            member(EntityKind::Player, 10), member(EntityKind::Staff, 10), member(EntityKind::Player, 11)] }).unwrap();
        assert_eq!(l.lists[0].members.len(), 3);
        let n = l.update_shortlist(ShortlistAction::Remove { name: "후보".into(), members: vec![member(EntityKind::Player, 10)] }).unwrap();
        assert_eq!(n.lists[0].members, vec![member(EntityKind::Staff, 10), member(EntityKind::Player, 11)]);
        assert_eq!(l.lists[0].members.len(), 3);
    }
    #[test] fn shortlist_rename_delete_export_keep_other_lists_and_presets() {
        let mut l = create().update_shortlist(ShortlistAction::Create { name: "Other".into(), members: vec![] }).unwrap();
        l.presets.push(Preset { name: "test".into(), kind: EntityKind::Player,
            rules: vec![Rule { key: "player.ca".into(), operation: "set".into(), value: 100.0 }] });
        let l = l.update_shortlist(ShortlistAction::Rename { name: "후보".into(), new_name: " 주전 ".into() }).unwrap();
        assert_eq!(l.export_shortlist("주전").unwrap().lists.len(), 1);
        assert!(l.export_shortlist("주전").unwrap().presets.is_empty());
        let l = l.update_shortlist(ShortlistAction::Delete { name: "주전".into() }).unwrap();
        assert_eq!(l.lists[0].name, "Other"); assert_eq!(l.presets.len(), 1);
    }
    #[test] fn shortlist_rejects_invalid_without_partial_changes() {
        let l = create(); let before = serde_json::to_value(&l).unwrap();
        for action in [
            ShortlistAction::Create { name: "후보".into(), members: vec![] },
            ShortlistAction::Create { name: "  ".into(), members: vec![] },
            ShortlistAction::Add { name: "후보".into(), members: vec![member(EntityKind::Player, 0)] },
            ShortlistAction::Delete { name: "missing".into() },
            ShortlistAction::Add { name: "후보".into(), members: (1..=10001).map(|id| member(EntityKind::Player, id)).collect() },
        ] { assert!(l.update_shortlist(action).is_err()); assert_eq!(before, serde_json::to_value(&l).unwrap()); }
    }
    #[test] fn shortlist_stale_window_cannot_overwrite_newer_edits() {
        let old = create(); let new = old.update_shortlist(ShortlistAction::Add { name: "후보".into(), members: vec![member(EntityKind::Player, 12)] }).unwrap();
        assert!(new.ensure_expected(&old).is_err()); assert!(new.ensure_expected(&new).is_ok());
    }
}
