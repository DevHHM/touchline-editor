//! Read-only, click-through native coach panel, explicitly pinned by the user.
//! No current-screen person pointer has been verified for this FM build.
use crate::{
    backend, fields,
    model::{Detail, Entity, EntityKind, Session},
};
#[path = "overlay_context.rs"]
mod context;
use context::Anchor;
use eframe::egui::{self, Color32, RichText, Stroke};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const VIEWPORT: &str = "touchline_coach_readonly_overlay";
const SIZE: [f32; 2] = [440., 524.];
const REFRESH: Duration = Duration::from_secs(2);
const STALE: Duration = Duration::from_secs(5);
const WINDOW_POLL: Duration = Duration::from_millis(200);
const GREEN: Color32 = Color32::from_rgb(171, 224, 117);
const MUTED: Color32 = Color32::from_rgb(157, 175, 190);
const INK: Color32 = Color32::from_rgb(233, 238, 243);

#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionKey {
    pid: u32,
    created: u64,
    module: u64,
    stamp: u32,
    db: u64,
    registry: u64,
}
impl From<&Session> for SessionKey {
    fn from(s: &Session) -> Self {
        Self {
            pid: s.pid,
            created: s.created,
            module: s.module_base,
            stamp: s.module_stamp,
            db: s.db,
            registry: s.registry_begin,
        }
    }
}
#[derive(Clone)]
struct Pin {
    session: Session,
    key: SessionKey,
    entity: Entity,
}
struct Shared {
    enabled: bool,
    pin: Option<Pin>,
    detail: Option<Arc<Detail>>,
    generation: u64,
    reading: bool,
    refreshed: Option<Instant>,
    last_request: Option<Instant>,
    status: String,
    anchor: Anchor,
    offsets: [f32; 2],
    ctx: Option<egui::Context>,
    refresh_count: u64,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            enabled: false,
            pin: None,
            detail: None,
            generation: 0,
            reading: false,
            refreshed: None,
            last_request: None,
            status: "스태프 상세에서 코치를 고정하세요".into(),
            anchor: Anchor::TopRight,
            offsets: [8., 56.],
            ctx: None,
            refresh_count: 0,
        }
    }
}

/// Window following/hiding and memory refresh do not depend on editor redraws.
/// A minimized editor therefore cannot leave a topmost panel over another app.
fn start_monitor(shared: &Arc<Mutex<Shared>>) {
    let weak = Arc::downgrade(shared);
    std::thread::spawn(move || {
        let mut window_was_active = false;
        loop {
            let Some(shared) = weak.upgrade() else {
                break;
            };
            let request = {
                let mut s = shared.lock().unwrap();
                if s.enabled && !s.reading && s.last_request.is_none_or(|t| t.elapsed() >= REFRESH)
                {
                    s.pin.clone().map(|pin| {
                        s.reading = true;
                        s.last_request = Some(Instant::now());
                        (pin, s.generation)
                    })
                } else {
                    None
                }
            };
            if let Some((pin, generation)) = request {
                let weak = Arc::downgrade(&shared);
                std::thread::spawn(move || {
                    let result =
                        backend::detail(&pin.session, &pin.entity).and_then(validated_detail);
                    if let Some(shared) = weak.upgrade() {
                        let mut s = shared.lock().unwrap();
                        if s.generation == generation {
                            s.reading = false;
                            match result {
                                Ok(d) => {
                                    s.refresh_count += 1;
                                    s.detail = Some(Arc::new(d));
                                    s.refreshed = Some(Instant::now());
                                    s.status = "조회 완료 · FM을 활성화하면 표시됩니다".into();
                                }
                                Err(e) => {
                                    s.enabled = false;
                                    s.pin = None;
                                    s.detail = None;
                                    s.refreshed = None;
                                    s.status = format!("표시 중단: {e:#}");
                                }
                            }
                            let repaint = s.ctx.clone();
                            drop(s);
                            if let Some(ctx) = repaint {
                                ctx.request_repaint_of(egui::ViewportId::ROOT);
                                ctx.request_repaint_of(egui::ViewportId::from_hash_of(VIEWPORT));
                            }
                        }
                    }
                });
            }
            let (target, anchor, offsets, generation, enabled) = {
                let s = shared.lock().unwrap();
                let target = if s.enabled && s.refreshed.is_some_and(|t| t.elapsed() < STALE) {
                    s.pin
                        .as_ref()
                        .and_then(|p| context::foreground_target(p.session.pid))
                } else {
                    None
                };
                (target, s.anchor, s.offsets, s.generation, s.enabled)
            };
            // Window messages may cross threads. Never hold state while calling SetWindowPos.
            if enabled || window_was_active {
                context::position_overlay(&CoachOverlay::title(), target, SIZE, anchor, offsets);
            }
            window_was_active = enabled;
            let invalidated = {
                let s = shared.lock().unwrap();
                !s.enabled || s.generation != generation
            };
            if invalidated && enabled {
                context::position_overlay(&CoachOverlay::title(), None, SIZE, anchor, offsets);
            }
            drop(shared);
            std::thread::sleep(if enabled {
                WINDOW_POLL
            } else {
                Duration::from_millis(500)
            });
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScoreGroup {
    Training,
    Management,
    Support,
}
impl ScoreGroup {
    fn label(self) -> &'static str {
        match self {
            Self::Training => "코치 · 훈련",
            Self::Management => "수석 · 유소년",
            Self::Support => "영입 · 지원",
        }
    }
    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Training => &[
                "technical_attack",
                "tactical_attack",
                "technical_defense",
                "tactical_defense",
                "technical_mental",
                "tactical_mental",
                "fitness",
                "gk_shot",
                "gk_distribution",
                "set_pieces",
            ],
            Self::Management => &[
                "assistant",
                "general",
                "reserve_manager",
                "youth_manager",
                "youth_assistant",
                "youth_coach",
                "youth_goalkeeper",
                "head_youth",
            ],
            Self::Support => &[
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
        }
    }
}

pub struct CoachOverlay {
    shared: Arc<Mutex<Shared>>,
    pinned_name: Option<String>,
    status: String,
    anchor: Anchor,
    offsets: [f32; 2],
    group: ScoreGroup,
    precise: bool,
}
impl Default for CoachOverlay {
    fn default() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        start_monitor(&shared);
        Self {
            shared,
            pinned_name: None,
            status: "스태프 상세에서 코치를 고정하세요".into(),
            anchor: Anchor::TopRight,
            offsets: [8., 56.],
            group: ScoreGroup::Training,
            precise: false,
        }
    }
}
impl CoachOverlay {
    fn title() -> String {
        format!("Touchline Coach Overlay · {}", std::process::id())
    }
    pub fn is_enabled(&self) -> bool {
        self.shared.lock().unwrap().enabled
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn pinned_name(&self) -> Option<&str> {
        self.pinned_name.as_deref()
    }
    pub fn clear(&mut self) {
        let mut s = self.shared.lock().unwrap();
        s.enabled = false;
        s.pin = None;
        s.detail = None;
        s.reading = false;
        s.refreshed = None;
        s.last_request = None;
        s.generation = s.generation.wrapping_add(1);
        s.ctx = None;
        self.pinned_name = None;
        drop(s);
        context::position_overlay(&Self::title(), None, SIZE, self.anchor, self.offsets);
    }
    pub fn pin(&mut self, session: &Session, detail: &Detail) -> anyhow::Result<()> {
        anyhow::ensure!(
            detail.entity.kind == EntityKind::Staff && detail.entity.id > 0,
            "스태프 상세를 선택하세요"
        );
        self.clear();
        let mut s = self.shared.lock().unwrap();
        s.pin = Some(Pin {
            session: session.clone(),
            key: SessionKey::from(session),
            entity: detail.entity.clone(),
        });
        s.enabled = true;
        self.pinned_name = Some(detail.entity.name.clone());
        self.status = "고정한 코치의 메모리 값을 다시 확인하는 중".into();
        s.status = self.status.clone();
        Ok(())
    }
    pub fn controls(
        &mut self,
        ui: &mut egui::Ui,
        session: Option<&Session>,
        detail: Option<&Detail>,
        live: bool,
    ) {
        ui.strong("FM 코치 오버레이");
        ui.label(
            RichText::new("선택한 코치를 고정해 FM 창 위에 표시합니다.")
                .color(MUTED)
                .size(12.),
        );
        ui.label(
            RichText::new("FM에서 다른 인물을 열어도 대상은 자동으로 바뀌지 않습니다.")
                .color(MUTED)
                .size(11.),
        );
        let selected = detail.filter(|d| d.entity.kind == EntityKind::Staff);
        let label = selected
            .map(|d| format!("{} 고정", d.entity.name))
            .unwrap_or_else(|| "스태프 상세에서 고정".into());
        if ui
            .add_enabled(
                live && session.is_some() && selected.is_some(),
                egui::Button::new(label),
            )
            .clicked()
        {
            if let (Some(session), Some(detail)) = (session, selected) {
                if let Err(e) = self.pin(session, detail) {
                    self.status = e.to_string();
                }
            }
        }
        ui.horizontal(|ui| {
            let mut enabled = self.is_enabled();
            let r = ui.add_enabled(
                self.pinned_name.is_some() && live,
                egui::Checkbox::new(&mut enabled, "FM 위에 표시"),
            );
            if r.changed() {
                self.shared.lock().unwrap().enabled = enabled;
            }
            if ui
                .add_enabled(self.pinned_name.is_some(), egui::Button::new("고정 해제"))
                .clicked()
            {
                self.clear();
                self.status = "고정을 해제했습니다".into();
            }
        });
        if let Some(name) = self.pinned_name() {
            ui.label(RichText::new(format!("고정 코치  {name}")).color(GREEN));
        }
        ui.label(RichText::new(&self.status).color(MUTED).size(11.));
        egui::ComboBox::from_id_salt("overlay_anchor")
            .selected_text(self.anchor.label())
            .show_ui(ui, |ui| {
                for anchor in Anchor::ALL {
                    ui.selectable_value(&mut self.anchor, anchor, anchor.label());
                }
            });
        egui::ComboBox::from_id_salt("overlay_score_group")
            .selected_text(self.group.label())
            .show_ui(ui, |ui| {
                for group in [
                    ScoreGroup::Training,
                    ScoreGroup::Management,
                    ScoreGroup::Support,
                ] {
                    ui.selectable_value(&mut self.group, group, group.label());
                }
            });
        ui.horizontal(|ui| {
            ui.label("여백");
            ui.add(
                egui::DragValue::new(&mut self.offsets[0])
                    .range(0.0..=1000.)
                    .prefix("가로 "),
            );
            ui.add(
                egui::DragValue::new(&mut self.offsets[1])
                    .range(0.0..=1000.)
                    .prefix("세로 "),
            );
        });
        ui.checkbox(&mut self.precise, "내부 소수 능력치 표시");
        ui.small("클릭은 FM으로 통과 · 다른 앱으로 전환하면 숨김 · 2초마다 읽기");
        ui.small("창 모드 / 테두리 없는 창 권장. FM 독점 전체 화면은 표시되지 않을 수 있습니다.");
        let mut s = self.shared.lock().unwrap();
        s.anchor = self.anchor;
        s.offsets = self.offsets;
    }
    /// Must run on every editor frame, including frames where another entity is selected.
    pub fn update(&mut self, ctx: &egui::Context, session: Option<&Session>, live: bool) {
        let stale_session = self
            .shared
            .lock()
            .unwrap()
            .pin
            .as_ref()
            .is_some_and(|p| !live || session.map(SessionKey::from).as_ref() != Some(&p.key));
        if stale_session {
            self.clear();
            self.status = "연결/세이브가 바뀌어 고정을 해제했습니다".into();
            self.shared.lock().unwrap().status = self.status.clone();
        }
        let show = {
            let mut s = self.shared.lock().unwrap();
            s.ctx = Some(ctx.clone());
            self.status = s.status.clone();
            self.pinned_name = s.pin.as_ref().map(|p| p.entity.name.clone());
            s.enabled && s.detail.is_some()
        };
        if !self.is_enabled() {
            return;
        }
        ctx.request_repaint_after(WINDOW_POLL);
        if !show {
            return;
        }
        let shared = self.shared.clone();
        let group = self.group;
        let precise = self.precise;
        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(VIEWPORT),
            egui::ViewportBuilder::default()
                .with_title(Self::title())
                .with_inner_size(SIZE)
                .with_active(false)
                .with_visible(false)
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_mouse_passthrough(true)
                .with_window_level(egui::WindowLevel::AlwaysOnTop),
            move |ctx, class| {
                if class == egui::ViewportClass::Embedded {
                    return;
                }
                context::cache_overlay(&CoachOverlay::title());
                ctx.request_repaint_after(WINDOW_POLL);
                let snapshot = {
                    let s = shared.lock().unwrap();
                    if s.enabled && s.refreshed.is_some_and(|t| t.elapsed() < STALE) {
                        s.detail.clone()
                    } else {
                        None
                    }
                };
                let Some(detail) = snapshot else {
                    return;
                };
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(Color32::from_rgb(16, 27, 39))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(65, 91, 104)))
                            .inner_margin(14),
                    )
                    .show(ctx, |ui| {
                        render_panel(ui, &detail, group, precise);
                    });
            },
        );
    }
}
impl Drop for CoachOverlay {
    fn drop(&mut self) {
        self.clear();
    }
}

fn validated_detail(mut d: Detail) -> anyhow::Result<Detail> {
    anyhow::ensure!(d.entity.kind == EntityKind::Staff, "스태프 객체가 아닙니다");
    let attributes = d
        .fields
        .iter()
        .filter(|f| f.key.starts_with("staff_"))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        attributes.len() == 23
            && attributes
                .iter()
                .all(|f| f.value.is_finite() && (0.0..=20.).contains(&f.value)),
        "검증된 코치 능력치 23개가 모두 필요합니다"
    );
    // Recompute from the fresh read only, never from editor drafts or old score fields.
    d.fields.retain(|f| !f.key.starts_with("score_"));
    fields::append_scores(&mut d);
    Ok(d)
}

fn numeric(value: f64, precise: bool) -> String {
    if precise && (value - value.round()).abs() > 0.001 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}
fn attribute_color(value: f64) -> Color32 {
    if value >= 16. {
        GREEN
    } else if value >= 12. {
        Color32::from_rgb(244, 204, 110)
    } else {
        MUTED
    }
}
fn row(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 17.), egui::Sense::hover());
    ui.painter().text(
        r.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.5),
        INK,
    );
    ui.painter().text(
        r.right_center(),
        egui::Align2::RIGHT_CENTER,
        value,
        crate::typography::title(13.),
        color,
    );
}
fn render_panel(ui: &mut egui::Ui, d: &Detail, group: ScoreGroup, precise: bool) {
    ui.spacing_mut().item_spacing = egui::vec2(10., 2.);
    ui.label(
        RichText::new("TOUCHLINE  /  고정 코치")
            .color(GREEN)
            .size(11.),
    );
    ui.label(
        RichText::new(&d.entity.name)
            .color(INK)
            .font(crate::typography::title(22.)),
    );
    ui.label(
        RichText::new(format!(
            "{}  ·  {}  ·  ID {}",
            d.entity.club, d.entity.role, d.entity.id
        ))
        .color(MUTED)
        .size(11.),
    );
    ui.add_space(4.);
    ui.separator();
    ui.label(
        RichText::new("코칭 / 관리 능력치")
            .color(GREEN)
            .strong()
            .size(12.),
    );
    let attributes = d
        .fields
        .iter()
        .filter(|f| f.key.starts_with("staff_"))
        .collect::<Vec<_>>();
    ui.columns(2, |columns| {
        for (i, field) in attributes.iter().enumerate() {
            row(
                &mut columns[i / 12],
                &field.label,
                &numeric(field.value, precise),
                attribute_color(field.value),
            );
        }
    });
    ui.add_space(5.);
    ui.separator();
    ui.label(
        RichText::new(format!("{}  /  사용자 점수", group.label()))
            .color(GREEN)
            .strong()
            .size(12.),
    );
    ui.columns(2, |columns| {
        for (i, key) in group.keys().iter().enumerate() {
            if let Some(f) = d.fields.iter().find(|f| f.key == format!("score_{key}")) {
                row(
                    &mut columns[i % 2],
                    &f.label,
                    &numeric(f.value, true),
                    if f.value >= 240. { GREEN } else { INK },
                );
            }
        }
    });
    ui.add_space(7.);
    ui.separator();
    ui.label(
        RichText::new("읽기 전용 · 2초 갱신 · 클릭은 FM으로 통과")
            .color(MUTED)
            .size(10.5),
    );
    ui.label(
        RichText::new("FM 프로필 자동 추적 아님 · 대상 변경/종료는 에디터에서")
            .color(MUTED)
            .size(10.5),
    );
}

/// Native overlay diagnostic; the host window stays hidden and FM receives no input.
/// It reads the pinned coach fresh, records actual Win32 styles, then closes in ~3 s.
pub fn run_native_check(
    session: Session,
    detail: Detail,
    output: std::path::PathBuf,
) -> eframe::Result<()> {
    let mut overlay = CoachOverlay::default();
    overlay
        .pin(&session, &detail)
        .expect("--overlay-check requires staff detail");
    let foreground_before = context::foreground_id();
    let foreground_pid_before = context::foreground_pid();
    eframe::run_native(
        "Touchline Overlay Check",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Touchline Overlay Check")
                .with_visible(false)
                .with_active(false)
                .with_taskbar(false),
            ..Default::default()
        },
        Box::new(move |cc| {
            configure_preview(&cc.egui_ctx);
            Ok(Box::new(CheckApp {
                overlay,
                session,
                output,
                start: Instant::now(),
                foreground_before,
                foreground_pid_before,
                own_focus_observed: false,
                written: false,
            }))
        }),
    )
}
struct CheckApp {
    overlay: CoachOverlay,
    session: Session,
    output: std::path::PathBuf,
    start: Instant,
    foreground_before: usize,
    foreground_pid_before: u32,
    own_focus_observed: bool,
    written: bool,
}
impl eframe::App for CheckApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.overlay.update(ctx, Some(&self.session), true);
        self.own_focus_observed |= context::foreground_pid() == std::process::id();
        ctx.request_repaint_after(Duration::from_millis(100));
        if !self.written && self.start.elapsed() >= Duration::from_secs(3) {
            let before_hide = context::diagnostic(&CoachOverlay::title(), self.session.pid);
            let (coach_id, fresh_detail_read, refresh_count) = {
                let mut s = self.overlay.shared.lock().unwrap();
                s.enabled = false;
                (
                    s.pin.as_ref().map(|p| p.entity.id),
                    s.detail.is_some(),
                    s.refresh_count,
                )
            };
            context::position_overlay(
                &CoachOverlay::title(),
                None,
                SIZE,
                self.overlay.anchor,
                self.overlay.offsets,
            );
            let after_hide = context::diagnostic(&CoachOverlay::title(), self.session.pid);
            let report = serde_json::json!({
                "game_values_changed": false, "game_input_sent": false,
                "coach_id": coach_id,
                "fresh_detail_read": fresh_detail_read, "status": self.overlay.status(),
                "foreground_before": self.foreground_before,
                "foreground_pid_before": self.foreground_pid_before,
                "foreground_pid_after": context::foreground_pid(),
                "own_process_focus_observed": self.own_focus_observed,
                "foreground_preserved": self.foreground_before == context::foreground_id(),
                "live_refresh_count_while_root_hidden": refresh_count,
                "native_window": before_hide, "after_explicit_hide": after_hide,
            });
            if let Some(parent) = self.output.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&self.output, serde_json::to_vec_pretty(&report).unwrap());
            self.written = true;
            self.overlay.clear();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Own-window visual QA: no FM connection or game reads/writes.
pub fn run_preview(detail: Detail, output: std::path::PathBuf) -> eframe::Result<()> {
    let detail =
        validated_detail(detail).expect("--overlay-preview requires complete staff detail");
    eframe::run_native(
        "Touchline Coach Preview",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Touchline Coach Preview")
                .with_inner_size(SIZE)
                .with_active(false),
            ..Default::default()
        },
        Box::new(move |cc| {
            configure_preview(&cc.egui_ctx);
            Ok(Box::new(PreviewApp {
                detail,
                output,
                frames: 0,
                requested: false,
            }))
        }),
    )
}
fn configure_preview(ctx: &egui::Context) {
    crate::typography::configure(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);
}
struct PreviewApp {
    detail: Detail,
    output: std::path::PathBuf,
    frames: usize,
    requested: bool,
}
impl eframe::App for PreviewApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(16, 27, 39))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(65, 91, 104)))
                    .inner_margin(14),
            )
            .show(ctx, |ui| {
                render_panel(ui, &self.detail, ScoreGroup::Training, false)
            });
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image: pixels, .. } = event {
                let bytes: Vec<u8> = pixels.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Some(parent) = self.output.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = image::save_buffer(
                    &self.output,
                    &bytes,
                    pixels.size[0] as u32,
                    pixels.size[1] as u32,
                    image::ColorType::Rgba8,
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.frames += 1;
        if self.frames >= 4 && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_change_never_reuses_old_pin() {
        let s = Session {
            pid: 1,
            created: 2,
            module_base: 3,
            module_size: 4,
            module_stamp: 5,
            db: 6,
            registry_begin: 7,
            game_date: "2027-11-14".into(),
            profile: "test".into(),
            own_club_id: None,
        };
        let a = SessionKey::from(&s);
        let mut other = s.clone();
        other.game_date = "2027-11-15".into();
        assert_eq!(a, SessionKey::from(&other));
        other.created += 1;
        assert_ne!(a, SessionKey::from(&other));
        other = s.clone();
        other.db += 1;
        assert_ne!(a, SessionKey::from(&other));
        other = s.clone();
        other.registry_begin += 8;
        assert_ne!(a, SessionKey::from(&other));
    }
    #[test]
    fn overlay_uses_same_scores_as_editor_and_rejects_missing_attributes() {
        let entity = Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id: 1,
            kind: EntityKind::Staff,
            name: "Test".into(),
            english_name: "Test".into(),
            club: "Lecce".into(),
            club_id: Some(1141),
            age: Some(50),
            ca: None,
            pa: None,
            role: "Coach".into(),
            group: String::new(),
            address: 1,
            base: 1,
            index: Some(0),
        };
        let keys = [
            "discipline",
            "youth",
            "determination",
            "gk_shot",
            "judging_ability",
            "judging_potential",
            "management",
            "motivation",
            "physiotherapy",
            "knowledge",
            "attack",
            "defense",
            "fitness",
            "mental",
            "technical",
            "tactical",
            "gk_handling",
            "gk_distribution",
            "analysis",
            "sports_science",
            "negotiation",
            "judging_staff",
            "set_pieces",
        ];
        let mut d = Detail {
            entity,
            fields: keys
                .iter()
                .map(|k| crate::model::Field {
                    key: format!("staff_{k}"),
                    label: k.to_string(),
                    section: "staff".into(),
                    value: 20.,
                    min: 0.,
                    max: 20.,
                    step: 1.,
                    hint: String::new(),
                    binding: None,
                })
                .collect(),
            guards: vec![],
            notes: vec![],
        };
        fields::append_scores(&mut d);
        let expected = 300.;
        d.fields
            .iter_mut()
            .find(|f| f.key == "score_assistant")
            .unwrap()
            .value = 999.;
        let actual = validated_detail(d.clone()).unwrap();
        assert_eq!(
            actual
                .fields
                .iter()
                .find(|f| f.key == "score_assistant")
                .unwrap()
                .value,
            expected
        );
        d.fields.retain(|f| f.key != "staff_discipline");
        assert!(validated_detail(d).is_err());
    }
}
