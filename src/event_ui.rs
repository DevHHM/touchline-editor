//! Native scenario studio. It never drives FM's UI or progresses the game.
use crate::{
    events::{self, EventContext, EventDraft, EventPlan, EventReceipt},
    model::{EntityKind, Index},
};
use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke, Vec2};
use std::{path::PathBuf, sync::mpsc, time::Duration};

const PANEL: Color32 = Color32::from_rgb(20, 31, 41);
const CARD: Color32 = Color32::from_rgb(26, 40, 51);
const LINE: Color32 = Color32::from_rgb(43, 60, 72);
const INK: Color32 = Color32::from_rgb(235, 237, 227);
const MUTED: Color32 = Color32::from_rgb(143, 161, 171);
const LIME: Color32 = Color32::from_rgb(199, 232, 114);
const AMBER: Color32 = Color32::from_rgb(238, 190, 111);
const RED: Color32 = Color32::from_rgb(239, 132, 135);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Compose,
    Drafts,
    History,
}
#[derive(Clone, Copy, PartialEq)]
enum Busy {
    Capture,
    Apply,
    Undo,
    Save,
}
enum Reply {
    Captured(u64, Result<EventContext, String>),
    Applied(u64, Result<EventReceipt, String>),
    Undone(u64, Result<EventReceipt, String>),
    Saved(u64, Result<PathBuf, String>),
}
#[derive(Clone)]
enum Confirmation {
    Apply(EventPlan),
    Undo(EventReceipt),
}

pub struct EventStudio {
    context: Option<EventContext>,
    draft: EventDraft,
    preview: Option<EventPlan>,
    page: Page,
    busy: Option<Busy>,
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
    generation: u64,
    selected_club: Option<u32>,
    route_club_seen: Option<u32>,
    club_search: String,
    enabled_sponsor: bool,
    enabled_cash: bool,
    enabled_transfer: bool,
    enabled_wage: bool,
    enabled_city: bool,
    enabled_fx: bool,
    cash_amount: i64,
    transfer_amount: i64,
    wage_amount: i64,
    population: u8,
    attraction: u8,
    exchange_rate: f64,
    confirm: Option<Confirmation>,
    saved: Vec<EventPlan>,
    history: Vec<EventReceipt>,
    notice: Option<(String, bool)>,
    preview_error: Option<String>,
    pending_capture: bool,
}

impl Default for EventStudio {
    fn default() -> Self {
        Self::new()
    }
}
impl EventStudio {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut out = Self {
            context: None,
            draft: EventDraft::default(),
            preview: None,
            page: Page::Compose,
            busy: None,
            tx,
            rx,
            generation: 0,
            selected_club: None,
            route_club_seen: None,
            club_search: String::new(),
            enabled_sponsor: true,
            enabled_cash: false,
            enabled_transfer: false,
            enabled_wage: false,
            enabled_city: false,
            enabled_fx: false,
            cash_amount: 1_000_000,
            transfer_amount: 1_000_000,
            wage_amount: 10_000,
            population: 8,
            attraction: 13,
            exchange_rate: 1800.,
            confirm: None,
            saved: events::drafts(),
            history: events::history(),
            notice: None,
            preview_error: None,
            pending_capture: false,
        };
        let args: Vec<String> = std::env::args().collect();
        let explicit = args
            .windows(2)
            .find(|v| v[0] == "--event-context")
            .map(|v| PathBuf::from(&v[1]));
        let adjacent = args
            .windows(2)
            .find(|v| v[0] == "--snapshot")
            .and_then(|v| {
                PathBuf::from(&v[1])
                    .parent()
                    .map(|p| p.join("event-context.json"))
            });
        if let Some(path) = explicit.or(adjacent) {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(context) = serde_json::from_slice(&bytes) {
                    out.set_context(context);
                }
            }
        }
        out
    }
    pub fn reset(&mut self) {
        if self.busy_write() {
            return;
        }
        self.generation += 1;
        self.busy = None;
        self.context = None;
        self.preview = None;
        self.confirm = None;
        self.selected_club = None;
        self.route_club_seen = None;
        self.pending_capture = false;
        self.notice = None;
        self.preview_error = None;
    }
    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }
    pub fn busy_write(&self) -> bool {
        matches!(self.busy, Some(Busy::Apply | Busy::Undo))
    }
    /// Call once when navigating into the studio from a club profile. Unlike
    /// per-frame route sync, this also handles returning to the same route after
    /// the user selected another club in the studio's dropdown.
    pub fn enter_club(&mut self, club_id: Option<u32>, connected: bool) {
        if self.is_busy() {
            return;
        }
        let Some(id) = club_id else {
            return;
        };
        self.route_club_seen = Some(id);
        self.page = Page::Compose;
        self.confirm = None;
        if self.selected_club != Some(id) {
            self.selected_club = Some(id);
            self.context = None;
            self.preview = None;
            self.confirm = None;
            self.preview_error = None;
            self.notice = None;
            self.draft.sponsor_slot = None;
        }
        if self.context.is_none() {
            self.pending_capture = connected;
        }
    }
    pub fn set_context(&mut self, context: EventContext) {
        self.confirm = None;
        self.selected_club = Some(context.club.entity.id);
        if self.draft.sponsor_slot.is_none()
            || !context
                .sponsors
                .iter()
                .any(|s| Some(s.slot) == self.draft.sponsor_slot)
        {
            self.draft.sponsor_slot = context
                .sponsors
                .iter()
                .max_by_key(|s| s.annual_gbp)
                .map(|s| s.slot);
        }
        if context.sponsors.is_empty() {
            self.enabled_sponsor = false;
        }
        if let Some(city) = &context.city
            && !self.enabled_city
        {
            if let Some(f) = city.fields.iter().find(|f| f.key == "city.population_band") {
                self.population = (f.value as u8).saturating_add(1).min(15);
            }
            if let Some(f) = city.fields.iter().find(|f| f.key == "city.attraction") {
                self.attraction = (f.value as u8).saturating_add(2).min(20);
            }
        }
        self.context = Some(context);
        self.rebuild();
    }
    pub fn set_preview(&mut self, plan: EventPlan) {
        self.context = Some(plan.context.clone());
        self.selected_club = Some(plan.context.club.entity.id);
        self.load_form(&plan.draft);
        self.preview = Some(plan);
        self.preview_error = None;
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.rx.try_recv() {
            let generation = match &reply {
                Reply::Captured(g, _)
                | Reply::Applied(g, _)
                | Reply::Undone(g, _)
                | Reply::Saved(g, _) => *g,
            };
            if generation != self.generation {
                continue;
            }
            self.busy = None;
            match reply {
                Reply::Captured(_, Ok(context)) => {
                    self.set_context(context);
                    self.notice = Some((
                        "구단의 현재 계약·재정·도시 정보를 불러왔습니다.".into(),
                        false,
                    ));
                }
                Reply::Captured(_, Err(e)) => self.notice = Some((e, true)),
                Reply::Applied(_, Ok(receipt)) => {
                    let warning = receipt.status.contains("warning");
                    self.notice = Some((
                        if warning {
                            format!("이벤트가 반영됐지만 확인이 필요합니다: {}", receipt.status)
                        } else {
                            "이벤트를 반영하고 원본을 백업했습니다. 게임 저장은 FM에서 해주세요."
                                .into()
                        },
                        warning,
                    ));
                    self.history = events::history();
                    self.preview = None;
                    self.context = None;
                    self.page = Page::History;
                }
                Reply::Undone(_, Ok(receipt)) => {
                    let warning = receipt.status.contains("warning");
                    self.notice = Some((
                        if warning {
                            format!("되돌렸지만 확인이 필요합니다: {}", receipt.status)
                        } else {
                            "이벤트의 변경 항목을 원래 값으로 되돌렸습니다.".into()
                        },
                        warning,
                    ));
                    self.history = events::history();
                    self.preview = None;
                    self.context = None;
                }
                Reply::Applied(_, Err(e)) | Reply::Undone(_, Err(e)) => {
                    self.notice = Some((e, true));
                    self.history = events::history();
                }
                Reply::Saved(_, Ok(path)) => {
                    self.notice = Some((
                        format!("시나리오와 기사를 저장했습니다: {}", path.display()),
                        false,
                    ));
                    self.saved = events::drafts();
                }
                Reply::Saved(_, Err(e)) => self.notice = Some((e, true)),
            }
            ctx.request_repaint();
        }
        if self.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn load_form(&mut self, draft: &EventDraft) {
        self.confirm = None;
        self.draft = draft.clone();
        self.enabled_sponsor = draft.annual_gbp > 0;
        self.enabled_cash = draft.cash_grant_gbp > 0;
        self.enabled_transfer = draft.transfer_budget_grant_gbp > 0;
        self.enabled_wage = draft.weekly_wage_increase_gbp > 0;
        self.enabled_city = draft.population_band.is_some() || draft.attraction.is_some();
        self.enabled_fx = draft.krw_per_gbp.is_some();
        self.cash_amount = if draft.cash_grant_gbp > 0 {
            draft.cash_grant_gbp
        } else {
            1_000_000
        };
        self.transfer_amount = if draft.transfer_budget_grant_gbp > 0 {
            draft.transfer_budget_grant_gbp
        } else {
            1_000_000
        };
        self.wage_amount = if draft.weekly_wage_increase_gbp > 0 {
            draft.weekly_wage_increase_gbp
        } else {
            10_000
        };
        self.population = draft.population_band.unwrap_or(8);
        self.attraction = draft.attraction.unwrap_or(13);
        self.exchange_rate = draft.krw_per_gbp.unwrap_or(1800.);
    }
    fn effective_draft(&self) -> EventDraft {
        let mut d = self.draft.clone();
        if !self.enabled_sponsor {
            d.annual_gbp = 0;
        }
        d.cash_grant_gbp = if self.enabled_cash {
            self.cash_amount
        } else {
            0
        };
        d.transfer_budget_grant_gbp = if self.enabled_transfer {
            self.transfer_amount
        } else {
            0
        };
        d.weekly_wage_increase_gbp = if self.enabled_wage {
            self.wage_amount
        } else {
            0
        };
        d.population_band = self.enabled_city.then_some(self.population);
        d.attraction = self.enabled_city.then_some(self.attraction);
        d.krw_per_gbp = self.enabled_fx.then_some(self.exchange_rate);
        d
    }
    fn rebuild(&mut self) {
        self.confirm = None;
        self.preview = None;
        self.preview_error = None;
        if let Some(context) = &self.context {
            match events::plan(context, &self.effective_draft()) {
                Ok(p) => self.preview = Some(p),
                Err(e) => self.preview_error = Some(format!("{e:#}")),
            }
        }
    }
    fn capture(&mut self, index: &Index, club_id: u32, ctx: &egui::Context) {
        if self.is_busy() {
            return;
        }
        let Some(club) = index
            .entities
            .iter()
            .find(|e| e.kind == EntityKind::Club && e.id == club_id)
            .cloned()
        else {
            self.notice = Some(("선택한 구단을 목록에서 찾을 수 없습니다.".into(), true));
            return;
        };
        self.busy = Some(Busy::Capture);
        self.notice = None;
        self.preview = None;
        self.context = None;
        let session = index.session.clone();
        let tx = self.tx.clone();
        let generation = self.generation;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = events::capture(&session, &club).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Reply::Captured(generation, result));
            ctx.request_repaint();
        });
    }
    fn save(&mut self, ctx: &egui::Context) {
        let Some(plan) = self.preview.clone() else {
            return;
        };
        if self.is_busy() {
            return;
        }
        self.busy = Some(Busy::Save);
        let tx = self.tx.clone();
        let generation = self.generation;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = events::save_draft(&plan).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Reply::Saved(generation, result));
            ctx.request_repaint();
        });
    }
    fn commit(&mut self, confirmation: Confirmation, ctx: &egui::Context, connected: bool) {
        if self.is_busy() || !connected {
            return;
        }
        self.busy = Some(match confirmation {
            Confirmation::Apply(_) => Busy::Apply,
            Confirmation::Undo(_) => Busy::Undo,
        });
        self.confirm = None;
        self.notice = None;
        let tx = self.tx.clone();
        let generation = self.generation;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let reply = match confirmation {
                Confirmation::Apply(plan) => Reply::Applied(
                    generation,
                    events::apply(&plan).map_err(|e| format!("{e:#}")),
                ),
                Confirmation::Undo(receipt) => Reply::Undone(
                    generation,
                    events::undo(&receipt).map_err(|e| format!("{e:#}")),
                ),
            };
            let _ = tx.send(reply);
            ctx.request_repaint();
        });
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        index: Option<&Index>,
        club_id: Option<u32>,
        connected: bool,
    ) {
        self.poll(ui.ctx());
        let route_club = club_id.or_else(|| index.and_then(|i| i.session.own_club_id));
        if self.route_club_seen != route_club {
            self.enter_club(route_club, connected);
        }
        let busy = self.is_busy();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("이벤트 스튜디오")
                        .size(20.)
                        .strong()
                        .color(INK),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                pill(
                    ui,
                    if connected {
                        "FM 연결됨"
                    } else {
                        "미리보기 모드"
                    },
                    if connected { LIME } else { AMBER },
                );
            });
        });
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!busy && self.confirm.is_none(), |ui| {
                ui.selectable_value(&mut self.page, Page::Compose, "이벤트 만들기");
                ui.selectable_value(
                    &mut self.page,
                    Page::Drafts,
                    format!("저장한 시나리오  {}", self.saved.len()),
                );
                ui.selectable_value(
                    &mut self.page,
                    Page::History,
                    format!("이벤트 기록  {}", self.history.len()),
                );
            });
            if busy {
                ui.spinner();
                ui.label(
                    RichText::new(match self.busy.unwrap() {
                        Busy::Capture => "현재 구단 정보 불러오는 중",
                        Busy::Apply => "원본 백업 · 이벤트 반영 중",
                        Busy::Undo => "이벤트 되돌리는 중",
                        Busy::Save => "시나리오 저장 중",
                    })
                    .color(LIME)
                    .size(12.),
                );
            }
        });
        ui.add_space(8.);
        if let Some((notice, error)) = &self.notice {
            egui::Frame::new()
                .fill(if *error {
                    Color32::from_rgb(55, 34, 39)
                } else {
                    Color32::from_rgb(30, 49, 40)
                })
                .corner_radius(8)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(notice).size(12.).color(if *error {
                        RED
                    } else {
                        LIME
                    }));
                });
            ui.add_space(8.);
        }
        ui.add_enabled_ui(!self.is_busy() && self.confirm.is_none(), |ui| {
            match self.page {
                Page::Compose => self.compose(ui, index, connected),
                Page::Drafts => self.library(ui, index, connected),
                Page::History => self.history_page(ui, connected),
            }
        });
        self.confirmation(ui.ctx(), connected);
        if self.pending_capture && !self.is_busy() && connected {
            if let (Some(index), Some(id)) = (index, self.selected_club) {
                self.pending_capture = false;
                self.capture(index, id, ui.ctx());
            }
        }
    }
    fn compose(&mut self, ui: &mut egui::Ui, index: Option<&Index>, connected: bool) {
        let total = ui.available_width();
        let left = (total * 0.45).clamp(370., 540.);
        let gap = 8.;
        let right = (total - left - gap).max(300.);
        let height = ui.available_height().max(300.);
        let mut changed = false;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(
                Vec2::new(left, height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(left);
                    egui::ScrollArea::vertical()
                        .id_salt("event-form-scroll")
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_enabled_ui(!self.is_busy() && self.confirm.is_none(), |ui| {
                                changed |= self.form(ui, index, connected);
                            });
                        });
                },
            );
            if changed {
                self.rebuild();
            }
            ui.allocate_ui_with_layout(
                Vec2::new(right, height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(right);
                    egui::ScrollArea::vertical()
                        .id_salt("event-article-scroll")
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.article(ui, connected);
                        });
                },
            );
        });
    }
    fn form(&mut self, ui: &mut egui::Ui, index: Option<&Index>, connected: bool) -> bool {
        let mut changed = false;
        card(ui, |ui| {
            section(ui, "01", "대상과 이야기");
            let selected_name = self
                .selected_club
                .and_then(|id| {
                    index.and_then(|i| {
                        i.entities
                            .iter()
                            .find(|e| e.kind == EntityKind::Club && e.id == id)
                    })
                })
                .map(|c| c.name.as_str())
                .or_else(|| self.context.as_ref().map(|c| c.club.entity.name.as_str()))
                .unwrap_or("구단을 선택하세요")
                .to_owned();
            let before = self.selected_club;
            egui::ComboBox::from_id_salt("event-club")
                .width(ui.available_width() - 8.)
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.club_search)
                            .hint_text("구단 이름 검색")
                            .desired_width(280.),
                    );
                    if let Some(index) = index {
                        let query = self.club_search.trim().to_lowercase();
                        egui::ScrollArea::vertical()
                            .max_height(230.)
                            .show(ui, |ui| {
                                for club in index
                                    .entities
                                    .iter()
                                    .filter(|e| {
                                        e.kind == EntityKind::Club
                                            && (query.is_empty()
                                                || e.name.to_lowercase().contains(&query)
                                                || e.english_name.to_lowercase().contains(&query))
                                    })
                                    .take(120)
                                {
                                    ui.selectable_value(
                                        &mut self.selected_club,
                                        Some(club.id),
                                        &club.name,
                                    );
                                }
                            });
                    }
                });
            if before != self.selected_club {
                self.context = None;
                self.preview = None;
                self.pending_capture = connected;
                changed = true;
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        connected && self.selected_club.is_some(),
                        egui::Button::new("현재 정보 불러오기"),
                    )
                    .clicked()
                {
                    self.pending_capture = true;
                }
                if let Some(context) = &self.context {
                    ui.label(RichText::new(&context.game_date).color(MUTED).size(11.));
                }
            });
            ui.add_space(5.);
            label(ui, "이벤트 제목");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.draft.title).desired_width(f32::INFINITY))
                .changed();
            ui.add_space(3.);
            label(ui, "파트너 이름");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.draft.sponsor_name)
                        .desired_width(f32::INFINITY),
                )
                .changed();
            ui.label(
                RichText::new("기업명과 기사는 이 프로그램의 시나리오에 표시됩니다.")
                    .color(MUTED)
                    .size(11.),
            );
        });
        ui.add_space(12.);
        card(ui, |ui| {
            section(ui, "02", "장기 후원 계약");
            changed |= ui
                .checkbox(&mut self.enabled_sponsor, "기존 후원 계약을 교체")
                .changed();
            ui.add_enabled_ui(self.enabled_sponsor, |ui| {
                let selected = self
                    .context
                    .as_ref()
                    .and_then(|c| {
                        c.sponsors
                            .iter()
                            .find(|s| Some(s.slot) == self.draft.sponsor_slot)
                    })
                    .map(|s| format!("계약 {} · 연 £{}", s.slot + 1, money(s.annual_gbp as i64)))
                    .unwrap_or_else(|| "교체할 계약 선택".into());
                egui::ComboBox::from_id_salt("event-sponsor-slot")
                    .width(ui.available_width() - 8.)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        if let Some(context) = &self.context {
                            for sponsor in &context.sponsors {
                                changed |= ui
                                    .selectable_value(
                                        &mut self.draft.sponsor_slot,
                                        Some(sponsor.slot),
                                        format!(
                                            "계약 {} · 연 £{} · {} 종료",
                                            sponsor.slot + 1,
                                            money(sponsor.annual_gbp as i64),
                                            sponsor.end
                                        ),
                                    )
                                    .changed();
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        label(ui, "연간 후원금 · GBP");
                        changed |= amount(ui, &mut self.draft.annual_gbp, 100_000.);
                    });
                    ui.vertical(|ui| {
                        label(ui, "기간");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.draft.years)
                                    .range(1..=30)
                                    .suffix("년"),
                            )
                            .changed();
                    });
                });
                let total = self
                    .draft
                    .annual_gbp
                    .saturating_mul(self.draft.years as i64);
                ui.label(
                    RichText::new(format!("계약 총액  £{}", money(total)))
                        .color(LIME)
                        .size(13.)
                        .strong(),
                );
                ui.add_space(3.);
                changed |= ui
                    .checkbox(&mut self.enabled_fx, "원화 환산도 표시")
                    .changed();
                if self.enabled_fx {
                    ui.horizontal(|ui| {
                        ui.label("£1 =");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.exchange_rate)
                                    .range(0.01..=1_000_000.)
                                    .speed(1.)
                                    .suffix("원"),
                            )
                            .changed();
                    });
                    ui.label(
                        RichText::new("직접 입력한 표시용 환율입니다.")
                            .color(MUTED)
                            .size(11.),
                    );
                }
                ui.label(
                    RichText::new(
                        "총액을 현금으로 지급하지 않습니다. 계약 종류와 기존 플래그는 유지합니다.",
                    )
                    .color(MUTED)
                    .size(11.),
                );
            });
        });
        ui.add_space(12.);
        card(ui, |ui| {
            section(ui, "03", "추가 재정 지원");
            ui.label(
                RichText::new("필요한 항목만 선택하세요. 후원금과 별개입니다.")
                    .color(MUTED)
                    .size(11.),
            );
            changed |= grant_row(
                ui,
                "구단 잔고에 현금 지급",
                &mut self.enabled_cash,
                &mut self.cash_amount,
                100_000.,
                "£",
            );
            changed |= grant_row(
                ui,
                "이적 예산 증가",
                &mut self.enabled_transfer,
                &mut self.transfer_amount,
                100_000.,
                "£",
            );
            changed |= grant_row(
                ui,
                "주급 예산 증가",
                &mut self.enabled_wage,
                &mut self.wage_amount,
                1_000.,
                "£/주",
            );
        });
        ui.add_space(12.);
        card(ui, |ui| {
            section(ui, "04", "연고지 성장");
            let available = self.context.as_ref().is_some_and(|c| c.city.is_some());
            ui.add_enabled_ui(available, |ui| {
                changed |= ui
                    .checkbox(&mut self.enabled_city, "도시 개발 초기 단계 반영")
                    .changed();
            });
            if !available {
                ui.label(
                    RichText::new("연결된 홈 구장의 도시 정보를 먼저 불러오세요.")
                        .color(MUTED)
                        .size(11.),
                );
            }
            if self.enabled_city && available {
                ui.scope(|ui|{
                label(ui,"지역 사업 이름");changed|=ui.add(egui::TextEdit::singleline(&mut self.draft.city_project).desired_width(f32::INFINITY)).changed();
                ui.horizontal(|ui|{ui.vertical(|ui|{label(ui,"인구 구간");changed|=ui.add(egui::DragValue::new(&mut self.population).range(0..=15)).changed();});ui.vertical(|ui|{label(ui,"도시 매력도");changed|=ui.add(egui::DragValue::new(&mut self.attraction).range(0..=20).suffix(" / 20")).changed();});});
                ui.label(RichText::new("7: 5만–10만 명 · 8: 10만–25만 명\n현재 단계만 반영하며 이후 성장은 자동 진행되지 않습니다.").color(MUTED).size(11.));
            });
            }
        });
        ui.add_space(12.);
        changed
    }
    fn article(&mut self, ui: &mut egui::Ui, connected: bool) {
        self.actions(ui, connected);
        ui.add_space(6.);
        let width = ui.available_width();
        egui::Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1.0_f32, LINE))
            .corner_radius(4)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width((width - 20.).max(200.));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("시나리오 미리보기")
                            .size(12.)
                            .strong()
                            .color(LIME),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("가상 구단 소식").size(10.).color(MUTED));
                    });
                });
                ui.separator();
                let name = self
                    .context
                    .as_ref()
                    .map(|c| c.club.entity.name.as_str())
                    .unwrap_or("새로운 구단 이야기");
                ui.label(RichText::new(name).size(11.).color(LIME));
                ui.label(
                    RichText::new(&self.draft.title)
                        .size(19.)
                        .strong()
                        .color(INK),
                );
                ui.label(
                    RichText::new(
                        self.context
                            .as_ref()
                            .map(|c| c.game_date.as_str())
                            .unwrap_or("구단 정보를 불러오면 현재 날짜를 표시합니다"),
                    )
                    .size(11.)
                    .color(MUTED),
                );
                ui.add_space(6.);
                ui.columns(3, |columns| {
                    metric(
                        &mut columns[0],
                        "연간 파트너십",
                        if self.enabled_sponsor {
                            format!("£{}", compact(self.draft.annual_gbp))
                        } else {
                            "—".into()
                        },
                    );
                    metric(
                        &mut columns[1],
                        "계약 기간",
                        if self.enabled_sponsor {
                            format!("{}년", self.draft.years)
                        } else {
                            "—".into()
                        },
                    );
                    metric(
                        &mut columns[2],
                        "현금 추가 지급",
                        if self.enabled_cash {
                            format!("£{}", compact(self.cash_amount))
                        } else {
                            "없음".into()
                        },
                    );
                });
                ui.add_space(6.);
                if let Some(preview) = &self.preview {
                    // Narrative begins with the title/date already shown above.
                    let story = preview
                        .narrative
                        .splitn(5, '\n')
                        .nth(4)
                        .unwrap_or(&preview.narrative);
                    for paragraph in story.split("\n\n") {
                        ui.label(RichText::new(paragraph).size(12.).color(INK));
                        ui.add_space(4.);
                    }
                } else {
                    ui.label(
                        RichText::new("장기 후원, 재정 지원, 연고지 개발을 구성합니다.")
                            .size(13.)
                            .color(INK),
                    );
                    ui.label(
                        RichText::new("대상 구단을 불러오고 조건을 입력하세요.")
                            .size(12.)
                            .color(MUTED),
                    );
                }
            });
        ui.add_space(8.);
        if let Some(error) = &self.preview_error {
            ui.label(RichText::new(error).color(AMBER));
            ui.add_space(8.);
        }
        if let Some(preview) = &self.preview {
            card(ui, |ui| {
                section(ui, "REVIEW", "게임에 반영할 변화");
                delta_table(ui, &preview.deltas, false);
            });
            ui.add_space(12.);
            egui::CollapsingHeader::new("적용 범위와 확인 사항")
                .id_salt("event-notes")
                .show(ui, |ui| {
                    for note in &preview.warnings {
                        ui.label(RichText::new(format!("• {note}")).size(12.).color(MUTED));
                    }
                });
        }
    }
    fn actions(&mut self, ui: &mut egui::Ui, connected: bool) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.is_busy() && self.preview.is_some(),
                    egui::Button::new("시나리오 저장").min_size(Vec2::new(105., 27.)),
                )
                .clicked()
            {
                self.save(ui.ctx());
            }
            if ui
                .add_enabled(
                    connected && !self.is_busy() && self.preview.is_some(),
                    egui::Button::new(RichText::new("검토 후 게임에 적용").color(PANEL).strong())
                        .fill(LIME)
                        .min_size(Vec2::new(155., 27.)),
                )
                .clicked()
            {
                self.confirm = self.preview.clone().map(Confirmation::Apply);
            }
        });
        if !connected {
            ui.add_space(7.);
            ui.label(
                RichText::new(
                    "미리보기 모드에서는 시나리오만 저장합니다. 게임 적용은 FM 연결 후 가능합니다.",
                )
                .color(AMBER)
                .size(12.),
            );
        }
    }
    fn library(&mut self, ui: &mut egui::Ui, index: Option<&Index>, connected: bool) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("저장한 조건을 불러와 현재 구단에 맞춰 다시 검토하세요.")
                    .color(MUTED),
            );
            if ui.small_button("새로고침").clicked() {
                self.saved = events::drafts();
            }
        });
        ui.add_space(12.);
        let mut load = None;
        egui::ScrollArea::vertical()
            .id_salt("event-library")
            .show(ui, |ui| {
                if self.saved.is_empty() {
                    empty(
                        ui,
                        "아직 저장한 시나리오가 없습니다",
                        "이벤트 만들기에서 조건을 정한 뒤 ‘시나리오 저장’을 누르세요.",
                    );
                }
                for saved in &self.saved {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&saved.draft.title).size(18.).strong());
                                ui.label(
                                    RichText::new(format!(
                                        "{}  ·  {}",
                                        saved.context.club.entity.name, saved.context.game_date
                                    ))
                                    .color(MUTED)
                                    .size(12.),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add_enabled(!self.is_busy(), egui::Button::new("불러오기"))
                                    .clicked()
                                {
                                    load = Some(saved.clone());
                                }
                            });
                        });
                        ui.add_space(7.);
                        ui.label(
                            RichText::new(format!(
                                "{} · 연 £{} · {}년 · {}개 변경",
                                saved.draft.sponsor_name,
                                money(saved.draft.annual_gbp),
                                saved.draft.years,
                                saved.changes.len()
                            ))
                            .color(LIME)
                            .size(12.),
                        );
                    });
                    ui.add_space(10.);
                }
            });
        if let Some(saved) = load {
            self.load_form(&saved.draft);
            self.selected_club = Some(saved.context.club.entity.id);
            self.page = Page::Compose;
            if connected && index.is_some() {
                self.context = None;
                self.preview = None;
                self.pending_capture = true;
            } else {
                self.set_context(saved.context);
            }
            self.notice = Some((
                "시나리오 조건을 불러왔습니다. 적용하기 전에 현재 값과 비교해 주세요.".into(),
                false,
            ));
        }
    }
    fn history_page(&mut self, ui: &mut egui::Ui, connected: bool) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("이벤트 반영 내역과 원본 백업").color(MUTED));
            if ui.small_button("새로고침").clicked() {
                self.history = events::history();
            }
        });
        ui.add_space(12.);
        let mut undo = None;
        egui::ScrollArea::vertical()
            .id_salt("event-history")
            .show(ui, |ui| {
                if self.history.is_empty() {
                    empty(
                        ui,
                        "아직 반영한 이벤트가 없습니다",
                        "게임에 적용한 이벤트의 변경 내용과 되돌리기가 여기에 표시됩니다.",
                    );
                }
                for receipt in &self.history {
                    let applied = receipt.status.starts_with("applied");
                    let failed = receipt.status.starts_with("failed");
                    let warning = receipt.status.contains("warning");
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&receipt.plan.draft.title).size(18.).strong(),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{}  ·  게임 날짜 {}",
                                        receipt.plan.context.club.entity.name,
                                        receipt.plan.context.game_date
                                    ))
                                    .color(MUTED)
                                    .size(12.),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                pill(
                                    ui,
                                    if warning {
                                        "확인 필요"
                                    } else if applied {
                                        "반영 완료"
                                    } else if failed {
                                        "적용 실패"
                                    } else if receipt.status.starts_with("prepared") {
                                        "원본 백업됨"
                                    } else {
                                        "되돌림"
                                    },
                                    if failed || warning { AMBER } else { LIME },
                                );
                            });
                        });
                        ui.add_space(8.);
                        egui::CollapsingHeader::new(format!(
                            "변경 내용 {}개",
                            receipt.plan.changes.len()
                        ))
                        .id_salt(&receipt.id)
                        .show(ui, |ui| {
                            delta_table(ui, &receipt.plan.deltas, false);
                            if warning || failed {
                                ui.label(RichText::new(&receipt.status).color(AMBER));
                            }
                        });
                        if applied
                            && ui
                                .add_enabled(
                                    connected && !self.is_busy(),
                                    egui::Button::new("이 이벤트 되돌리기"),
                                )
                                .clicked()
                        {
                            undo = Some(receipt.clone());
                        }
                    });
                    ui.add_space(10.);
                }
            });
        if let Some(receipt) = undo {
            self.confirm = Some(Confirmation::Undo(receipt));
        }
    }
    fn confirmation(&mut self, ctx: &egui::Context, connected: bool) {
        let Some(confirm) = self.confirm.clone() else {
            return;
        };
        let (plan, is_undo) = match &confirm {
            Confirmation::Apply(p) => (p, false),
            Confirmation::Undo(r) => (&r.plan, true),
        };
        let mut open = true;
        let mut commit = false;
        let mut cancel = false;
        egui::Window::new(if is_undo {
            "이벤트 되돌리기 검토"
        } else {
            "이벤트 적용 전 검토"
        })
        .id(egui::Id::new("event-review-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(760.)
        .max_height(720.)
        .show(ctx, |ui| {
            ui.label(RichText::new(&plan.draft.title).size(22.).strong());
            ui.label(
                RichText::new(format!(
                    "{}  ·  {}",
                    plan.context.club.entity.name, plan.context.game_date
                ))
                .color(LIME),
            );
            ui.add_space(10.);
            egui::ScrollArea::vertical()
                .max_height(420.)
                .show(ui, |ui| {
                    delta_table(ui, &plan.deltas, is_undo);
                    ui.add_space(8.);
                    for warning in &plan.warnings {
                        ui.label(RichText::new(format!("• {warning}")).size(12.).color(MUTED));
                    }
                });
            ui.add_space(10.);
            ui.separator();
            ui.add_space(8.);
            ui.label(
                RichText::new(if is_undo {
                    "적용 이후 값이 바뀐 항목은 덮어쓰지 않고 중단합니다."
                } else {
                    "현재 게임 값을 다시 확인하고 원본을 백업한 뒤 표시된 항목만 함께 반영합니다."
                })
                .size(12.)
                .color(AMBER),
            );
            ui.horizontal(|ui| {
                cancel = ui.button("취소").clicked();
                commit = ui
                    .add_enabled(
                        connected && !self.is_busy(),
                        egui::Button::new(
                            RichText::new(if is_undo {
                                "확인 · 원래 값으로 되돌리기"
                            } else {
                                "확인 · 이 이벤트 적용"
                            })
                            .strong()
                            .color(PANEL),
                        )
                        .fill(LIME),
                    )
                    .clicked();
            });
        });
        if cancel || !open {
            self.confirm = None;
        }
        if commit && connected {
            self.commit(confirm, ctx, connected);
        }
    }
}

fn card<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let width = ui.available_width();
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(4)
        .inner_margin(9)
        .show(ui, |ui| {
            ui.set_width((width - 18.).max(150.));
            body(ui)
        })
        .inner
}
fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.).color(MUTED));
}
fn section(ui: &mut egui::Ui, n: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(n).color(LIME).size(11.).strong());
        ui.label(RichText::new(title).color(INK).size(13.).strong());
    });
    ui.add_space(3.);
}
fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.13))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.35)))
        .corner_radius(20)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.).color(color));
        });
}
fn metric(ui: &mut egui::Ui, title: &str, value: String) {
    label(ui, title);
    ui.label(RichText::new(value).size(17.).strong().color(LIME));
}
fn amount(ui: &mut egui::Ui, value: &mut i64, speed: f64) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .range(0..=2_000_000_000i64)
            .speed(speed)
            .custom_formatter(|v, _| money(v as i64))
            .custom_parser(|s| s.replace(',', "").replace('£', "").trim().parse().ok()),
    )
    .changed()
}
fn grant_row(
    ui: &mut egui::Ui,
    name: &str,
    enabled: &mut bool,
    value: &mut i64,
    speed: f64,
    unit: &str,
) -> bool {
    let mut changed = ui.checkbox(enabled, name).changed();
    if *enabled {
        ui.horizontal(|ui| {
            changed |= amount(ui, value, speed);
            ui.label(RichText::new(unit).color(MUTED).size(12.));
        });
    }
    changed
}
fn money(value: i64) -> String {
    let s = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("−{out}") } else { out }
}
fn compact(value: i64) -> String {
    if value.abs() >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.)
    } else if value.abs() >= 1_000 {
        format!("{:.0}K", value as f64 / 1_000.)
    } else {
        money(value)
    }
}
fn empty(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    card(ui, |ui| {
        ui.add_space(26.);
        ui.label(RichText::new(title).size(22.).strong());
        ui.add_space(8.);
        ui.label(RichText::new(subtitle).color(MUTED));
        ui.add_space(26.);
    });
}
fn delta_table(ui: &mut egui::Ui, deltas: &[events::EventDelta], reverse: bool) {
    egui::Grid::new(ui.id().with("event-deltas"))
        .num_columns(3)
        .spacing(Vec2::new(12., 10.))
        .striped(true)
        .show(ui, |ui| {
            label(ui, "변경 항목");
            label(ui, "현재");
            label(ui, "변경 후");
            ui.end_row();
            for delta in deltas {
                ui.label(RichText::new(&delta.label).size(12.));
                ui.label(
                    RichText::new(if reverse { &delta.after } else { &delta.before })
                        .size(12.)
                        .color(MUTED),
                );
                ui.label(
                    RichText::new(if reverse { &delta.before } else { &delta.after })
                        .size(12.)
                        .color(LIME)
                        .strong(),
                );
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_plan() -> EventPlan {
        use crate::model::{Detail, Entity, Session};
        let entity = Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id: 1141,
            kind: EntityKind::Club,
            name: "레체".into(),
            english_name: "Lecce".into(),
            club: "레체".into(),
            club_id: Some(1141),
            age: None,
            ca: None,
            pa: None,
            role: String::new(),
            group: String::new(),
            address: 0x10000,
            base: 0x10000,
            index: Some(1),
        };
        EventPlan {
            id: "1-2-3".into(),
            draft: EventDraft::default(),
            context: EventContext {
                session: Session {
                    pid: 0,
                    created: 0,
                    module_base: 0,
                    module_size: 0,
                    module_stamp: 0,
                    db: 0,
                    registry_begin: 0,
                    game_date: "2027-11-14".into(),
                    profile: "unit-test".into(),
                    own_club_id: Some(1141),
                },
                club: Detail {
                    entity,
                    fields: vec![],
                    guards: vec![],
                    notes: vec![],
                },
                city: None,
                sponsors: vec![],
                game_date: "2027-11-14".into(),
                warnings: vec![],
                guards: vec![],
            },
            deltas: vec![],
            narrative: String::new(),
            warnings: vec![],
            changes: vec![],
            guards: vec![],
        }
    }
    #[test]
    fn money_formatting() {
        assert_eq!(money(26_653_522), "26,653,522");
        assert_eq!(money(-500), "−500");
        assert_eq!(compact(26_653_522), "26.7M");
    }
    #[test]
    fn club_navigation_switches_target_without_reverting_dropdown_each_frame() {
        let mut studio = EventStudio::new();
        studio.enter_club(Some(1141), false);
        assert_eq!(studio.selected_club, Some(1141));
        studio.enter_club(Some(999), false);
        assert_eq!(studio.selected_club, Some(999));
        assert!(studio.context.is_none() && studio.preview.is_none() && studio.confirm.is_none());
        // A dropdown choice persists while the parent route remains unchanged.
        studio.selected_club = Some(888);
        assert_eq!(studio.route_club_seen, Some(999));
        // Re-entering that same club profile route still deliberately selects it.
        studio.enter_club(Some(999), false);
        assert_eq!(studio.selected_club, Some(999));
        assert!(!studio.pending_capture);
    }
    #[test]
    fn loading_draft_preserves_small_explicit_grants() {
        let mut studio = EventStudio::new();
        studio.load_form(&EventDraft {
            cash_grant_gbp: 20_000,
            transfer_budget_grant_gbp: 30_000,
            weekly_wage_increase_gbp: 500,
            population_band: Some(7),
            attraction: Some(12),
            ..Default::default()
        });
        let effective = studio.effective_draft();
        assert_eq!(effective.cash_grant_gbp, 20_000);
        assert_eq!(effective.transfer_budget_grant_gbp, 30_000);
        assert_eq!(effective.weekly_wage_increase_gbp, 500);
        assert_eq!(effective.population_band, Some(7));
    }
    #[test]
    fn changing_context_or_form_invalidates_review_and_offline_never_commits() {
        let plan = fixture_plan();
        let mut studio = EventStudio::new();
        studio.confirm = Some(Confirmation::Apply(plan.clone()));
        studio.commit(
            Confirmation::Apply(plan.clone()),
            &egui::Context::default(),
            false,
        );
        assert!(!studio.is_busy());
        assert!(studio.confirm.is_some());
        studio.set_context(plan.context.clone());
        assert!(studio.confirm.is_none());
        studio.confirm = Some(Confirmation::Apply(plan.clone()));
        studio.load_form(&plan.draft);
        assert!(studio.confirm.is_none());
        studio.confirm = Some(Confirmation::Apply(plan.clone()));
        studio.rebuild();
        assert!(studio.confirm.is_none());
        studio.confirm = Some(Confirmation::Apply(plan));
        studio.enter_club(Some(555), false);
        assert!(studio.confirm.is_none());
    }
}
