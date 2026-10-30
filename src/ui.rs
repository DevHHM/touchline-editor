//! Native editor UI. All game operations are explicit, asynchronous user actions.
use crate::{
    assets::ImageAssets, backend, event_ui::EventStudio, model::*, overlay::CoachOverlay,
    transaction, typography,
};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const BG: Color32 = Color32::from_rgb(15, 23, 31);
const PANEL: Color32 = Color32::from_rgb(20, 31, 41);
const CARD: Color32 = Color32::from_rgb(26, 40, 51);
const LINE: Color32 = Color32::from_rgb(43, 60, 72);
const INK: Color32 = Color32::from_rgb(235, 237, 227);
const MUTED: Color32 = Color32::from_rgb(143, 161, 171);
const LIME: Color32 = Color32::from_rgb(199, 232, 114);
const AMBER: Color32 = Color32::from_rgb(238, 190, 111);
const RED: Color32 = Color32::from_rgb(239, 132, 135);

pub fn run(initial: Option<Index>) -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let size = args
        .windows(2)
        .find(|a| a[0] == "--ui-check-size")
        .and_then(|a| a[1].split_once('x'))
        .and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))
        .unwrap_or([1440.0, 920.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TOUCHLINE · FM26 Editor")
            .with_inner_size(size)
            .with_min_inner_size([1240.0, 760.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Touchline Editor",
        options,
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            Ok(Box::new(Editor::new(initial)))
        }),
    )
}

fn configure(ctx: &egui::Context) {
    // Windows may report a light system theme after startup. Keep this app's palette coherent.
    ctx.set_theme(egui::ThemePreference::Dark);
    typography::configure(ctx);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.faint_bg_color = Color32::from_rgb(23, 34, 44);
    style.visuals.override_text_color = Some(INK);
    style.visuals.selection.bg_fill = Color32::from_rgb(49, 69, 52);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, LIME);
    style.visuals.widgets.inactive.bg_fill = CARD;
    style.visuals.widgets.inactive.weak_bg_fill = CARD;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(41, 58, 68);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(41, 58, 68);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, MUTED);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(61, 79, 49);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, LIME);
    style.visuals.window_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.window_corner_radius = 6.into();
    style.visuals.menu_corner_radius = 4.into();
    style.spacing.item_spacing = Vec2::new(7.0, 4.0);
    style.spacing.button_padding = Vec2::new(9.0, 4.0);
    style.spacing.interact_size.y = 25.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, typography::title(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, typography::title(22.0));
    ctx.set_style(style);
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Sort {
    Name,
    Id,
    Age,
    Club,
    Ca,
    Pa,
}
#[derive(Serialize, Deserialize)]
struct Preferences {
    kind: EntityKind,
    mine: bool,
    sort: Sort,
    descending: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            kind: EntityKind::Player,
            mine: true,
            sort: Sort::Ca,
            descending: true,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Connect,
    Detail,
    Apply,
    Undo,
    Export,
}
enum Event {
    Progress(u64, String),
    Connected(u64, Result<Index, String>),
    Detail(u64, Result<Detail, String>),
    Applied(u64, Result<Receipt, String>),
    Undone(u64, Result<Receipt, String>),
    Exported(u64, Result<PathBuf, String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Route {
    List {
        kind: EntityKind,
        club_id: Option<u32>,
    },
    Profile {
        index: usize,
        tab: String,
        origin_club: Option<u32>,
    },
    Events {
        club_id: Option<u32>,
    },
}

struct Editor {
    index: Option<Arc<Index>>,
    route: Route,
    routes: Vec<Route>,
    route_cursor: usize,
    pending_route: Option<Route>,
    pending_history: Option<usize>,
    club_scope: Option<u32>,
    detail_cache: HashMap<(EntityKind, u32), Detail>,
    assets: ImageAssets,
    event_studio: EventStudio,
    coach_overlay: CoachOverlay,
    show_overlay: bool,
    show_assets: bool,
    asset_roots: String,
    precise_values: bool,
    qa_route: Option<String>,
    qa_route_done: bool,
    connected: bool,
    prefs: Preferences,
    search: String,
    ca_min: u16,
    pa_min: u16,
    squad_filter: u8,
    member_group: String,
    filtered: Vec<usize>,
    dirty: bool,
    selected: Option<usize>,
    detail: Option<Detail>,
    draft: BTreeMap<String, f64>,
    section: String,
    field_search: String,
    busy: Option<Busy>,
    progress: String,
    notice: Option<(String, bool)>,
    generation: u64,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    history: Vec<Receipt>,
    show_history: bool,
    show_review: bool,
    undo_target: Option<Receipt>,
    select_after_discard: Option<usize>,
    connect_after_discard: bool,
    average_ca: String,
    highest_pa: String,
    own_club_name: String,
    qa_output: Option<PathBuf>,
    qa_frames: u32,
    qa_requested: bool,
}

impl Editor {
    fn new(initial: Option<Index>) -> Self {
        let mut prefs: Preferences = std::fs::read(app_dir().join("preferences.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        let notice = None;
        let own_club_name = initial.as_ref().map(own_name).unwrap_or_default();
        let args = std::env::args().collect::<Vec<_>>();
        let arg_path = |name: &str| {
            args.windows(2)
                .find(|a| a[0] == name)
                .map(|a| PathBuf::from(&a[1]))
        };
        let preview: Option<Detail> = arg_path("--preview-detail")
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok());
        let selected = preview.as_ref().and_then(|d| {
            initial.as_ref().and_then(|i| {
                i.entities
                    .iter()
                    .position(|e| e.id == d.entity.id && e.kind == d.entity.kind)
            })
        });
        let preview = preview.filter(|_| selected.is_some());
        if let Some(detail) = &preview {
            prefs.kind = detail.entity.kind;
            if prefs.kind == EntityKind::City {
                prefs.mine = false;
            }
        }
        let section = preview.as_ref().map(default_section).unwrap_or_default();
        let mut detail_cache = HashMap::new();
        for folder in [
            arg_path("--snapshot").and_then(|p| p.parent().map(|p| p.to_path_buf())),
            arg_path("--preview-detail").and_then(|p| p.parent().map(|p| p.to_path_buf())),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(entries) = std::fs::read_dir(folder) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("detail-"))
                    {
                        if let Ok(bytes) = std::fs::read(&p) {
                            if let Ok(d) = serde_json::from_slice::<Detail>(&bytes) {
                                detail_cache.insert((d.entity.kind, d.entity.id), d);
                            }
                        }
                    }
                }
            }
        }
        if let Some(d) = &preview {
            detail_cache.insert((d.entity.kind, d.entity.id), d.clone());
        }
        let tab = args
            .windows(2)
            .find(|a| a[0] == "--preview-tab")
            .map(|a| a[1].clone())
            .unwrap_or_else(|| "개요".into());
        let route = if args.iter().any(|a| a == "--preview-events") {
            Route::Events {
                club_id: initial.as_ref().and_then(|i| i.session.own_club_id),
            }
        } else {
            selected
                .map(|index| Route::Profile {
                    index,
                    tab,
                    origin_club: None,
                })
                .unwrap_or(Route::List {
                    kind: prefs.kind,
                    club_id: None,
                })
        };
        let qa_route = args
            .windows(2)
            .find(|a| a[0] == "--ui-route-check")
            .map(|a| a[1].clone());
        Self {
            index: initial.map(Arc::new),
            route: route.clone(),
            routes: vec![route],
            route_cursor: 0,
            pending_route: None,
            pending_history: None,
            club_scope: None,
            detail_cache,
            assets: ImageAssets::new(),
            event_studio: EventStudio::new(),
            coach_overlay: CoachOverlay::default(),
            show_overlay: false,
            show_assets: false,
            asset_roots: String::new(),
            precise_values: false,
            qa_route,
            qa_route_done: false,
            connected: false,
            prefs,
            search: String::new(),
            ca_min: 0,
            pa_min: 0,
            squad_filter: 1,
            member_group: String::new(),
            filtered: vec![],
            dirty: true,
            selected,
            detail: preview,
            draft: BTreeMap::new(),
            section,
            field_search: String::new(),
            busy: None,
            progress: String::new(),
            notice,
            generation: 0,
            tx,
            rx,
            history: transaction::history(),
            show_history: false,
            show_review: false,
            undo_target: None,
            select_after_discard: None,
            connect_after_discard: false,
            average_ca: "—".into(),
            highest_pa: "—".into(),
            own_club_name,
            qa_output: arg_path("--ui-check"),
            qa_frames: 0,
            qa_requested: false,
        }
    }

    fn begin(&mut self, busy: Busy, message: &str) -> u64 {
        self.generation += 1;
        self.busy = Some(busy);
        self.progress = message.to_string();
        self.notice = None;
        self.generation
    }

    fn connect(&mut self) {
        if self.busy.is_some() || self.event_studio.is_busy() {
            return;
        }
        let generation = self.begin(
            Busy::Connect,
            "FM 프로세스와 데이터 버전을 확인하고 있습니다…",
        );
        self.connected = false;
        self.event_studio.reset();
        self.coach_overlay.clear();
        self.detail = None;
        self.selected = None;
        self.draft.clear();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = backend::connect(move |s| {
                let _ = progress_tx.send(Event::Progress(generation, s));
            });
            let _ = tx.send(Event::Connected(
                generation,
                result.map_err(|e| format!("{e:#}")),
            ));
        });
    }

    fn request_connect(&mut self) {
        if self.draft.is_empty() {
            self.connect();
        } else {
            self.connect_after_discard = true;
        }
    }

    fn select(&mut self, index: usize) {
        let origin_club = self
            .index
            .as_ref()
            .and_then(|i| i.entities.get(index))
            .filter(|e| e.kind == EntityKind::City && matches!(self.route, Route::Profile { .. }))
            .and_then(|_| {
                self.current_entity()
                    .filter(|e| e.kind == EntityKind::Club)
                    .map(|e| e.id)
            });
        self.navigate(Route::Profile {
            index,
            tab: "개요".into(),
            origin_club,
        });
    }
    fn navigate(&mut self, route: Route) {
        if self.busy.is_some() || self.event_studio.is_busy() || self.route == route {
            return;
        }
        if !self.draft.is_empty() {
            self.pending_route = Some(route);
            return;
        }
        self.routes.truncate(self.route_cursor + 1);
        self.routes.push(route.clone());
        self.route_cursor = self.routes.len() - 1;
        self.activate_route(route);
    }
    fn history_go(&mut self, cursor: usize) {
        if self.busy.is_some() || self.event_studio.is_busy() || cursor >= self.routes.len() {
            return;
        }
        if !self.draft.is_empty() {
            self.pending_history = Some(cursor);
            return;
        }
        self.route_cursor = cursor;
        self.activate_route(self.routes[cursor].clone());
    }
    fn activate_route(&mut self, route: Route) {
        if let Some(d) = &self.detail {
            self.detail_cache
                .insert((d.entity.kind, d.entity.id), d.clone());
        }
        self.route = route.clone();
        self.field_search.clear();
        match route {
            Route::Profile { index, .. } => {
                self.selected = Some(index);
                self.detail = None;
                if let Some(e) = self.index.as_ref().and_then(|i| i.entities.get(index)) {
                    self.prefs.kind = e.kind;
                    self.detail = self.detail_cache.get(&(e.kind, e.id)).cloned();
                }
                if self.connected {
                    self.load_detail();
                }
            }
            Route::List { kind, club_id } => {
                self.prefs.kind = kind;
                self.club_scope = club_id;
                self.ca_min = 0;
                self.pa_min = 0;
                self.search.clear();
                if club_id.is_some() || kind == EntityKind::City {
                    self.prefs.mine = false;
                }
                self.dirty = true;
            }
            Route::Events { club_id } => {
                self.event_studio.enter_club(club_id, self.connected);
            }
        }
    }
    fn open_entity(&mut self, kind: EntityKind, id: u32) {
        if let Some(i) = self
            .index
            .as_ref()
            .and_then(|i| i.entities.iter().position(|e| e.kind == kind && e.id == id))
        {
            self.select(i);
        }
    }
    fn current_entity(&self) -> Option<Entity> {
        self.index
            .as_ref()
            .and_then(|i| self.selected.and_then(|s| i.entities.get(s)))
            .cloned()
    }
    fn set_tab(&mut self, tab: &str) {
        if let Route::Profile { tab: t, .. } = &mut self.route {
            *t = tab.into();
            self.routes[self.route_cursor] = self.route.clone();
        }
    }

    fn load_detail(&mut self) {
        if !self.draft.is_empty() || self.event_studio.is_busy() {
            return;
        }
        let Some(index) = self.index.as_ref() else {
            return;
        };
        let Some(entity) = self.selected.and_then(|i| index.entities.get(i)).cloned() else {
            return;
        };
        if !self.connected {
            return;
        }
        let session = index.session.clone();
        let generation = self.begin(Busy::Detail, "선택한 대상의 현재 값을 읽고 있습니다…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = backend::detail(&session, &entity).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Detail(generation, result));
        });
    }

    fn apply(&mut self) {
        if !self.connected
            || self.busy.is_some()
            || self.event_studio.is_busy()
            || self.draft.is_empty()
        {
            return;
        }
        let (Some(index), Some(detail)) = (&self.index, &self.detail) else {
            return;
        };
        let session = index.session.clone();
        let detail = detail.clone();
        let edits = self
            .draft
            .iter()
            .map(|(key, value)| Edit {
                key: key.clone(),
                value: *value,
            })
            .collect::<Vec<_>>();
        let generation = self.begin(Busy::Apply, "원본 검증 · 변경 백업 · 적용 후 확인 중…");
        self.show_review = false;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result =
                transaction::apply(&session, &detail, &edits).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Applied(generation, result));
        });
    }

    fn undo(&mut self, receipt: Receipt) {
        if !self.connected
            || self.busy.is_some()
            || self.event_studio.is_busy()
            || !self.draft.is_empty()
        {
            return;
        }
        let generation = self.begin(Busy::Undo, "변경 이력의 원본 값을 복원하고 있습니다…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = transaction::undo(&receipt).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Undone(generation, result));
        });
    }

    fn export(&mut self) {
        if self.busy.is_some() || self.event_studio.is_busy() {
            return;
        }
        let Some(index) = self.index.clone() else {
            return;
        };
        let generation = self.begin(
            Busy::Export,
            "조회 결과를 JSON 스냅샷으로 저장하고 있습니다…",
        );
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<PathBuf> {
                let dir = app_dir().join("snapshots");
                std::fs::create_dir_all(&dir)?;
                let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                let path = dir.join(format!("fm26-{stamp}.json"));
                let file = std::fs::File::create(&path)?;
                serde_json::to_writer(std::io::BufWriter::new(file), &*index)?;
                Ok(path)
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Exported(generation, result));
        });
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Progress(id, value) if id == self.generation => self.progress = value,
                Event::Connected(id, result) if id == self.generation => {
                    self.busy = None;
                    match result {
                        Ok(index) => {
                            let count = index.entities.len();
                            self.own_club_name = own_name(&index);
                            self.prefs.mine = index.session.own_club_id.is_some();
                            self.index = Some(Arc::new(index));
                            self.connected = true;
                            self.dirty = true;
                            self.detail_cache.clear();
                            self.route = Route::List {
                                kind: self.prefs.kind,
                                club_id: None,
                            };
                            self.routes = vec![self.route.clone()];
                            self.route_cursor = 0;
                            self.club_scope = None;
                            self.notice = Some((
                                format!("연결 완료 · {count}개 대상을 조회했습니다."),
                                false,
                            ));
                        }
                        Err(error) => {
                            self.notice = Some((format!("연결하지 못했습니다. {error}"), true))
                        }
                    }
                }
                Event::Detail(id, result) if id == self.generation => {
                    self.busy = None;
                    match result {
                        Ok(detail) => {
                            if !detail.fields.iter().any(|f| f.section == self.section) {
                                self.section = default_section(&detail);
                            }
                            self.detail_cache
                                .insert((detail.entity.kind, detail.entity.id), detail.clone());
                            self.detail = Some(detail);
                            self.draft.clear();
                        }
                        Err(error) => {
                            self.detail = None;
                            self.notice = Some((error, true));
                        }
                    }
                }
                Event::Applied(id, result) | Event::Undone(id, result) if id == self.generation => {
                    self.busy = None;
                    match result {
                        Ok(receipt) => {
                            self.update_summary(&receipt);
                            let warning = receipt.status.contains("warning");
                            let message = if warning {
                                format!(
                                    "값은 변경됐지만 기록 저장 또는 게임 재개 확인이 필요합니다. {}",
                                    receipt.status
                                )
                            } else {
                                format!(
                                    "{} · {}개 항목 확인 완료",
                                    receipt.entity.name,
                                    receipt.changes.len()
                                )
                            };
                            self.history = transaction::history();
                            self.draft.clear();
                            self.load_detail();
                            self.notice = Some((message, warning));
                        }
                        Err(error) => {
                            self.notice =
                                Some((format!("변경을 완료하지 못했습니다. {error}"), true))
                        }
                    }
                }
                Event::Exported(id, result) if id == self.generation => {
                    self.busy = None;
                    self.notice = Some(match result {
                        Ok(path) => (format!("스냅샷 저장 완료: {}", path.display()), false),
                        Err(error) => (error, true),
                    });
                }
                _ => {} // A result from an older selection/session never changes the current editor.
            }
        }
    }

    fn update_summary(&mut self, receipt: &Receipt) {
        if let Some(index) = self.index.as_mut() {
            let index = Arc::make_mut(index);
            if let Some(entity) = index
                .entities
                .iter_mut()
                .find(|e| e.id == receipt.entity.id && e.kind == receipt.entity.kind)
            {
                for change in &receipt.changes {
                    match change.key.to_lowercase().as_str() {
                        "ca" | "current_ability" => entity.ca = Some(change.after as u16),
                        "pa" | "potential_ability" => entity.pa = Some(change.after as u16),
                        _ => {}
                    }
                }
            }
            self.dirty = true;
        }
    }

    fn rebuild_filter(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.filtered.clear();
        let Some(index) = &self.index else {
            return;
        };
        let needle = self.search.trim().to_lowercase();
        self.filtered
            .extend(index.entities.iter().enumerate().filter_map(|(i, e)| {
                if e.kind != self.prefs.kind {
                    return None;
                }
                if self.club_scope.is_some() && e.club_id != self.club_scope {
                    return None;
                }
                if e.kind == EntityKind::Player && self.prefs.mine {
                    let keep = match self.squad_filter {
                        1 => e.group.starts_with("1군"),
                        2 => {
                            e.group.contains("U-")
                                || e.group.contains("유소년")
                                || e.group.contains("청소년")
                        }
                        3 => e.group.contains("임대"),
                        _ => true,
                    };
                    if !keep {
                        return None;
                    }
                }
                if self.prefs.mine {
                    let own = index.session.own_club_id;
                    let ours = own.is_some()
                        && (e.club_id == own || (e.kind == EntityKind::Club && Some(e.id) == own));
                    if !ours {
                        return None;
                    }
                }
                if e.ca.unwrap_or_default() < self.ca_min || e.pa.unwrap_or_default() < self.pa_min
                {
                    return None;
                }
                if !needle.is_empty()
                    && !e.name.to_lowercase().contains(&needle)
                    && !e.english_name.to_lowercase().contains(&needle)
                    && !e.club.to_lowercase().contains(&needle)
                    && !e.id.to_string().contains(&needle)
                {
                    return None;
                }
                Some(i)
            }));
        let sort = self.prefs.sort;
        let desc = self.prefs.descending;
        self.filtered.sort_unstable_by(|a, b| {
            let (a, b) = (&index.entities[*a], &index.entities[*b]);
            let order = match sort {
                Sort::Name => a.name.cmp(&b.name),
                Sort::Id => a.id.cmp(&b.id),
                Sort::Age => a.age.cmp(&b.age),
                Sort::Club => a.club.cmp(&b.club),
                Sort::Ca => a.ca.cmp(&b.ca),
                Sort::Pa => a.pa.cmp(&b.pa),
            }
            .then_with(|| a.id.cmp(&b.id));
            if desc { order.reverse() } else { order }
        });
        let (mut sum, mut count, mut highest) = (0u64, 0u64, None::<u16>);
        for i in &self.filtered {
            let entity = &index.entities[*i];
            if let Some(ca) = entity.ca {
                sum += ca as u64;
                count += 1;
            }
            if let Some(pa) = entity.pa {
                highest = Some(highest.unwrap_or_default().max(pa));
            }
        }
        self.average_ca = if count == 0 {
            "—".into()
        } else {
            format!("{:.1}", sum as f64 / count as f64)
        };
        self.highest_pa = highest.map(|v| v.to_string()).unwrap_or_else(|| "—".into());
    }

    fn side_nav(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(164.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(10))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new("TOUCHLINE")
                        .font(typography::title(18.0))
                        .color(INK),
                );
                eyebrow(ui, "FM26 EDITOR");
                let compact = true;
                ui.add_space(8.0);
                if let Some(id) = self.index.as_ref().and_then(|i| i.session.own_club_id) {
                    let name = self.own_club_name.clone();
                    egui::Frame::new()
                        .fill(CARD)
                        .corner_radius(4)
                        .inner_margin(7)
                        .show(ui, |ui| {
                            if compact {
                                ui.horizontal(|ui| {
                                    self.entity_image(ui, EntityKind::Club, id, Vec2::splat(34.0));
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(short_club(&name)).strong(),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        self.open_entity(EntityKind::Club, id);
                                    }
                                });
                            } else {
                                ui.vertical_centered(|ui| {
                                    self.entity_image(ui, EntityKind::Club, id, Vec2::splat(58.0));
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(short_club(&name))
                                                    .strong()
                                                    .size(13.0),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        self.open_entity(EntityKind::Club, id);
                                    }
                                    ui.label(RichText::new("MY CLUB").size(9.0).color(MUTED));
                                });
                            }
                        });
                    ui.add_space(10.0);
                }
                eyebrow(ui, "데이터베이스");
                for (mark, kind, label) in [
                    ("01", EntityKind::Player, "선수단"),
                    ("02", EntityKind::Staff, "코치 · 스태프"),
                    ("03", EntityKind::Club, "구단"),
                    ("04", EntityKind::Person, "기타 인물"),
                    ("05", EntityKind::City, "도시"),
                ] {
                    let active =
                        !matches!(self.route, Route::Events { .. }) && self.prefs.kind == kind;
                    let button = egui::Button::new(
                        RichText::new(format!("{mark}   {label}"))
                            .size(13.0)
                            .color(if active { LIME } else { INK }),
                    )
                    .fill(if active {
                        Color32::from_rgb(38, 55, 44)
                    } else {
                        PANEL
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(4)
                    .min_size(Vec2::new(144.0, 30.0));
                    if ui
                        .add_enabled(self.busy.is_none() && !self.event_studio.is_busy(), button)
                        .clicked()
                    {
                        if kind == EntityKind::Club {
                            if let Some(id) =
                                self.index.as_ref().and_then(|i| i.session.own_club_id)
                            {
                                self.open_entity(kind, id)
                            } else {
                                self.navigate(Route::List {
                                    kind,
                                    club_id: None,
                                });
                            }
                        } else {
                            self.navigate(Route::List {
                                kind,
                                club_id: None,
                            });
                        }
                    }
                }
                if let Route::Profile { tab, .. } = self.route.clone() {
                    if let Some(entity) = self.current_entity() {
                        ui.add_space(10.0);
                        ui.separator();
                        eyebrow(ui, "상세 메뉴");
                        for target in profile_tabs(entity.kind) {
                            if ui
                                .add_enabled(
                                    self.busy.is_none() && !self.event_studio.is_busy(),
                                    egui::Button::new(
                                        RichText::new(format!("  {}", target))
                                            .size(12.0)
                                            .color(if tab == *target { LIME } else { MUTED }),
                                    )
                                    .fill(if tab == *target { CARD } else { PANEL })
                                    .stroke(Stroke::NONE)
                                    .min_size(Vec2::new(142.0, 22.0)),
                                )
                                .clicked()
                            {
                                self.set_tab(target);
                            }
                        }
                    }
                }
                ui.add_space(12.0);
                eyebrow(ui, "시나리오 · 도구");
                let active = matches!(self.route, Route::Events { .. });
                if ui
                    .add_enabled(
                        self.busy.is_none() && !self.event_studio.is_busy(),
                        egui::Button::new(RichText::new("+    이벤트").color(if active {
                            LIME
                        } else {
                            INK
                        }))
                        .fill(if active {
                            Color32::from_rgb(38, 55, 44)
                        } else {
                            PANEL
                        })
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::new(144.0, 28.0)),
                    )
                    .clicked()
                {
                    self.navigate(Route::Events {
                        club_id: self.index.as_ref().and_then(|i| i.session.own_club_id),
                    });
                }
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new("↶    변경 이력")
                            .fill(PANEL)
                            .stroke(Stroke::NONE),
                    )
                    .clicked()
                {
                    self.history = transaction::history();
                    self.show_history = true;
                }
                if ui
                    .add(
                        egui::Button::new(if self.coach_overlay.is_enabled() {
                            RichText::new("▣    코치 오버레이 · ON").color(LIME)
                        } else {
                            RichText::new("▣    코치 오버레이")
                        })
                        .fill(PANEL)
                        .stroke(Stroke::NONE),
                    )
                    .clicked()
                {
                    self.show_overlay = true;
                }
                if ui
                    .add_enabled(
                        self.index.is_some()
                            && (self.busy.is_none() && !self.event_studio.is_busy()),
                        egui::Button::new("↓    스냅샷 저장")
                            .fill(PANEL)
                            .stroke(Stroke::NONE),
                    )
                    .clicked()
                {
                    self.export();
                }
                let gap = ui.available_height() - 80.0;
                if gap > 0.0 {
                    ui.add_space(gap);
                }
                ui.separator();
                status_dot(
                    ui,
                    if self.connected {
                        "FM 연결됨"
                    } else if self.index.is_some() {
                        "스냅샷 조회 중"
                    } else {
                        "연결 대기"
                    },
                    self.connected,
                );
                if let Some(index) = &self.index {
                    ui.label(
                        RichText::new(&index.session.game_date)
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("사진 · 로고 설정").size(10.0).color(MUTED),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.asset_roots = self
                        .assets
                        .roots()
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.show_assets = true;
                }
            });
    }

    fn footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("footer")
            .exact_height(31.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy.is_some() {
                        busy_mark(ui);
                        ui.label(RichText::new(&self.progress).size(11.0).color(LIME));
                    } else {
                        ui.label(
                            RichText::new(if self.connected {
                                "현재 FM 데이터 · 변경 검토 후 적용"
                            } else {
                                "오프라인 조회 · 게임을 수정하지 않습니다"
                            })
                            .size(11.0)
                            .color(MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("TOUCHLINE / 0.3.1").size(10.0).color(MUTED));
                    });
                });
            });
    }

    fn content(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.route_cursor > 0
                                && (self.busy.is_none() && !self.event_studio.is_busy()),
                            egui::Button::new("‹").min_size(Vec2::new(30.0, 29.0)),
                        )
                        .on_hover_text("이전 화면")
                        .clicked()
                    {
                        self.history_go(self.route_cursor - 1);
                    }
                    if ui
                        .add_enabled(
                            self.route_cursor + 1 < self.routes.len()
                                && (self.busy.is_none() && !self.event_studio.is_busy()),
                            egui::Button::new("›").min_size(Vec2::new(30.0, 29.0)),
                        )
                        .on_hover_text("다음 화면")
                        .clicked()
                    {
                        self.history_go(self.route_cursor + 1);
                    }
                    let route = self.route.clone();
                    if ui
                        .link(
                            RichText::new(self.prefs.kind.label())
                                .color(MUTED)
                                .size(12.0),
                        )
                        .clicked()
                    {
                        self.navigate(Route::List {
                            kind: self.prefs.kind,
                            club_id: None,
                        });
                    }
                    match route {
                        Route::Profile { .. } => {
                            if let Some(e) = self.current_entity() {
                                ui.label(RichText::new("/").color(MUTED));
                                ui.label(RichText::new(e.name).size(12.0));
                            }
                        }
                        Route::Events { .. } => {
                            ui.label(RichText::new("/ 이벤트 스튜디오").size(12.0));
                        }
                        _ => {}
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                self.busy.is_none() && !self.event_studio.is_busy(),
                                primary(if self.connected {
                                    "FM 새로 연결"
                                } else {
                                    "FM 연결"
                                }),
                            )
                            .clicked()
                        {
                            self.request_connect();
                        }
                        if !self.draft.is_empty() {
                            if ui
                                .add_enabled(
                                    self.busy.is_none() && !self.event_studio.is_busy(),
                                    primary(&format!("변경 {}개 검토", self.draft.len())),
                                )
                                .clicked()
                            {
                                self.show_review = true;
                            }
                        }
                        if self.index.is_some() {
                            pill(
                                ui,
                                if self.connected { "LIVE" } else { "SNAPSHOT" },
                                if self.connected { LIME } else { AMBER },
                            );
                        }
                    });
                });
                ui.add_space(4.0);
                ui.separator();
                self.recent_records(ui);
                ui.add_space(5.0);
                if let Some((notice, error)) = &self.notice {
                    let text = notice.clone();
                    let error = *error;
                    let mut close = false;
                    egui::Frame::new()
                        .fill(if error {
                            Color32::from_rgb(54, 34, 41)
                        } else {
                            Color32::from_rgb(28, 43, 39)
                        })
                        .inner_margin(egui::Margin::symmetric(10, 7))
                        .corner_radius(6)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(text).size(11.0).color(if error {
                                            RED
                                        } else {
                                            LIME
                                        }),
                                    )
                                    .truncate(),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        close = ui.small_button("×").clicked();
                                    },
                                );
                            });
                        });
                    if close {
                        self.notice = None;
                    }
                    ui.add_space(7.0);
                }
                if self.index.is_none() {
                    self.welcome(ui);
                    return;
                }
                match self.route.clone() {
                    Route::List { .. } => self.list_page(ui),
                    Route::Profile { tab, .. } => self.profile_page(ui, &tab),
                    Route::Events { club_id } => self.events_page(ui, club_id),
                }
            });
    }
    fn recent_records(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.index.clone() else {
            return;
        };
        let mut seen = std::collections::HashSet::new();
        let records = self
            .routes
            .iter()
            .rev()
            .filter_map(|route| match route {
                Route::Profile { index: idx, .. } => index.entities.get(*idx).map(|e| (*idx, e)),
                _ => None,
            })
            .filter(|(_, e)| seen.insert((e.kind, e.id)))
            .take(7)
            .collect::<Vec<_>>();
        if records.len() < 2 {
            return;
        }
        egui::ScrollArea::horizontal()
            .id_salt("recent_record_strip")
            .max_height(30.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("열어본 항목").size(10.0).color(MUTED));
                    for (idx, e) in records.iter().rev() {
                        let active = self.selected == Some(*idx)
                            && matches!(self.route, Route::Profile { .. });
                        if ui
                            .add_enabled(
                                self.busy.is_none() && !self.event_studio.is_busy(),
                                egui::Button::new(
                                    RichText::new(&e.name).size(11.0).color(if active {
                                        LIME
                                    } else {
                                        MUTED
                                    }),
                                )
                                .fill(if active { CARD } else { BG })
                                .stroke(Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.select(*idx);
                        }
                    }
                });
            });
    }
    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(55.0);
        eyebrow(ui, "YOUR FOOTBALL. YOUR WORLD.");
        ui.label(
            RichText::new("구단의 다음 이야기를\n직접 만들어 보세요.")
                .size(37.0)
                .strong(),
        );
        ui.add_space(12.0);
        ui.label(
            RichText::new("선수의 재능부터 구단의 미래까지. FM26과 연결해 시작합니다.")
                .size(15.0)
                .color(MUTED),
        );
        ui.add_space(25.0);
        ui.columns(3, |cols| {
            feature_card(
                &mut cols[0],
                "01 / PLAYER PROFILE",
                "경기장 위의 모든 재능",
                "사진 · 능력치 · 성격 · 포지션",
            );
            feature_card(
                &mut cols[1],
                "02 / CLUB CENTRE",
                "구단을 한곳에서",
                "선수단 · 스태프 · 재정 · 연고 도시",
            );
            feature_card(
                &mut cols[2],
                "03 / EVENT STUDIO",
                "새로운 구단의 이야기",
                "스폰서 · 재정 지원 · 도시 성장",
            );
        });
    }
    fn list_page(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.index.clone() else {
            return;
        };
        self.rebuild_filter();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                eyebrow(ui, "DATABASE / OVERVIEW");
                ui.label(
                    RichText::new(match self.prefs.kind {
                        EntityKind::Player => "선수단",
                        EntityKind::Staff => "코치 · 스태프",
                        EntityKind::Club => "구단 데이터베이스",
                        EntityKind::City => "도시 데이터베이스",
                        EntityKind::Person => "인물 데이터베이스",
                    })
                    .size(23.0)
                    .strong(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                pill(ui, &format!("{} 명", self.filtered.len()), LIME);
                if matches!(self.prefs.kind, EntityKind::Player | EntityKind::Staff) {
                    ui.label(
                        RichText::new(format!(
                            "평균 CA {}   ·   최고 PA {}",
                            self.average_ca, self.highest_pa
                        ))
                        .size(12.0)
                        .color(MUTED),
                    );
                }
            });
        });
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(8)
            .corner_radius(4)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 350.0).max(250.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("이름, 영문명, 구단 또는 ID 검색")
                                .id(egui::Id::new("database_search"))
                                .desired_width(w),
                        )
                        .changed()
                    {
                        self.dirty = true;
                    }
                    let old = self.prefs.mine;
                    egui::ComboBox::from_id_salt("club_filter")
                        .width(128.0)
                        .selected_text(if self.club_scope.is_some() {
                            "선택한 구단"
                        } else if self.prefs.mine {
                            "우리 구단"
                        } else {
                            "모든 구단"
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    !self.prefs.mine && self.club_scope.is_none(),
                                    "모든 구단",
                                )
                                .clicked()
                            {
                                self.prefs.mine = false;
                                self.club_scope = None;
                                self.dirty = true;
                            }
                            if self.prefs.kind != EntityKind::City
                                && ui.selectable_label(self.prefs.mine, "우리 구단").clicked()
                            {
                                self.prefs.mine = true;
                                self.club_scope = None;
                                self.dirty = true;
                            }
                        });
                    self.dirty |= old != self.prefs.mine;
                    if self.prefs.kind == EntityKind::Player && self.prefs.mine {
                        let old = self.squad_filter;
                        egui::ComboBox::from_id_salt("squad_filter")
                            .width(100.0)
                            .selected_text(
                                ["전체 선수단", "1군", "유소년", "임대 중"]
                                    [self.squad_filter as usize],
                            )
                            .show_ui(ui, |ui| {
                                for (v, t) in ["전체 선수단", "1군", "유소년", "임대 중"]
                                    .into_iter()
                                    .enumerate()
                                {
                                    ui.selectable_value(&mut self.squad_filter, v as u8, t);
                                }
                            });
                        self.dirty |= old != self.squad_filter;
                    }
                });
                if matches!(self.prefs.kind, EntityKind::Player | EntityKind::Staff) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("최소 현재 능력").size(11.0).color(MUTED));
                        self.dirty |= ui
                            .add(egui::DragValue::new(&mut self.ca_min).range(0..=200))
                            .changed();
                        ui.label(RichText::new("최소 잠재 능력").size(11.0).color(MUTED));
                        self.dirty |= ui
                            .add(egui::DragValue::new(&mut self.pa_min).range(0..=200))
                            .changed();
                        ui.label(
                            RichText::new("이름과 구단을 클릭해 상세 화면으로 이동")
                                .size(11.0)
                                .color(MUTED),
                        );
                    });
                }
            });
        ui.add_space(10.0);
        self.rebuild_filter();
        self.entity_table(ui, &index);
    }
    fn entity_table(&mut self, ui: &mut egui::Ui, index: &Index) {
        let rows = self.filtered.clone();
        self.members_table(ui, index, &rows, true);
    }
    fn members_table(&mut self, ui: &mut egui::Ui, index: &Index, rows: &[usize], sortable: bool) {
        let mut picked = None;
        let mut club_picked = None;
        let mut sort_change = None;
        let people = rows.first().is_none_or(|i| {
            matches!(
                index.entities[*i].kind,
                EntityKind::Player | EntityKind::Staff | EntityKind::Person
            )
        });
        let height = ui.available_height().max(140.0) - 5.0;
        TableBuilder::new(ui)
            .id_salt(("entity_table", format!("{:?}", self.route)))
            .striped(true)
            .resizable(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .sense(egui::Sense::click())
            .min_scrolled_height(height)
            .max_scroll_height(height)
            .column(Column::remainder().at_least(230.0))
            .column(Column::exact(60.0))
            .column(Column::exact(72.0))
            .column(Column::exact(72.0))
            .column(Column::exact(215.0))
            .header(29.0, |mut header| {
                for (label, key) in [
                    (
                        if people {
                            "이름 / 포지션"
                        } else {
                            "이름"
                        },
                        Sort::Name,
                    ),
                    ("나이", Sort::Age),
                    ("CA", Sort::Ca),
                    ("PA", Sort::Pa),
                    ("구단", Sort::Club),
                ] {
                    header.col(|ui| {
                        let suffix = if self.prefs.sort == key {
                            if self.prefs.descending {
                                " ↓"
                            } else {
                                " ↑"
                            }
                        } else {
                            ""
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{label}{suffix}"))
                                        .size(11.0)
                                        .color(MUTED),
                                )
                                .frame(false),
                            )
                            .clicked()
                            && sortable
                        {
                            sort_change = Some(key);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(40.0, rows.len(), |mut row| {
                    let idx = rows[row.index()];
                    let e = &index.entities[idx];
                    row.col(|ui| {
                        self.entity_image(ui, e.kind, e.id, Vec2::splat(32.0));
                        ui.vertical(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(&e.name).size(13.0).strong())
                                        .frame(false),
                                )
                                .clicked()
                            {
                                picked = Some(idx);
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(if e.role.is_empty() {
                                        &e.english_name
                                    } else {
                                        &e.role
                                    })
                                    .size(10.0)
                                    .color(MUTED),
                                )
                                .truncate(),
                            );
                        });
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(
                                e.age.map(|a| a.to_string()).unwrap_or_else(|| "—".into()),
                            )
                            .color(MUTED),
                        );
                    });
                    row.col(|ui| ability(ui, e.ca, false));
                    row.col(|ui| ability(ui, e.pa, true));
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            if let Some(id) = e.club_id {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(short_club(&e.club))
                                                .size(12.0)
                                                .color(Color32::from_rgb(141, 195, 209)),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    club_picked = Some(id);
                                }
                            } else {
                                ui.label(RichText::new("소속 없음").size(12.0).color(MUTED));
                            }
                            ui.label(RichText::new(&e.group).size(10.0).color(MUTED));
                        });
                    });
                    if row.response().clicked() && picked.is_none() && club_picked.is_none() {
                        picked = Some(idx);
                    }
                });
            });
        if let Some(key) = sort_change {
            if self.prefs.sort == key {
                self.prefs.descending = !self.prefs.descending;
            } else {
                self.prefs.sort = key;
                self.prefs.descending = matches!(key, Sort::Ca | Sort::Pa);
            }
            self.dirty = true;
        }
        if let Some(id) = club_picked {
            self.open_entity(EntityKind::Club, id);
        } else if let Some(idx) = picked {
            self.select(idx);
        }
        if rows.is_empty() {
            ui.label(RichText::new("조건에 맞는 대상이 없습니다.").color(MUTED));
        }
    }

    fn profile_page(&mut self, ui: &mut egui::Ui, tab: &str) {
        let Some(entity) = self.current_entity() else {
            return;
        };
        ui.horizontal(|ui|{
            ui.vertical(|ui|{ui.label(RichText::new(&entity.name).font(typography::title(25.0)));
                ui.horizontal(|ui|{ui.label(RichText::new(if entity.role.is_empty(){entity.kind.label()}else{&entity.role}).size(12.0).color(MUTED));if entity.kind!=EntityKind::Club{if let Some(id)=entity.club_id{ui.label(RichText::new("·").color(MUTED));if ui.link(RichText::new(short_club(&entity.club)).size(12.0).color(Color32::from_rgb(141,195,209))).clicked(){self.open_entity(EntityKind::Club,id);}}}ui.label(RichText::new(format!("· ID {}",entity.id)).size(10.0).color(MUTED));});
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                if entity.kind == EntityKind::Staff && ui.button("▣ FM에 고정").on_hover_text("이 코치를 FM 창 위에 표시하는 오버레이 설정").clicked() {
                    self.show_overlay = true;
                }
                if self.connected&&ui.add_enabled((self.busy.is_none() && !self.event_studio.is_busy()) && self.draft.is_empty(),egui::Button::new("↻ 현재 값")).clicked(){self.load_detail();}
                if !self.draft.is_empty()&&ui.button("변경 취소").clicked(){self.draft.clear();}
                if matches!(entity.kind,EntityKind::Player|EntityKind::Staff){ui.checkbox(&mut self.precise_values,"세부값").on_hover_text("기본은 FM처럼 정수로 표시합니다. 세부값은 내부 0.2 단위 그대로 보여줍니다.");}
            });
        });
        if self
            .current_entity()
            .is_some_and(|e| e.id != entity.id || e.kind != entity.kind)
        {
            return;
        }
        ui.add_space(6.0);
        let tabs = profile_tabs(entity.kind);
        ui.horizontal(|ui| {
            for t in tabs {
                if tab_button(ui, tab == *t, t) {
                    self.set_tab(t);
                }
            }
            if entity.kind == EntityKind::Club {
                if tab_button(ui, false, "연고 도시 ↗") {
                    self.open_city(&entity);
                }
                if tab_button(ui, false, "이벤트 +") {
                    self.navigate(Route::Events {
                        club_id: Some(entity.id),
                    });
                }
            }
        });
        ui.add_space(3.0);
        ui.separator();
        ui.add_space(7.0);
        if entity.kind == EntityKind::Club && (tab == "선수단" || tab == "스태프") {
            let kind = if tab == "선수단" {
                EntityKind::Player
            } else {
                EntityKind::Staff
            };
            if let Some(index) = self.index.clone() {
                let mut groups = index
                    .entities
                    .iter()
                    .filter(|e| e.kind == kind && e.club_id == Some(entity.id))
                    .map(|e| e.group.clone())
                    .filter(|g| !g.is_empty())
                    .collect::<Vec<_>>();
                groups.sort();
                groups.dedup();
                if !self.member_group.is_empty() && !groups.contains(&self.member_group) {
                    self.member_group.clear();
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("선수단 구분").size(11.0).color(MUTED));
                    if tab_button(ui, self.member_group.is_empty(), "전체") {
                        self.member_group.clear();
                    }
                    for group in &groups {
                        if tab_button(ui, self.member_group == *group, group) {
                            self.member_group = group.clone();
                        }
                    }
                });
                let mut rows = index
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.kind == kind
                            && e.club_id == Some(entity.id)
                            && (self.member_group.is_empty() || e.group == self.member_group)
                    })
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                rows.sort_by_key(|i| std::cmp::Reverse(index.entities[*i].ca));
                ui.label(
                    RichText::new(format!("{} · {}명", short_club(&entity.name), rows.len()))
                        .size(12.0)
                        .color(MUTED),
                );
                self.members_table(ui, &index, &rows, false);
            }
            return;
        }
        let detail = self.detail.clone().unwrap_or_else(|| Detail {
            entity: entity.clone(),
            fields: vec![],
            guards: vec![],
            notes: vec![],
        });
        if self.busy == Some(Busy::Detail) {
            ui.horizontal(|ui| {
                busy_mark(ui);
                ui.label(
                    RichText::new("현재 상세 정보를 읽고 있습니다…")
                        .size(12.0)
                        .color(MUTED),
                );
            });
        }
        egui::ScrollArea::vertical().id_salt(("profile_body",entity.kind,entity.id,tab)).auto_shrink([false,false]).show(ui,|ui|{
            match entity.kind{
                EntityKind::Player=>self.player_profile(ui,&detail,tab),
                EntityKind::Staff=>self.staff_profile(ui,&detail,tab),
                EntityKind::Club=>self.club_profile(ui,&detail,tab),
                EntityKind::City=>self.city_profile(ui,&detail,tab),
                EntityKind::Person=>self.person_profile(ui,&detail),
            }
            if detail.fields.is_empty()&&self.busy!=Some(Busy::Detail){ui.add_space(16.0);ui.label(RichText::new("이 스냅샷에는 상세 능력치가 없습니다. FM 연결 후 현재 값을 불러올 수 있습니다.").size(12.0).color(AMBER));}
            if !detail.notes.is_empty(){ui.add_space(14.0);egui::CollapsingHeader::new(RichText::new("데이터 안내").size(11.0).color(MUTED)).show(ui,|ui|{for note in &detail.notes{ui.label(RichText::new(note).size(11.0).color(MUTED));}});}
        });
    }
    fn player_profile(&mut self, ui: &mut egui::Ui, d: &Detail, tab: &str) {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(164.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.identity_card(ui, d);
                    ui.add_space(7.0);
                    self.position_pitch(ui, d, 164.0);
                    ui.add_space(5.0);
                    ui.label(
                        RichText::new("포지션을 클릭해 숙련도 편집")
                            .size(10.0)
                            .color(MUTED),
                    );
                },
            );
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                match tab {
                    "개요" => {
                        ui.columns(3, |cols| {
                            self.field_group(&mut cols[0], d, "기술", &["기술"]);
                            self.field_group(&mut cols[1], d, "정신", &["정신"]);
                            self.field_group(&mut cols[2], d, "신체", &["신체"]);
                            cols[2].add_space(6.0);
                            self.field_group(&mut cols[2], d, "경기 히든", &["경기 히든"]);
                        });
                        ui.add_space(7.0);
                        ui.columns(3, |cols| {
                            self.field_group(&mut cols[0], d, "성격 · 성장", &["성격"]);
                            let positions = d
                                .fields
                                .iter()
                                .filter(|f| f.section == "포지션")
                                .collect::<Vec<_>>();
                            let half = positions.len().div_ceil(2);
                            self.fields_card(
                                &mut cols[1],
                                "포지션 · 수비 / 중원",
                                &positions[..half],
                            );
                            self.fields_card(
                                &mut cols[2],
                                "포지션 · 공격 / 측면",
                                &positions[half..],
                            );
                        });
                    }
                    "성격 · 히든" => {
                        ui.columns(2, |cols| {
                            self.field_group(&mut cols[0], d, "성격", &["성격"]);
                            self.field_group(&mut cols[1], d, "경기 성향", &["경기 히든"]);
                        });
                    }
                    "포지션" => {
                        ui.columns(2, |cols| {
                            let positions = d
                                .fields
                                .iter()
                                .filter(|f| f.section == "포지션")
                                .collect::<Vec<_>>();
                            let half = positions.len().div_ceil(2);
                            self.fields_card(&mut cols[0], "수비 / 중원", &positions[..half]);
                            self.fields_card(&mut cols[1], "공격 / 측면", &positions[half..]);
                        });
                    }
                    "골키퍼" => {
                        self.field_group(ui, d, "골키퍼 능력", &["골키퍼"]);
                    }
                    _ => {}
                }
            });
        });
    }
    fn identity_card(&mut self, ui: &mut egui::Ui, d: &Detail) {
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(4)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(148.0);
                ui.vertical_centered(|ui| {
                    self.entity_image(ui, d.entity.kind, d.entity.id, Vec2::new(148.0, 158.0));
                    ui.add_space(5.0);
                    if d.entity.english_name != d.entity.name {
                        ui.label(
                            RichText::new(&d.entity.english_name)
                                .size(11.0)
                                .color(MUTED),
                        );
                    }
                    if let Some(age) = d.entity.age {
                        ui.label(RichText::new(format!("{age}세")).size(12.0).color(MUTED));
                    }
                    ui.add_space(4.0);
                    if let Some(id) = d.entity.club_id {
                        ui.horizontal(|ui| {
                            self.entity_image(ui, EntityKind::Club, id, Vec2::splat(20.0));
                            if ui
                                .link(
                                    RichText::new(short_club(&d.entity.club))
                                        .size(12.0)
                                        .color(Color32::from_rgb(141, 195, 209)),
                                )
                                .clicked()
                            {
                                self.open_entity(EntityKind::Club, id);
                            }
                        });
                    }
                });
                ui.add_space(5.0);
                ui.separator();
                for key in [
                    "ca",
                    "pa",
                    "height",
                    "left_foot",
                    "right_foot",
                    "world_reputation",
                ] {
                    if let Some(f) = d.fields.iter().find(|f| f.key == key) {
                        self.field_row(ui, f);
                    }
                }
                if !d.entity.group.is_empty() {
                    ui.label(RichText::new(&d.entity.group).size(11.0).color(MUTED));
                }
            });
    }
    fn field_group(&mut self, ui: &mut egui::Ui, d: &Detail, title: &str, sections: &[&str]) {
        let mut fields = d
            .fields
            .iter()
            .filter(|f| sections.contains(&f.section.as_str()) && f.section != "연결")
            .collect::<Vec<_>>();
        let order = [
            "technique",
            "finishing",
            "dribbling",
            "marking",
            "long_shots",
            "crossing",
            "tackling",
            "passing",
            "first_touch",
            "heading",
            "corners",
            "free_kicks",
            "penalties",
            "long_throws",
            "bravery",
            "leadership",
            "positioning",
            "determination",
            "vision",
            "anticipation",
            "off_ball",
            "aggression",
            "concentration",
            "flair",
            "composure",
            "teamwork",
            "decisions",
            "work_rate",
            "acceleration",
            "balance",
            "strength",
            "agility",
            "jumping_reach",
            "pace",
            "stamina",
            "natural_fitness",
        ];
        if d.entity.kind == EntityKind::Player
            && sections
                .iter()
                .any(|s| ["기술", "정신", "신체"].contains(s))
        {
            fields.sort_by_key(|f| order.iter().position(|k| *k == f.key).unwrap_or(999));
        }
        self.fields_card(ui, title, &fields);
    }
    fn fields_card(&mut self, ui: &mut egui::Ui, title: &str, fields: &[&Field]) {
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(4)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.spacing_mut().interact_size.y = 20.0;
                ui.spacing_mut().button_padding = Vec2::new(4.0, 2.0);
                ui.label(
                    RichText::new(title)
                        .font(typography::title(14.0))
                        .color(INK),
                );
                ui.add_space(3.0);
                ui.separator();
                ui.add_space(2.0);
                for f in fields {
                    self.field_row(ui, f);
                }
            });
    }
    fn field_row(&mut self, ui: &mut egui::Ui, f: &Field) {
        let changed = self.draft.contains_key(&f.key);
        let mut value = *self.draft.get(&f.key).unwrap_or(&f.value);
        let color = if f.key.starts_with("score_") {
            attribute_color(value / 15.0)
        } else if f.max <= 20.0 {
            attribute_color(value)
        } else {
            INK
        };
        let rect = ui
            .allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 22.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let reserve = if f.max > 10000.0 { 116.0 } else { 42.0 };
                    let (label_rect, label_response) = ui.allocate_exact_size(
                        Vec2::new((ui.available_width() - reserve).max(30.0), 21.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        label_rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        &f.label,
                        FontId::proportional(13.0),
                        if changed { LIME } else { INK },
                    );
                    label_response.on_hover_text(&f.hint);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.style_mut().override_font_id = Some(typography::title(14.0));
                        ui.visuals_mut().override_text_color = Some(color);
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
                        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
                        let precise = self.precise_values || f.max > 20.0;
                        if f.binding.is_some() && self.connected {
                            let res = ui.add_enabled(
                                self.busy.is_none() && !self.event_studio.is_busy(),
                                egui::DragValue::new(&mut value)
                                    .range(f.min..=f.max)
                                    .speed(if precise { f.step } else { 1.0 })
                                    .custom_formatter(move |v, _| {
                                        if precise {
                                            format_number(v)
                                        } else {
                                            format!("{v:.0}")
                                        }
                                    })
                                    .custom_parser(|s| s.replace(',', "").parse().ok()),
                            );
                            res.clone().on_hover_text(format!(
                                "실제 값 {}\n{}",
                                format_number(f.value),
                                f.hint
                            ));
                            if res.changed() {
                                value = (value / f.step).round() * f.step;
                                if (value - f.value).abs() < 0.00001 {
                                    self.draft.remove(&f.key);
                                } else {
                                    self.draft.insert(f.key.clone(), value);
                                }
                            }
                        } else {
                            ui.label(
                                RichText::new(if precise {
                                    format_number(value)
                                } else {
                                    format!("{value:.0}")
                                })
                                .font(typography::title(14.0))
                                .color(color),
                            )
                            .on_hover_text(format!(
                                "실제 값 {}\n{}",
                                format_number(value),
                                f.hint
                            ));
                        }
                        if changed {
                            ui.label(RichText::new("•").size(11.0).color(LIME));
                        }
                    });
                },
            )
            .response
            .rect;
        ui.painter().line_segment(
            [
                egui::pos2(rect.left(), rect.bottom() + 1.0),
                egui::pos2(rect.right(), rect.bottom() + 1.0),
            ],
            Stroke::new(0.5_f32, LINE.gamma_multiply(0.22)),
        );
    }
    fn staff_profile(&mut self, ui: &mut egui::Ui, d: &Detail, tab: &str) {
        // Derived scores follow the draft without writing any game memory.
        let mut preview = d.clone();
        for field in &mut preview.fields {
            if let Some(value) = self.draft.get(&field.key) {
                field.value = *value;
            }
        }
        preview.fields.retain(|f| !f.key.starts_with("score_"));
        crate::fields::append_scores(&mut preview);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(164.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.identity_card(ui, d);
                    ui.add_space(7.0);
                    if tab != "성격" {
                        self.field_group(ui, d, "성격", &["성격"]);
                    }
                },
            );
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                if tab == "성격" {
                    self.field_group(ui, d, "성격", &["성격"]);
                } else if tab == "직무 점수" {
                    ui.label(
                        RichText::new("등록한 직무 공식으로 계산 · 각 항목 300점 만점")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.columns(3, |c| {
                        for (col, (title, fields)) in
                            staff_score_groups(&preview).into_iter().enumerate()
                        {
                            self.fields_card(&mut c[col], title, &fields);
                        }
                    });
                } else {
                    let coach = [
                        "staff_attack",
                        "staff_defense",
                        "staff_fitness",
                        "staff_mental",
                        "staff_technical",
                        "staff_tactical",
                        "staff_set_pieces",
                        "staff_gk_shot",
                        "staff_gk_handling",
                        "staff_gk_distribution",
                    ];
                    let manage = [
                        "staff_discipline",
                        "staff_determination",
                        "staff_motivation",
                        "staff_management",
                        "staff_youth",
                        "staff_knowledge",
                        "staff_judging_ability",
                        "staff_judging_potential",
                    ];
                    ui.columns(3, |c| {
                        self.fields_card(
                            &mut c[0],
                            "코칭 능력",
                            &coach
                                .iter()
                                .filter_map(|key| d.fields.iter().find(|f| f.key == *key))
                                .collect::<Vec<_>>(),
                        );
                        self.fields_card(
                            &mut c[1],
                            "관리 · 육성",
                            &manage
                                .iter()
                                .filter_map(|key| d.fields.iter().find(|f| f.key == *key))
                                .collect::<Vec<_>>(),
                        );
                        self.fields_card(
                            &mut c[2],
                            "전문 분야",
                            &d.fields
                                .iter()
                                .filter(|f| {
                                    f.section == "스태프 능력"
                                        && !coach.contains(&f.key.as_str())
                                        && !manage.contains(&f.key.as_str())
                                })
                                .collect::<Vec<_>>(),
                        );
                    });
                    ui.add_space(7.0);
                    ui.columns(3, |c| {
                        for (col, (title, fields)) in
                            staff_score_groups(&preview).into_iter().enumerate()
                        {
                            self.fields_card(&mut c[col], title, &fields);
                        }
                    });
                }
            });
        });
    }
    fn person_profile(&mut self, ui: &mut egui::Ui, d: &Detail) {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(164.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.identity_card(ui, d),
            );
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.columns(2, |cols| {
                    self.field_group(&mut cols[0], d, "인물 성격", &["성격"]);
                    self.field_group(&mut cols[1], d, "기본 정보", &["기본"]);
                });
            });
        });
    }
    fn club_profile(&mut self, ui: &mut egui::Ui, d: &Detail, tab: &str) {
        if tab == "스폰서" {
            self.sponsor_table(ui, d);
            return;
        }
        if tab != "개요" {
            self.field_group(ui, d, tab, &[tab]);
            return;
        }
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(4)
            .inner_margin(9)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    self.entity_image(ui, EntityKind::Club, d.entity.id, Vec2::splat(58.0));
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&d.entity.english_name).font(typography::title(18.0)),
                        );
                        if let Some(stadium) = d.notes.iter().find(|n| n.starts_with("홈 구장:"))
                        {
                            ui.label(RichText::new(stadium).size(12.0).color(MUTED));
                        }
                        ui.horizontal(|ui| {
                            if ui.link("연고 도시 ›").clicked() {
                                self.open_city(&d.entity);
                            }
                            if ui.link("스폰서 / 지역 개발 이벤트 ›").clicked() {
                                self.navigate(Route::Events {
                                    club_id: Some(d.entity.id),
                                });
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(index) = &self.index {
                            let players = index
                                .entities
                                .iter()
                                .filter(|e| {
                                    e.kind == EntityKind::Player && e.club_id == Some(d.entity.id)
                                })
                                .count();
                            let staff = index
                                .entities
                                .iter()
                                .filter(|e| {
                                    e.kind == EntityKind::Staff && e.club_id == Some(d.entity.id)
                                })
                                .count();
                            pill(ui, &format!("선수 {players} · 스태프 {staff}"), LIME);
                        }
                    });
                });
            });
        ui.add_space(7.0);
        ui.columns(3, |cols| {
            self.field_group(&mut cols[0], d, "재정 · 예산  /  GBP", &["재정"]);
            self.field_group(&mut cols[1], d, "구장 · 시설", &["구장"]);
            cols[1].add_space(7.0);
            self.field_group(&mut cols[1], d, "구단 정보", &["구단"]);
            self.club_links_card(&mut cols[2], d);
        });
        ui.add_space(7.0);
        ui.columns(2, |cols| {
            self.club_member_preview(&mut cols[0], d, EntityKind::Player, "주요 선수", "선수단");
            self.club_member_preview(
                &mut cols[1],
                d,
                EntityKind::Staff,
                "코치 · 스태프",
                "스태프",
            );
        });
        ui.add_space(7.0);
        self.sponsor_table(ui, d);
    }
    fn club_links_card(&mut self, ui: &mut egui::Ui, d: &Detail) {
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(4)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("구단 메뉴")
                        .font(typography::title(14.0))
                        .color(INK),
                );
                ui.separator();
                for (label, target) in [
                    ("선수단 · 1군 / 유소년 / 임대", "선수단"),
                    ("코치 · 스태프 능력치", "스태프"),
                    ("재정 · 이적 / 급료 예산", "재정"),
                    ("홈 구장 · 시설", "구장"),
                    ("스폰서 계약", "스폰서"),
                ] {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(format!("{label}  ›")).size(12.0))
                                .frame(false),
                        )
                        .clicked()
                    {
                        self.set_tab(target);
                    }
                }
                if ui.link("연고 도시  ↗").clicked() {
                    self.open_city(&d.entity);
                }
                if ui.link("이벤트 생성기  ↗").clicked() {
                    self.navigate(Route::Events {
                        club_id: Some(d.entity.id),
                    });
                }
            });
    }
    fn club_member_preview(
        &mut self,
        ui: &mut egui::Ui,
        d: &Detail,
        kind: EntityKind,
        title: &str,
        tab: &str,
    ) {
        let Some(index) = self.index.clone() else {
            return;
        };
        let mut rows = index
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == kind && e.club_id == Some(d.entity.id))
            .collect::<Vec<_>>();
        rows.sort_by_key(|(_, e)| std::cmp::Reverse(e.ca));
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(4)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(title)
                            .font(typography::title(14.0))
                            .color(INK),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.link(format!("전체 {}명 ›", rows.len())).clicked() {
                            self.set_tab(tab);
                        }
                    });
                });
                ui.separator();
                for (idx, entity) in rows.iter().take(5) {
                    ui.horizontal(|ui| {
                        self.entity_image(ui, kind, entity.id, Vec2::splat(25.0));
                        if ui
                            .add(
                                egui::Button::new(RichText::new(&entity.name).size(12.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.select(*idx);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} / {}",
                                    entity.ca.map(|v| v.to_string()).unwrap_or("—".into()),
                                    entity.pa.map(|v| v.to_string()).unwrap_or("—".into())
                                ))
                                .size(12.0)
                                .color(LIME),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&entity.role).size(10.0).color(MUTED),
                                )
                                .truncate(),
                            );
                        });
                    });
                }
                if rows.is_empty() {
                    ui.label(
                        RichText::new("현재 목록에 소속 인물이 없습니다.")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
            });
    }
    fn sponsor_table(&mut self, ui: &mut egui::Ui, d: &Detail) {
        let annuals = d
            .fields
            .iter()
            .filter(|f| f.key.starts_with("sponsor.") && f.key.ends_with(".annual"))
            .collect::<Vec<_>>();
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(4)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("후원 계약  /  GBP")
                            .size(13.0)
                            .strong()
                            .color(AMBER),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.link("계약 갱신 · 이벤트 ›").clicked() {
                            self.navigate(Route::Events {
                                club_id: Some(d.entity.id),
                            });
                        }
                    });
                });
                ui.separator();
                ui.columns(2, |cols| {
                    for (i, annual) in annuals.iter().enumerate() {
                        let col = i % 2;
                        let totalkey = annual.key.replace(".annual", ".total");
                        self.field_row(&mut cols[col], annual);
                        if let Some(total) = d.fields.iter().find(|f| f.key == totalkey) {
                            self.field_row(&mut cols[col], total);
                        }
                    }
                });
                if annuals.is_empty() {
                    ui.label(
                        RichText::new("조회된 후원 계약이 없습니다.")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
            });
    }
    fn open_city(&mut self, club: &Entity) {
        let exact = self
            .detail
            .as_ref()
            .filter(|d| d.entity.kind == EntityKind::Club && d.entity.id == club.id)
            .and_then(|d| {
                d.fields
                    .iter()
                    .find(|f| f.key == "club.city_id")
                    .map(|f| f.value as u32)
            });
        let id = exact.or_else(|| {
            self.index.as_ref().and_then(|i| {
                i.entities
                    .iter()
                    .find(|e| e.kind == EntityKind::City && e.club_id == Some(club.id))
                    .map(|e| e.id)
            })
        });
        if let Some(id) = id {
            self.open_entity(EntityKind::City, id);
        } else {
            self.notice = Some(("이 구단의 연고 도시가 현재 목록에 없습니다.".into(), false));
        }
    }
    fn city_profile(&mut self, ui: &mut egui::Ui, d: &Detail, tab: &str) {
        if tab == "연결 구단" {
            self.linked_clubs(ui, d);
            return;
        }
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(22)
            .corner_radius(10)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    city_emblem(ui, Vec2::new(150.0, 120.0));
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        eyebrow(ui, "THE HOME OF FOOTBALL");
                        ui.label(RichText::new(&d.entity.name).size(35.0).strong());
                        ui.label(
                            RichText::new("연고 도시 · 지역 개발")
                                .size(13.0)
                                .color(MUTED),
                        );
                        if let Some(id) = d.entity.club_id {
                            if ui
                                .link(RichText::new(short_club(&d.entity.club)).color(LIME))
                                .clicked()
                            {
                                self.open_entity(EntityKind::Club, id);
                            }
                        }
                    });
                });
            });
        ui.add_space(18.0);
        ui.columns(2, |c| {
            self.field_group(&mut c[0], d, "도시 개발", &["도시"]);
            self.field_group(&mut c[1], d, "위치", &["위치"]);
        });
        ui.add_space(15.0);
        let origin = match self.route {
            Route::Profile { origin_club, .. } => origin_club,
            _ => None,
        };
        if let Some(id) = origin.or(d.entity.club_id) {
            let target = self
                .index
                .as_ref()
                .and_then(|i| {
                    i.entities
                        .iter()
                        .find(|e| e.kind == EntityKind::Club && e.id == id)
                })
                .map(|e| short_club(&e.name))
                .unwrap_or_else(|| format!("구단 {id}"));
            if ui
                .add(primary(&format!("{target} · 도시 개발 이벤트 만들기")))
                .clicked()
            {
                self.navigate(Route::Events { club_id: Some(id) });
            }
        }
        ui.add_space(18.0);
        self.linked_clubs(ui, d);
    }
    fn linked_clubs(&mut self, ui: &mut egui::Ui, d: &Detail) {
        eyebrow(ui, "CONNECTED CLUBS");
        let mut ids = Vec::new();
        if let Some(id) = d.entity.club_id {
            ids.push(id);
        }
        if let Route::Profile {
            origin_club: Some(id),
            ..
        } = self.route
        {
            ids.push(id);
        }
        for ((kind, id), detail) in &self.detail_cache {
            if *kind == EntityKind::Club
                && detail
                    .fields
                    .iter()
                    .any(|f| f.key == "club.city_id" && f.value == d.entity.id as f64)
            {
                ids.push(*id);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        let clubs = ids
            .into_iter()
            .filter_map(|id| {
                self.index
                    .as_ref()
                    .and_then(|i| crate::navigation::entity(i, EntityKind::Club, id))
                    .cloned()
            })
            .collect::<Vec<_>>();
        for club in clubs {
            if option_card(
                ui,
                &format!("CLUB {}", club.id),
                &short_club(&club.name),
                "구단 상세 화면으로 이동",
            ) {
                self.open_entity(EntityKind::Club, club.id);
            }
            ui.add_space(6.0);
        }
    }
    fn entity_image(&mut self, ui: &mut egui::Ui, kind: EntityKind, id: u32, size: Vec2) {
        if let Some(texture) = self.assets.image(ui.ctx(), kind, id) {
            ui.add(egui::Image::new(&texture).fit_to_exact_size(size));
        } else {
            let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter()
                .rect_filled(r, 8, Color32::from_rgb(29, 44, 53));
            if matches!(
                kind,
                EntityKind::Player | EntityKind::Staff | EntityKind::Person
            ) {
                let c = r.center();
                let scale = size.x.min(size.y);
                ui.painter().circle_filled(
                    egui::pos2(c.x, r.top() + size.y * 0.32),
                    scale * 0.16,
                    Color32::from_rgb(67, 86, 94),
                );
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(c.x, r.top() + size.y * 0.77),
                        Vec2::new(scale * 0.63, size.y * 0.46),
                    ),
                    (scale * 0.2) as u8,
                    Color32::from_rgb(67, 86, 94),
                );
            } else {
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    if kind == EntityKind::Club {
                        "FC"
                    } else {
                        "CITY"
                    },
                    FontId::proportional((size.x * 0.25).min(24.0)),
                    LIME,
                );
            }
        }
    }
    fn position_pitch(&mut self, ui: &mut egui::Ui, d: &Detail, width: f32) {
        let (r, response) = ui.allocate_exact_size(Vec2::new(width, 118.0), egui::Sense::click());
        let p = ui.painter();
        p.rect_filled(r, 8, Color32::from_rgb(22, 53, 48));
        let pitch = r.shrink(12.0);
        let line = Stroke::new(1.0_f32, Color32::from_rgb(68, 111, 91));
        p.rect_stroke(pitch, 0, line, egui::StrokeKind::Inside);
        p.line_segment(
            [
                egui::pos2(pitch.center().x, pitch.top()),
                egui::pos2(pitch.center().x, pitch.bottom()),
            ],
            line,
        );
        p.circle_stroke(pitch.center(), 18.0, line);
        for (key, x, y) in [
            ("gk", 0.07, 0.5),
            ("dl", 0.26, 0.14),
            ("dc", 0.26, 0.5),
            ("dr", 0.26, 0.86),
            ("dm", 0.43, 0.5),
            ("ml", 0.55, 0.14),
            ("mc", 0.55, 0.5),
            ("mr", 0.55, 0.86),
            ("aml", 0.76, 0.14),
            ("amc", 0.76, 0.5),
            ("amr", 0.76, 0.86),
            ("st", 0.94, 0.5),
        ] {
            let value = d
                .fields
                .iter()
                .find(|f| f.key == format!("position_{key}"))
                .map(|f| *self.draft.get(&f.key).unwrap_or(&f.value))
                .unwrap_or(0.0);
            let pt = egui::pos2(
                pitch.left() + pitch.width() * x,
                pitch.top() + pitch.height() * y,
            );
            p.circle_filled(
                pt,
                if value >= 15.0 { 5.0 } else { 3.5 },
                if value >= 15.0 {
                    LIME
                } else if value >= 10.0 {
                    AMBER
                } else {
                    Color32::from_rgb(42, 73, 64)
                },
            );
        }
        if response
            .on_hover_text("포지션 숙련도 보기 / 수정")
            .clicked()
        {
            self.set_tab("포지션");
        }
    }
    fn events_page(&mut self, ui: &mut egui::Ui, club_id: Option<u32>) {
        self.event_studio.show(
            ui,
            self.index.as_deref(),
            club_id,
            self.connected && self.busy.is_none(),
        );
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.show_overlay {
            let mut open = true;
            egui::Window::new("FM 코치 오버레이")
                .open(&mut open)
                .default_width(390.0)
                .resizable(false)
                .show(ctx, |ui| {
                    self.coach_overlay.controls(
                        ui,
                        self.index.as_ref().map(|i| &i.session),
                        self.detail.as_ref(),
                        self.connected,
                    );
                });
            self.show_overlay = open;
        }
        if self.show_assets {
            let mut open = true;
            egui::Window::new("사진 · 구단 로고")
                .open(&mut open)
                .default_width(620.0)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new(self.assets.status()).size(13.0).color(LIME));
                    ui.label(
                        "FM 그래픽 폴더를 한 줄에 하나씩 입력하세요. 위쪽 폴더가 우선 적용됩니다.",
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut self.asset_roots)
                            .desired_width(f32::INFINITY)
                            .desired_rows(5),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("폴더 설정 저장").clicked() {
                            self.assets.set_roots(
                                self.asset_roots
                                    .lines()
                                    .map(str::trim)
                                    .filter(|p| !p.is_empty())
                                    .map(PathBuf::from)
                                    .collect(),
                            );
                        }
                        if ui.button("사진 다시 읽기").clicked() {
                            self.assets.refresh();
                        }
                    });
                    ui.label(
                        RichText::new(
                            "설치된 페이스팩과 로고팩을 읽습니다. FM 파일은 변경하지 않습니다.",
                        )
                        .size(11.0)
                        .color(MUTED),
                    );
                });
            self.show_assets = open;
        }

        if self.show_review {
            let mut open = true;
            let mut apply = false;
            egui::Window::new("변경사항 검토")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if let Some(detail) = &self.detail {
                        ui.label(RichText::new(&detail.entity.name).size(20.0).strong());
                        ui.label(
                            RichText::new("아래 값이 실행 중인 FM에 적용됩니다.").color(MUTED),
                        );
                        ui.add_space(12.0);
                        egui::ScrollArea::vertical()
                            .max_height(390.0)
                            .show(ui, |ui| {
                                for field in &detail.fields {
                                    if let Some(value) = self.draft.get(&field.key) {
                                        ui.horizontal(|ui| {
                                            ui.label(&field.label);
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(format_number(*value))
                                                            .strong()
                                                            .color(LIME),
                                                    );
                                                    ui.label(RichText::new("→").color(MUTED));
                                                    ui.label(
                                                        RichText::new(format_number(field.value))
                                                            .color(MUTED),
                                                    );
                                                },
                                            );
                                        });
                                        ui.separator();
                                    }
                                }
                            });
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("원본 값 검증 → 로컬 백업 → 쓰기 → 결과 재확인")
                                .size(12.0)
                                .color(MUTED),
                        );
                        ui.label(
                            RichText::new("CA를 변경해도 다른 능력치를 자동 배분하지 않습니다.")
                                .size(11.0)
                                .color(AMBER),
                        );
                        ui.add_space(10.0);
                        apply = ui
                            .add_enabled(
                                self.connected
                                    && (self.busy.is_none() && !self.event_studio.is_busy()),
                                primary("확인 · 게임에 적용"),
                            )
                            .clicked();
                    }
                });
            self.show_review = open;
            if apply {
                self.apply();
            }
        }
        if self.show_history {
            let mut open = true;
            let mut undo = None;
            egui::Window::new("변경 이력")
                .open(&mut open)
                .default_width(670.0)
                .default_height(530.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new("검증된 변경과 원본 백업").size(20.0).strong());
                    ui.label(
                        RichText::new("실행 중인 세션과 현재 값이 일치할 때만 되돌릴 수 있습니다.")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.add_space(10.0);
                    if self.history.is_empty() {
                        ui.label(RichText::new("아직 적용한 변경이 없습니다.").color(MUTED));
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for receipt in &self.history {
                            egui::Frame::new()
                                .fill(CARD)
                                .inner_margin(12)
                                .corner_radius(7)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&receipt.entity.name).strong());
                                        ui.label(
                                            RichText::new(history_time(&receipt.time))
                                                .size(11.0)
                                                .color(MUTED),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add_enabled(
                                                        self.connected
                                                            && (self.busy.is_none()
                                                                && !self.event_studio.is_busy())
                                                            && self.draft.is_empty(),
                                                        egui::Button::new("되돌리기"),
                                                    )
                                                    .clicked()
                                                {
                                                    undo = Some(receipt.clone());
                                                }
                                            },
                                        );
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{}개 변경 · {}",
                                            receipt.changes.len(),
                                            receipt.status
                                        ))
                                        .size(11.0)
                                        .color(MUTED),
                                    );
                                    egui::CollapsingHeader::new("변경 내용")
                                        .id_salt(&receipt.id)
                                        .show(ui, |ui| {
                                            for change in &receipt.changes {
                                                ui.label(format!(
                                                    "{}    {} → {}",
                                                    change.label,
                                                    format_number(change.before),
                                                    format_number(change.after)
                                                ));
                                            }
                                        });
                                });
                            ui.add_space(7.0);
                        }
                    });
                });
            self.show_history = open;
            if let Some(receipt) = undo {
                self.undo_target = Some(receipt);
            }
        }
        if let Some(receipt) = self.undo_target.clone() {
            let mut open = true;
            let mut confirmed = false;
            egui::Window::new("원본 값으로 되돌리기")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{} · {}개 항목",
                        receipt.entity.name,
                        receipt.changes.len()
                    ));
                    ui.label("해당 변경 직전 값으로 복원합니다.");
                    confirmed = ui
                        .add_enabled(
                            self.connected && (self.busy.is_none() && !self.event_studio.is_busy()),
                            primary("확인 · 되돌리기"),
                        )
                        .clicked();
                });
            if !open {
                self.undo_target = None;
            }
            if confirmed {
                self.undo_target = None;
                self.undo(receipt);
            }
        }
        if self.select_after_discard.is_some()
            || self.connect_after_discard
            || self.pending_route.is_some()
            || self.pending_history.is_some()
        {
            let mut open = true;
            let mut discard = false;
            egui::Window::new("미적용 변경사항")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!(
                        "아직 적용하지 않은 변경 {}개가 있습니다.",
                        self.draft.len()
                    ));
                    ui.label("초안을 버리고 이동할까요?");
                    discard = ui.button("초안 버리고 계속").clicked();
                });
            if discard {
                self.draft.clear();
                if let Some(cursor) = self.pending_history.take() {
                    self.history_go(cursor);
                } else if let Some(route) = self.pending_route.take() {
                    self.navigate(route);
                } else if self.connect_after_discard {
                    self.connect_after_discard = false;
                    self.select_after_discard = None;
                    self.connect();
                } else if let Some(idx) = self.select_after_discard.take() {
                    self.select(idx);
                }
            }
            if !open {
                self.select_after_discard = None;
                self.connect_after_discard = false;
                self.pending_route = None;
                self.pending_history = None;
            }
        }
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S))
            && !self.draft.is_empty()
        {
            self.show_review = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::R))
            && self.connected
            && matches!(self.route, Route::Profile { .. })
        {
            self.load_detail();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
            if !matches!(self.route, Route::List { .. }) {
                self.navigate(Route::List {
                    kind: self.prefs.kind,
                    club_id: None,
                });
            }
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("database_search")));
        }
        if (matches!(self.busy, Some(Busy::Apply | Busy::Undo)) || self.event_studio.busy_write())
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice = Some((
                "변경 검증이 진행 중입니다. 완료된 뒤 창을 닫아 주세요.".into(),
                false,
            ));
        }
        if !self.qa_route_done {
            if let Some(action) = self.qa_route.take() {
                match action.as_str() {
                    "club" => {
                        if let Some(id) = self.index.as_ref().and_then(|i| i.session.own_club_id) {
                            self.open_entity(EntityKind::Club, id);
                        }
                    }
                    "city" => {
                        if let Some(e) = self.current_entity() {
                            self.open_city(&e);
                        }
                    }
                    "roster" => self.set_tab("선수단"),
                    "events" => self.navigate(Route::Events {
                        club_id: self.index.as_ref().and_then(|i| i.session.own_club_id),
                    }),
                    "overlay" => self.show_overlay = true,
                    _ => {}
                }
            }
            self.qa_route_done = true;
        }
        self.drain_events();
        self.assets.poll(ctx);
        self.event_studio.poll(ctx);
        self.coach_overlay
            .update(ctx, self.index.as_ref().map(|i| &i.session), self.connected);
        self.footer(ctx);
        self.side_nav(ctx);
        self.content(ctx);
        self.dialogs(ctx);
        if self.busy.is_some() || self.event_studio.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if let Some(path) = self.qa_output.clone() {
            self.qa_frames += 1;
            if self.qa_frames >= 12 && !self.assets.is_loading() && !self.qa_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.qa_requested = true;
            }
            let screenshot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = screenshot {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let bytes = image
                    .pixels
                    .iter()
                    .flat_map(|p| p.to_array())
                    .collect::<Vec<_>>();
                if let Err(error) = image::save_buffer(
                    &path,
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("UI screenshot failed: {error}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.qa_output.is_some() {
            return;
        }
        let dir = app_dir();
        if std::fs::create_dir_all(&dir).is_ok() {
            if let Ok(bytes) = serde_json::to_vec_pretty(&self.prefs) {
                let _ = std::fs::write(dir.join("preferences.json"), bytes);
            }
        }
    }
}

fn app_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("TouchlineEditor")
}
fn own_name(index: &Index) -> String {
    index
        .session
        .own_club_id
        .and_then(|id| {
            index
                .entities
                .iter()
                .find(|e| e.kind == EntityKind::Club && e.id == id)
                .map(|e| e.name.clone())
        })
        .or_else(|| {
            index.session.own_club_id.and_then(|id| {
                index
                    .entities
                    .iter()
                    .find(|e| e.club_id == Some(id) && !e.club.is_empty())
                    .map(|e| e.club.clone())
            })
        })
        .unwrap_or_else(|| "전체 데이터베이스".into())
}
fn default_section(detail: &Detail) -> String {
    if detail.fields.iter().any(|f| f.section == "기본") {
        "기본".into()
    } else {
        detail
            .fields
            .first()
            .map(|f| f.section.clone())
            .unwrap_or_default()
    }
}

fn profile_tabs(kind: EntityKind) -> &'static [&'static str] {
    match kind {
        EntityKind::Player => &["개요", "성격 · 히든", "포지션", "골키퍼"],
        EntityKind::Staff => &["개요", "직무 점수", "성격"],
        EntityKind::Club => &["개요", "선수단", "스태프", "재정", "구장", "스폰서"],
        EntityKind::City => &["개요", "연결 구단"],
        EntityKind::Person => &["개요", "성격"],
    }
}

fn staff_score_groups(detail: &Detail) -> [(&'static str, Vec<&Field>); 3] {
    let groups: [(&str, &[&str]); 3] = [
        (
            "훈련 코치 점수 / 300",
            &[
                "technical_attack",
                "technical_defense",
                "technical_mental",
                "tactical_attack",
                "tactical_defense",
                "tactical_mental",
                "fitness",
                "gk_shot",
                "gk_distribution",
                "set_pieces",
            ],
        ),
        (
            "감독 · 육성 점수 / 300",
            &[
                "assistant",
                "general",
                "reserve_manager",
                "youth_manager",
                "youth_assistant",
                "youth_coach",
                "youth_goalkeeper",
                "head_youth",
            ],
        ),
        (
            "영입 · 지원 점수 / 300",
            &[
                "dof",
                "td",
                "loan_manager",
                "scout",
                "head_analyst",
                "performance_analyst",
                "recruitment_analyst",
                "physio",
                "scientist",
            ],
        ),
    ];
    groups.map(|(title, keys)| {
        (
            title,
            keys.iter()
                .filter_map(|key| {
                    detail
                        .fields
                        .iter()
                        .find(|f| f.key == format!("score_{key}"))
                })
                .collect(),
        )
    })
}
fn history_time(value: &str) -> String {
    let Ok(ms) = value.parse::<i64>() else {
        return value.to_string();
    };
    // Receipt timestamps are Unix milliseconds; label UTC explicitly instead of assuming a zone.
    let seconds = ms / 1000;
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    let s = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        s / 3_600,
        s / 60 % 60
    )
}
fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(BG).strong())
        .fill(LIME)
        .stroke(Stroke::NONE)
        .corner_radius(7)
}
fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(10.0)
            .color(MUTED)
            .extra_letter_spacing(1.5),
    );
}
fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.13))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .corner_radius(5)
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color));
        });
}
fn status_dot(ui: &mut egui::Ui, text: &str, connected: bool) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 3.5, if connected { LIME } else { AMBER });
        ui.label(
            RichText::new(text)
                .size(12.0)
                .color(if connected { LIME } else { MUTED }),
        );
    });
}
fn busy_mark(ui: &mut egui::Ui) {
    // Unlike egui's Spinner, this does not request continuous redraws.
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
    let center = rect.center();
    ui.painter()
        .circle_stroke(center, 5.5, Stroke::new(1.5_f32, LINE));
    let angle = ui.input(|i| i.time) as f32 * 4.0;
    ui.painter().circle_filled(
        center + Vec2::new(angle.cos(), angle.sin()) * 5.5,
        2.0,
        LIME,
    );
}
fn ability(ui: &mut egui::Ui, value: Option<u16>, potential: bool) {
    if let Some(value) = value {
        let color = if value >= 170 {
            LIME
        } else if value >= 140 {
            INK
        } else {
            MUTED
        };
        ui.vertical(|ui| {
            ui.add_space(5.0);
            ui.label(
                RichText::new(value.to_string())
                    .size(16.0)
                    .strong()
                    .color(color),
            );
            let (rect, _) = ui.allocate_exact_size(Vec2::new(43.0, 3.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, LINE);
            let fill = egui::Rect::from_min_size(
                rect.min,
                Vec2::new(43.0 * (value as f32 / 200.0).clamp(0.0, 1.0), 3.0),
            );
            ui.painter().rect_filled(
                fill,
                2.0,
                if potential {
                    LIME.gamma_multiply(0.75)
                } else {
                    Color32::from_rgb(113, 167, 171)
                },
            );
        });
    } else {
        ui.label(RichText::new("—").color(MUTED));
    }
}
fn feature_card(ui: &mut egui::Ui, title: &str, body: &str, sub: &str) {
    egui::Frame::new()
        .fill(CARD)
        .inner_margin(18)
        .corner_radius(9)
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 2.0).max(100.0));
            eyebrow(ui, title);
            ui.add_space(12.0);
            ui.label(RichText::new(body).size(18.0).strong());
            ui.add_space(5.0);
            ui.label(RichText::new(sub).size(11.0).color(MUTED));
        });
}
fn format_number(value: f64) -> String {
    let raw = if value.fract().abs() < 0.000001 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    let (integer, decimal) = raw
        .split_once('.')
        .map(|(a, b)| (a, Some(b)))
        .unwrap_or((&raw, None));
    let mut output = String::new();
    let digits = integer.strip_prefix('-').unwrap_or(integer);
    if integer.starts_with('-') {
        output.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            output.push(',');
        }
        output.push(c);
    }
    if let Some(decimal) = decimal {
        output.push('.');
        output.push_str(decimal);
    }
    output
}

fn short_club(name: &str) -> String {
    name.replace("우니오네 스포르티바 ", "")
        .replace("풋볼 클럽", "FC")
}
fn attribute_color(v: f64) -> Color32 {
    if v >= 16.0 {
        LIME
    } else if v >= 13.0 {
        AMBER
    } else if v >= 10.0 {
        INK
    } else {
        MUTED
    }
}
fn tab_button(ui: &mut egui::Ui, active: bool, text: &str) -> bool {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .size(12.0)
                .color(if active { LIME } else { MUTED }),
        )
        .fill(if active { CARD } else { BG })
        .stroke(Stroke::NONE)
        .corner_radius(5),
    )
    .clicked()
}
fn option_card(ui: &mut egui::Ui, mark: &str, title: &str, sub: &str) -> bool {
    let frame = egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(8)
        .inner_margin(15)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            eyebrow(ui, mark);
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{title}  ›")).size(16.0).strong());
            ui.add_space(3.0);
            ui.add(egui::Label::new(RichText::new(sub).size(11.0).color(MUTED)).wrap());
        });
    let response = ui.interact(
        frame.response.rect,
        ui.id().with((mark, title)),
        egui::Sense::click(),
    );
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            8,
            Stroke::new(1.0_f32, LIME),
            egui::StrokeKind::Inside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}
fn city_emblem(ui: &mut egui::Ui, size: Vec2) {
    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 8, Color32::from_rgb(28, 47, 48));
    for (x, w, h) in [
        (0.1, 0.2, 0.44),
        (0.31, 0.21, 0.71),
        (0.54, 0.13, 0.52),
        (0.69, 0.21, 0.37),
    ] {
        let rect = egui::Rect::from_min_size(
            egui::pos2(r.left() + size.x * x, r.bottom() - size.y * (h + 0.13)),
            Vec2::new(size.x * w, size.y * h),
        );
        p.rect_filled(rect, 2, Color32::from_rgb(92, 125, 112));
        for row in 0..3 {
            for col in 0..2 {
                p.rect_filled(
                    egui::Rect::from_min_size(
                        rect.min + Vec2::new(5.0 + col as f32 * 9.0, 7.0 + row as f32 * 12.0),
                        Vec2::splat(4.0),
                    ),
                    0,
                    LIME.gamma_multiply(0.6),
                );
            }
        }
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    fn entity(id: u32, kind: EntityKind, club: Option<u32>) -> Entity {
        Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id,
            kind,
            name: format!("대상 {id}"),
            english_name: format!("Entity {id}"),
            club: "테스트 구단".into(),
            club_id: club,
            age: Some(20),
            ca: Some(150),
            pa: Some(180),
            role: "MC".into(),
            group: "1군".into(),
            address: 0,
            base: 0,
            index: None,
        }
    }
    fn editor() -> Editor {
        let mut e = Editor::new(None);
        e.index = Some(Arc::new(Index {
            club_leagues: Vec::new(),
            session: Session {
                pid: 0,
                created: 0,
                module_base: 0,
                module_size: 0,
                module_stamp: 0,
                db: 0,
                registry_begin: 0,
                game_date: "2027-11-05".into(),
                profile: "fixture".into(),
                own_club_id: Some(77),
            },
            entities: vec![
                entity(1, EntityKind::Player, Some(77)),
                entity(2, EntityKind::Staff, Some(77)),
                entity(77, EntityKind::Club, Some(77)),
                entity(3, EntityKind::City, Some(77)),
            ],
            warnings: vec![],
        }));
        e.prefs.kind = EntityKind::Player;
        e.route = Route::List {
            kind: EntityKind::Player,
            club_id: None,
        };
        e.routes = vec![e.route.clone()];
        e.route_cursor = 0;
        e
    }
    #[test]
    fn profile_club_roster_and_back_forward_keep_identity() {
        let mut e = editor();
        e.select(0);
        assert_eq!(e.current_entity().unwrap().id, 1);
        e.open_entity(EntityKind::Club, 77);
        e.set_tab("선수단");
        assert_eq!(
            e.route,
            Route::Profile {
                index: 2,
                tab: "선수단".into(),
                origin_club: None
            }
        );
        e.select(0);
        e.history_go(2);
        assert_eq!(e.current_entity().unwrap().kind, EntityKind::Club);
        assert_eq!(
            e.route,
            Route::Profile {
                index: 2,
                tab: "선수단".into(),
                origin_club: None
            }
        );
        e.history_go(1);
        assert_eq!(e.current_entity().unwrap().id, 1);
        e.history_go(2);
        assert_eq!(e.current_entity().unwrap().id, 77);
        let club = e.current_entity().unwrap();
        e.open_city(&club);
        assert_eq!(e.current_entity().unwrap().id, 3);
    }
    #[test]
    fn pending_edits_block_navigation_until_discard() {
        let mut e = editor();
        e.select(0);
        e.draft.insert("ca".into(), 155.0);
        let before = e.route.clone();
        e.open_entity(EntityKind::Club, 77);
        assert_eq!(e.route, before);
        assert!(e.pending_route.is_some());
        e.draft.clear();
        let route = e.pending_route.take().unwrap();
        e.navigate(route);
        assert_eq!(e.current_entity().unwrap().id, 77);
        e.draft.insert("club.cash".into(), 1.0);
        e.history_go(1);
        assert_eq!(e.current_entity().unwrap().id, 77);
        assert_eq!(e.pending_history, Some(1));
    }
    #[test]
    fn offline_details_are_cached_and_never_applied() {
        let mut e = editor();
        let detail = Detail {
            entity: e.index.as_ref().unwrap().entities[0].clone(),
            fields: vec![],
            guards: vec![],
            notes: vec!["snapshot".into()],
        };
        e.detail_cache.insert((EntityKind::Player, 1), detail);
        e.select(0);
        assert_eq!(e.detail.as_ref().unwrap().notes[0], "snapshot");
        e.draft.insert("ca".into(), 160.0);
        e.apply();
        assert!(e.busy.is_none());
        assert!(!e.connected);
        e.draft.clear();
        e.busy = Some(Busy::Apply);
        let before = e.route.clone();
        e.open_entity(EntityKind::Club, 77);
        assert_eq!(e.route, before);
    }
    #[test]
    fn profile_rounding_does_not_mutate_precise_attribute() {
        let mut e = editor();
        let f = Field {
            key: "passing".into(),
            label: "패스".into(),
            section: "기술".into(),
            value: 12.4,
            min: 0.0,
            max: 20.0,
            step: 0.2,
            hint: "".into(),
            binding: None,
        };
        let ctx = egui::Context::default();
        typography::configure(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| e.field_row(ui, &f));
        });
        assert!(e.draft.is_empty());
        assert_eq!(f.value, 12.4);
    }
    #[test]
    fn shared_city_preserves_entry_club_and_refresh_keeps_draft() {
        let mut e = editor();
        Arc::make_mut(e.index.as_mut().unwrap()).entities[3].club_id = Some(88);
        e.open_entity(EntityKind::Club, 77);
        let club = e.current_entity().unwrap();
        e.detail = Some(Detail {
            entity: club.clone(),
            fields: vec![Field {
                key: "club.city_id".into(),
                label: "도시 ID".into(),
                section: "연결".into(),
                value: 3.0,
                min: 0.0,
                max: u32::MAX as f64,
                step: 1.0,
                hint: "".into(),
                binding: None,
            }],
            guards: vec![],
            notes: vec![],
        });
        e.open_city(&club);
        assert!(matches!(
            e.route,
            Route::Profile {
                origin_club: Some(77),
                ..
            }
        ));
        e.history_go(1);
        e.history_go(2);
        assert!(matches!(
            e.route,
            Route::Profile {
                origin_club: Some(77),
                ..
            }
        ));
        e.connected = true;
        e.draft.insert("city.attraction".into(), 19.0);
        e.load_detail();
        assert!(e.busy.is_none());
        assert_eq!(e.draft["city.attraction"], 19.0);
    }
    #[test]
    fn global_city_list_does_not_inherit_previous_club() {
        let mut e = editor();
        e.open_entity(EntityKind::Club, 77);
        e.navigate(Route::List {
            kind: EntityKind::City,
            club_id: None,
        });
        e.select(3);
        assert!(matches!(
            e.route,
            Route::Profile {
                origin_club: None,
                ..
            }
        ));
    }
    #[test]
    fn real_option_card_click_changes_club_tab() {
        let mut e = editor();
        e.open_entity(EntityKind::Club, 77);
        let ctx = egui::Context::default();
        let mut card = egui::Rect::NOTHING;
        let mut draw = |input: egui::RawInput| {
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if option_card(ui, "01", "선수단", "선수단 상세 목록") {
                        e.set_tab("선수단");
                    }
                    card = ui.min_rect();
                });
            });
        };
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 600.0),
            )),
            ..Default::default()
        };
        draw(input());
        drop(draw);
        let pos = card.min + Vec2::new(35.0, 35.0);
        for pressed in [true, false] {
            let mut raw = input();
            raw.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if option_card(ui, "01", "선수단", "선수단 상세 목록") {
                        e.set_tab("선수단");
                    }
                });
            });
        }
        assert!(matches!(e.route,Route::Profile{ref tab,..} if tab=="선수단"));
    }
}
