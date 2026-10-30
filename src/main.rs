#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod assets;
mod api;
mod backend;
mod compatibility;
mod batch;
mod bridge;
mod diagnostics;
mod event_ui;
mod events;
mod extended;
mod extras;
mod traits;
mod workspace;
mod fields;
mod growth;
mod memory;
mod native_alloc;
mod name_localization;
mod model;
mod navigation;
mod overlay;
mod relations;
mod staff_review;
mod transaction;
mod typography;
mod ui;

fn main() {
    if let Err(e) = start() {
        eprintln!("{e:#}");
        let _ = std::fs::write(
            transaction::data_dir().join("last-error.txt"),
            format!("{e:#}"),
        );
        std::process::exit(1);
    }
}
fn start() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|s| s == "--api") {
        return api::run(&args[1..]);
    }
    if args.iter().any(|s| s == "--bridge") {
        return bridge::run();
    }
    if let Some(i) = args.iter().position(|s| s == "--overlay-preview") {
        let input = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--overlay-preview DETAIL OUTPUT.png"))?;
        let output = args
            .get(i + 2)
            .ok_or_else(|| anyhow::anyhow!("--overlay-preview DETAIL OUTPUT.png"))?;
        let detail: model::Detail = serde_json::from_slice(&std::fs::read(input)?)?;
        return overlay::run_preview(detail, output.into()).map_err(|e| anyhow::anyhow!("{e}"));
    }
    if let Some(i) = args.iter().position(|s| s == "--overlay-check") {
        let input = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--overlay-check INDEX DETAIL OUTPUT.json"))?;
        let detail_path = args
            .get(i + 2)
            .ok_or_else(|| anyhow::anyhow!("--overlay-check INDEX DETAIL OUTPUT.json"))?;
        let output = args
            .get(i + 3)
            .ok_or_else(|| anyhow::anyhow!("--overlay-check INDEX DETAIL OUTPUT.json"))?;
        let index: model::Index = serde_json::from_slice(&std::fs::read(input)?)?;
        let detail: model::Detail = serde_json::from_slice(&std::fs::read(detail_path)?)?;
        return overlay::run_native_check(index.session, detail, output.into())
            .map_err(|e| anyhow::anyhow!("{e}"));
    }
    if let Some(i) = args.iter().position(|s| s == "--preview-event") {
        let input = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--preview-event INDEX OUTPUT_DIRECTORY"))?;
        let output = std::path::PathBuf::from(
            args.get(i + 2)
                .ok_or_else(|| anyhow::anyhow!("--preview-event INDEX OUTPUT_DIRECTORY"))?,
        );
        let index: model::Index = serde_json::from_slice(&std::fs::read(input)?)?;
        let club = index
            .entities
            .iter()
            .find(|e| e.kind == model::EntityKind::Club && Some(e.id) == index.session.own_club_id)
            .ok_or_else(|| anyhow::anyhow!("내 구단 레코드 없음"))?;
        let context = events::capture(&index.session, club)?;
        let slot = context
            .sponsors
            .iter()
            .max_by_key(|s| s.annual_gbp)
            .map(|s| s.slot);
        let draft = events::EventDraft {
            sponsor_slot: slot,
            annual_gbp: if slot.is_some() { 26_653_522 } else { 0 },
            population_band: context.city.as_ref().map(|_| 8),
            attraction: context.city.as_ref().map(|_| 13),
            ..Default::default()
        };
        let preview = events::plan(&context, &draft)?;
        std::fs::create_dir_all(&output)?;
        std::fs::write(
            output.join("event-context.json"),
            serde_json::to_vec_pretty(&context)?,
        )?;
        std::fs::write(
            output.join("event-preview.json"),
            serde_json::to_vec_pretty(&preview)?,
        )?;
        std::fs::write(
            output.join("event-readonly-report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "read_only": true, "game_values_changed": false,
                "game_date": context.game_date, "club": club.name,
                "sponsors": context.sponsors.len(), "city": context.city.as_ref().map(|d| &d.entity.name),
                "planned_changes": preview.changes.len(), "warnings": preview.warnings,
            }))?,
        )?;
        return Ok(());
    }
    if let Some(i) = args.iter().position(|s| s == "--refresh-details") {
        let input = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--refresh-details INDEX OUTPUT_DIRECTORY"))?;
        let output = std::path::PathBuf::from(
            args.get(i + 2)
                .ok_or_else(|| anyhow::anyhow!("--refresh-details INDEX OUTPUT_DIRECTORY"))?,
        );
        let mut index: model::Index = serde_json::from_slice(&std::fs::read(input)?)?;
        backend::refresh_squad(&mut index)?;
        std::fs::create_dir_all(&output)?;
        let mut first = std::collections::HashSet::new();
        let mut checks = Vec::new();
        for entity in index
            .entities
            .iter()
            .filter(|e| {
                e.club_id == index.session.own_club_id
                    || (e.kind == model::EntityKind::Club
                        && Some(e.id) == index.session.own_club_id)
            })
            .take(400)
        {
            match backend::detail(&index.session, entity) {
                Ok(detail) => {
                    let bytes = serde_json::to_vec_pretty(&detail)?;
                    std::fs::write(output.join(format!("detail-{:?}-{}.json", entity.kind, entity.id)), &bytes)?;
                    if first.insert(entity.kind) {
                        std::fs::write(output.join(format!("detail-{:?}.json", entity.kind)), &bytes)?;
                    }
                    checks.push(serde_json::json!({"kind": entity.kind, "id": entity.id, "name":entity.name,"fields":detail.fields.len(),"status":"read_ok"}));
                }
                Err(error) => checks.push(serde_json::json!({"kind":entity.kind,"id":entity.id,"status":"error","message":format!("{error:#}")})),
            }
        }
        std::fs::write(
            output.join("index.json"),
            serde_json::to_vec_pretty(&index)?,
        )?;
        std::fs::write(
            output.join("details-report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"read_only":true,"checks":checks}))?,
        )?;
        return Ok(());
    }
    if args.iter().any(|s| s == "--memory-fixture") {
        return diagnostics::fixture();
    }
    if let Some(i) = args.iter().position(|s| s == "--refresh-squad") {
        let input = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--refresh-squad INPUT OUTPUT"))?;
        let output = args
            .get(i + 2)
            .ok_or_else(|| anyhow::anyhow!("--refresh-squad INPUT OUTPUT"))?;
        let mut index: model::Index = serde_json::from_slice(&std::fs::read(input)?)?;
        backend::refresh_squad(&mut index)?;
        std::fs::write(output, serde_json::to_vec_pretty(&index)?)?;
        return Ok(());
    }
    if let Some(i) = args.iter().position(|s| s == "--self-test-io") {
        let dest = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--self-test-io OUTPUT.json"))?;
        let report = diagnostics::memory_io()?;
        std::fs::write(dest, serde_json::to_vec_pretty(&report)?)?;
        return Ok(());
    }
    if let Some(i) = args.iter().position(|s| s == "--verify") {
        let dest = std::path::PathBuf::from(
            args.get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("--verify OUTPUT_DIRECTORY"))?,
        );
        std::fs::create_dir_all(&dest)?;
        let begin = std::time::Instant::now();
        let index = backend::connect(|s| eprintln!("{s}"))?;
        std::fs::write(dest.join("index.json"), serde_json::to_vec_pretty(&index)?)?;
        let mut checks = vec![];
        for kind in model::EntityKind::ALL {
            let own = index
                .entities
                .iter()
                .find(|e| e.kind == kind && e.club_id == index.session.own_club_id);
            if let Some(e) = own.or_else(|| index.entities.iter().find(|e| e.kind == kind)) {
                match backend::detail(&index.session,e) {
                    Ok(d)=>{
                        std::fs::write(dest.join(format!("detail-{kind:?}.json")),serde_json::to_vec_pretty(&d)?)?;
                        let mut fields_valid=true;
                        for f in &d.fields {
                            if f.binding.is_some() && transaction::encode_value(f,f.value).is_err(){fields_valid=false;}
                        }
                        checks.push(serde_json::json!({"kind":kind,"id":e.id,"name":e.name,"fields":d.fields.len(),"editable":d.fields.iter().filter(|f|f.binding.is_some()).count(),"bounds_valid":fields_valid,"guards":d.guards.len(),"status":"read_ok"}));
                    },
                    Err(err)=>checks.push(serde_json::json!({"kind":kind,"id":e.id,"status":"error","message":format!("{err:#}")})),
                }
            }
        }
        let report = serde_json::json!({"read_only":true,"game_date":index.session.game_date,"pid":index.session.pid,"records":index.entities.len(),"elapsed_seconds":begin.elapsed().as_secs_f64(),"checks":checks,"warnings":index.warnings});
        std::fs::write(
            dest.join("verification.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if let Some(i) = args.iter().position(|s| s == "--inspect") {
        let dest = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--inspect requires a JSON output path"))?;
        let index = backend::connect(|s| eprintln!("{s}"))?;
        std::fs::write(dest, serde_json::to_vec_pretty(&index)?)?;
        println!("{} records exported to {dest}", index.entities.len());
        return Ok(());
    }
    if let Some(i) = args.iter().position(|s| s == "--detail") {
        let uid: u32 = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--detail UID OUTPUT"))?
            .parse()?;
        let dest = args
            .get(i + 2)
            .ok_or_else(|| anyhow::anyhow!("--detail UID OUTPUT"))?;
        let index = backend::connect(|s| eprintln!("{s}"))?;
        let entity = index
            .entities
            .iter()
            .find(|e| e.id == uid)
            .ok_or_else(|| anyhow::anyhow!("UID not found"))?;
        let detail = backend::detail(&index.session, entity)?;
        std::fs::write(dest, serde_json::to_vec_pretty(&detail)?)?;
        return Ok(());
    }
    let snapshot = if let Some(i) = args.iter().position(|s| s == "--snapshot") {
        let path = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--snapshot requires a JSON path"))?;
        Some(serde_json::from_slice(&std::fs::read(path)?)?)
    } else {
        None
    };
    ui::run(snapshot).map_err(|e| anyhow::anyhow!("{e}"))
}
