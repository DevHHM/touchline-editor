//! Explicitly synthetic offline data for relation engine and native UI tests.
use super::*;
use std::path::Path;

fn entity(id: u32, kind: EntityKind, name: &str, address: u64) -> Entity {
    Entity {
        nationality: String::new(),
        nationality_id: None,
        football_category: crate::model::FootballCategory::Unknown,
        id,
        kind,
        name: name.into(),
        english_name: format!("Synthetic {id}"),
        club: if kind == EntityKind::Club {
            name.into()
        } else {
            "합성 구단 A".into()
        },
        club_id: Some(if kind == EntityKind::Club { id } else { 44 }),
        age: Some(24),
        ca: Some(100),
        pa: Some(150),
        role: if kind == EntityKind::Player {
            "MC"
        } else {
            "코치"
        }
        .into(),
        group: "합성 자료".into(),
        address,
        base: address,
        index: Some(id),
    }
}
fn linked(uid: u32, name: &str, address: u64, object_type: u8) -> LinkedRecord {
    LinkedRecord {
        uid,
        name: name.into(),
        address,
        object_type,
    }
}
fn snapshot(entity: Entity, nation: LinkedRecord) -> PersonRelations {
    let names = [
        (NameField::First, "민준", 0x50),
        (NameField::Last, "검증", 0x58),
        (NameField::Common, "합성 인물", 0x60),
    ]
    .into_iter()
    .map(|(field, text, offset)| NameRecord {
        field,
        text: format!("{text}{}", entity.id),
        record: entity.address + 0x1000 + offset,
        text_object: entity.address + 0x2000 + offset,
    })
    .collect::<Vec<_>>();
    let mut guards = vec![Guard {
        address: entity.address + 12,
        expected: entity.id.to_le_bytes().to_vec(),
    }];
    guards.extend(names.iter().map(|name| Guard {
        address: entity.address + name.field.offset(),
        expected: name.record.to_le_bytes().to_vec(),
    }));
    PersonRelations {
        entity,
        names,
        primary_nationality: Some(nation),
        rows: vec![],
        spare_slots: 0,
        notes: vec!["합성 오프라인 테스트 자료입니다. 실제 게임 인물이 아닙니다".into()],
        holder: 0,
        begin: 0,
        end: 0,
        capacity: 0,
        guards,
        career: Some(CareerPlans {
            rows: vec![],
            holder: 0,
            begin: 0,
            end: 0,
            capacity: 0,
            reusable: None,
        allocation_available: false,
        allocation_reason: None,
        }),
        append_bytes: None,
        vector_growth_available: false,
        vector_growth_reason: None,
    }
}
pub(crate) fn data() -> (Index, Vec<Detail>, Vec<PersonRelations>) {
    let entities = vec![
        entity(7, EntityKind::Player, "합성 선수", 0x10000),
        entity(8, EntityKind::Player, "커리어 미지원 합성 선수", 0x20000),
        entity(17, EntityKind::Staff, "합성 코치", 0x30000),
        entity(9, EntityKind::Person, "빈 커리어 합성 인물", 0x40000),
        entity(44, EntityKind::Club, "합성 구단 A", 0x60000),
        entity(45, EntityKind::Club, "합성 구단 B", 0x61000),
        entity(
            10,
            EntityKind::Person,
            "삭제 보존 커리어 합성 인물",
            0x50000,
        ),
    ];
    let mut subject = snapshot(entities[0].clone(), linked(113, "합성 한국", 0x71000, 8));
    subject.holder = 0x80000;
    subject.begin = 0x81000;
    subject.end = 0x81020;
    subject.capacity = 0x81040;
    subject.spare_slots = 2;
    subject.append_bytes = Some(vec![0xAA; 16]);
    for (slot, target, kind, tail) in [
        (
            0,
            linked(44, "합성 구단 A", 0x60000, 1),
            3,
            [1, 0, 1, 3, 85, 78, 0, 255],
        ),
        (
            1,
            linked(112, "합성 잉글랜드", 0x72000, 8),
            9,
            NATIONALITY_TAIL,
        ),
    ] {
        let mut raw = target.address.to_le_bytes().to_vec();
        raw.extend(tail);
        subject.guards.push(Guard {
            address: subject.begin + slot as u64 * 16,
            expected: raw.clone(),
        });
        subject.rows.push(RelationRow {
            slot,
            kind,
            kind_label: kind_label(kind).into(),
            object_type: target.object_type,
            reason: u16::from_le_bytes([raw[8], raw[9]]),
            level: raw[12],
            target: Some(target),
            editable_favourite: kind == 3,
            editable_secondary_nationality: kind == 9,
            raw,
        });
    }
    subject.guards.push(Guard {
        address: subject.entity.address + 0x78,
        expected: subject.holder.to_le_bytes().to_vec(),
    });
    subject.guards.push(Guard {
        address: subject.holder,
        expected: [
            subject.begin.to_le_bytes(),
            subject.end.to_le_bytes(),
            subject.capacity.to_le_bytes(),
        ]
        .concat(),
    });
    subject.guards.push(Guard {
        address: subject.end,
        expected: subject.append_bytes.clone().unwrap(),
    });
    let mut career_raw = 0x60000u64.to_le_bytes().to_vec();
    career_raw.extend([1, 1, 1, 3, 2, 7, 165, 255]);
    let career = CareerPlans {
        rows: vec![CareerPlanRow {
            slot: 0,
            condition: 1,
            scope: 1,
            job: 1,
            object_type: 3,
            target: Some(linked(44, "합성 구단 A", 0x60000, 1)),
            editable_club: true,
            record: 0x92000,
            raw: career_raw.clone(),
        }],
        holder: 0x90000,
        begin: 0x91000,
        end: 0x91008,
        capacity: 0x91010,
        reusable: None,
        allocation_available: false,
        allocation_reason: None,
    };
    subject.guards.extend([
        Guard {
            address: subject.entity.address + 0xC8,
            expected: career.holder.to_le_bytes().to_vec(),
        },
        Guard {
            address: career.holder,
            expected: [
                career.begin.to_le_bytes(),
                career.end.to_le_bytes(),
                career.capacity.to_le_bytes(),
            ]
            .concat(),
        },
        Guard {
            address: career.begin,
            expected: career.rows[0].record.to_le_bytes().to_vec(),
        },
        Guard {
            address: career.rows[0].record,
            expected: career_raw,
        },
    ]);
    subject.career = Some(career);
    let mut unsupported = snapshot(entities[1].clone(), linked(113, "합성 한국", 0x71000, 8));
    unsupported.career = None;
    unsupported
        .notes
        .push("합성 미지원 사례: 커리어 계획 벡터 검증 실패".into());
    let mut donor = snapshot(
        entities[2].clone(),
        linked(111, "합성 이탈리아", 0x73000, 8),
    );
    // Empty active relations with allocated capacity: first favourite/nationality.
    donor.holder = 0xB0000;
    donor.begin = 0xB1000;
    donor.end = 0xB1000;
    donor.capacity = 0xB1020;
    donor.spare_slots = 2;
    donor.append_bytes = Some(vec![0xBB; 16]);
    donor.guards.extend([
        Guard {
            address: donor.entity.address + 0x78,
            expected: donor.holder.to_le_bytes().to_vec(),
        },
        Guard {
            address: donor.holder,
            expected: [
                donor.begin.to_le_bytes(),
                donor.end.to_le_bytes(),
                donor.capacity.to_le_bytes(),
            ]
            .concat(),
        },
        Guard {
            address: donor.end,
            expected: donor.append_bytes.clone().unwrap(),
        },
    ]);
    let mut blank = snapshot(entities[3].clone(), linked(113, "합성 한국", 0x71000, 8));
    let mut raw = vec![0; 8];
    raw.extend([1, 1, 0, 255, 2, 0, 0, 0]);
    blank.career = Some(CareerPlans {
        rows: vec![CareerPlanRow {
            slot: 0,
            condition: 1,
            scope: 1,
            job: 0,
            object_type: 255,
            target: None,
            editable_club: true,
            record: 0xA2000,
            raw: raw.clone(),
        }],
        holder: 0xA0000,
        begin: 0xA1000,
        end: 0xA1008,
        capacity: 0xA1010,
        reusable: None,
        allocation_available: false,
        allocation_reason: None,
    });
    blank.guards.extend([
        Guard {
            address: blank.entity.address + 0xC8,
            expected: 0xA0000u64.to_le_bytes().to_vec(),
        },
        Guard {
            address: 0xA0000,
            expected: [
                0xA1000u64.to_le_bytes(),
                0xA1008u64.to_le_bytes(),
                0xA1010u64.to_le_bytes(),
            ]
            .concat(),
        },
        Guard {
            address: 0xA1000,
            expected: 0xA2000u64.to_le_bytes().to_vec(),
        },
        Guard {
            address: 0xA2000,
            expected: raw,
        },
    ]);
    // Synthetic deletion-post-state. Live inspection never trusts an unused
    // pointer: it additionally requires the matching immutable deletion receipt.
    let mut reusable = snapshot(entities[6].clone(), linked(113, "합성 한국", 0x71000, 8));
    reusable.career = subject.career.clone();
    let career = reusable.career.as_mut().unwrap();
    let saved = career.rows.remove(0);
    career.end = career.begin;
    career.reusable = Some(saved);
    reusable.guards.extend([
        Guard {
            address: reusable.entity.address + 0xC8,
            expected: career.holder.to_le_bytes().to_vec(),
        },
        Guard {
            address: career.holder,
            expected: [
                career.begin.to_le_bytes(),
                career.end.to_le_bytes(),
                career.capacity.to_le_bytes(),
            ]
            .concat(),
        },
        Guard {
            address: career.end,
            expected: career
                .reusable
                .as_ref()
                .unwrap()
                .record
                .to_le_bytes()
                .to_vec(),
        },
        Guard {
            address: career.reusable.as_ref().unwrap().record,
            expected: career.reusable.as_ref().unwrap().raw.clone(),
        },
    ]);
    reusable
        .notes
        .push("합성 삭제 후 보존 상태: 실제 삭제 이력이 아닙니다".into());
    let details = entities
        .iter()
        .map(|entity| Detail {
            entity: entity.clone(),
            fields: [("ca", 100.), ("pa", 150.)]
                .into_iter()
                .map(|(key, value)| Field {
                    key: key.into(),
                    label: key.into(),
                    section: "합성 자료".into(),
                    value,
                    min: 0.,
                    max: 200.,
                    step: 1.,
                    hint: "오프라인 UI 검증용 합성 수치".into(),
                    binding: None,
                })
                .collect(),
            guards: vec![],
            notes: vec!["실제 게임 자료가 아닌 합성 테스트 자료".into()],
        })
        .collect();
    let index = Index {
        club_leagues: Vec::new(),
        session: Session {
            pid: 0,
            created: 0,
            module_base: 0,
            module_size: 0,
            module_stamp: 0,
            db: 0,
            registry_begin: 0,
            game_date: "2027-11-26".into(),
            profile: "SYNTHETIC OFFLINE RELATIONS FIXTURE — NOT FM".into(),
            own_club_id: Some(44),
        },
        entities,
        warnings: vec![
            "합성 오프라인 테스트 자료입니다. 실제 게임 인물·주소·수치가 아닙니다".into(),
        ],
    };
    (
        index,
        details,
        vec![subject, unsupported, donor, blank, reusable],
    )
}
pub(crate) fn write(folder: &Path) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    let (index, details, snapshots) = data();
    std::fs::write(
        folder.join("index.json"),
        serde_json::to_vec_pretty(&index)?,
    )?;
    for detail in details {
        std::fs::write(
            folder.join(format!(
                "detail-{:?}-{}.json",
                detail.entity.kind, detail.entity.id
            )),
            serde_json::to_vec_pretty(&detail)?,
        )?;
    }
    for snapshot in snapshots {
        std::fs::write(
            folder.join(format!(
                "relations-{:?}-{}.json",
                snapshot.entity.kind, snapshot.entity.id
            )),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
    }
    std::fs::write(
        folder.join("fixture-info.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"synthetic":true,"real_game_data":false,"game_io":false,"purpose":"Offline bridge and native WPF relation/career QA","subject":{"kind":"Player","id":7},"target_club_id":45,"donor":{"kind":"Staff","id":17},"expected_nation_uid":111,"blank_career":{"kind":"Person","id":9},"unsupported_career":{"kind":"Player","id":8}}),
        )?,
    )?;
    Ok(())
}
