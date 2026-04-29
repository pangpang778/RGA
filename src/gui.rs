use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use eframe::egui;

use crate::agent_loop::agent_runner_loop;
use crate::assets::RuntimePaths;
use crate::config::{LlmConfig, ProviderKind};
use crate::llm::AnyLlmClient;
use crate::tools::ToolDispatcher;

// ── Color palette ──────────────────────────────────────────────────────

const BG: egui::Color32 = egui::Color32::from_rgb(250, 249, 247);
const BG_WHITE: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);
const BG_CARD: egui::Color32 = egui::Color32::from_rgb(245, 243, 240);
const TEXT: egui::Color32 = egui::Color32::from_rgb(38, 38, 38);
const TEXT2: egui::Color32 = egui::Color32::from_rgb(115, 115, 115);
const TEXT3: egui::Color32 = egui::Color32::from_rgb(168, 162, 158);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(99, 102, 241);
const ACCENT_BG: egui::Color32 = egui::Color32::from_rgb(238, 242, 255);
const USER_BG: egui::Color32 = egui::Color32::from_rgb(99, 102, 241);
const ASST_BG: egui::Color32 = egui::Color32::from_rgb(245, 243, 240);
const STREAM_BG: egui::Color32 = egui::Color32::from_rgb(238, 242, 255);
const BORDER: egui::Color32 = egui::Color32::from_rgb(232, 229, 224);
const GREEN: egui::Color32 = egui::Color32::from_rgb(34, 197, 94);

// ── Channel types ──────────────────────────────────────────────────────

enum AgentMsg {
    /// Partial text from SSE stream
    Token(String),
    /// Agent finished
    Done,
    /// Agent error
    Error(String),
}

struct AgentRequest {
    prompt: String,
    provider: ProviderKind,
    api_key: String,
    base_url: String,
    model: String,
    paths: RuntimePaths,
}

// ── Chat message ───────────────────────────────────────────────────────

#[derive(Clone)]
struct ChatMsg {
    role: String,
    content: String,
}

// ── Application state ──────────────────────────────────────────────────

pub struct ChatApp {
    messages: Vec<ChatMsg>,
    input: String,
    // Config
    provider: ProviderKind,
    api_key: String,
    base_url: String,
    model: String,
    show_settings: bool,
    // Agent
    is_running: bool,
    tx: mpsc::Sender<AgentRequest>,
    rx: mpsc::Receiver<AgentMsg>,
    scroll_to_bottom: bool,
    focus_input: bool,
    // Streaming: accumulated text being received
    streaming_text: String,
    // Paths
    paths: RuntimePaths,
}

impl ChatApp {
    pub fn new(paths: RuntimePaths) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<AgentRequest>();
        // Channel for streaming tokens from LLM to GUI
        let (stream_tx, stream_rx) = mpsc::channel::<String>();
        // Channel for agent lifecycle messages
        let (msg_tx, msg_rx) = mpsc::channel::<AgentMsg>();

        // Clone senders before moving into threads
        let msg_tx_agent = msg_tx.clone();
        let stream_tx_agent = stream_tx.clone();

        // Agent worker thread
        thread::spawn(move || {
            while let Ok(req) = req_rx.recv() {
                let result = run_agent(&req, stream_tx_agent.clone());
                match result {
                    Ok(_) => {
                        let _ = msg_tx_agent.send(AgentMsg::Done);
                    }
                    Err(e) => {
                        let _ = msg_tx_agent.send(AgentMsg::Error(format!("[\u{9519}\u{8bef}] {e}")));
                    }
                }
            }
        });

        // Token forwarding thread: reads stream_rx and sends as AgentMsg::Token
        // This bridges the stream channel (from LLM) to the msg channel (to GUI)
        thread::spawn(move || {
            while let Ok(token) = stream_rx.recv() {
                if msg_tx.send(AgentMsg::Token(token)).is_err() {
                    break;
                }
            }
        });

        let default_cfg = LlmConfig::from_env(None);
        let (provider, api_key, base_url, model) = match default_cfg.provider {
            ProviderKind::OpenAi => (
                ProviderKind::OpenAi,
                default_cfg.api_key.unwrap_or_default(),
                default_cfg.api_base,
                default_cfg.model,
            ),
            ProviderKind::Anthropic => (
                ProviderKind::Anthropic,
                default_cfg.api_key.unwrap_or_default(),
                default_cfg.api_base,
                default_cfg.model,
            ),
            _ => (
                ProviderKind::Mock,
                String::new(),
                "https://api.openai.com/v1".to_string(),
                "gpt-4o-mini".to_string(),
            ),
        };

        Self {
            messages: Vec::new(),
            input: String::new(),
            provider,
            api_key,
            base_url,
            model,
            show_settings: false,
            is_running: false,
            tx: req_tx,
            rx: msg_rx,
            scroll_to_bottom: false,
            focus_input: true,
            streaming_text: String::new(),
            paths,
        }
    }

    fn send_prompt(&mut self) {
        let prompt = self.input.trim().to_string();
        if prompt.is_empty() || self.is_running {
            return;
        }
        self.messages.push(ChatMsg {
            role: "user".into(),
            content: prompt.clone(),
        });
        self.input.clear();
        self.is_running = true;
        self.scroll_to_bottom = true;
        self.focus_input = true;
        self.streaming_text.clear();

        let _ = self.tx.send(AgentRequest {
            prompt,
            provider: self.provider.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            paths: self.paths.clone(),
        });
    }

    /// Poll for agent messages. Returns true if a message was fully received.
    fn poll_agent(&mut self) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(AgentMsg::Token(chunk)) => {
                    self.streaming_text.push_str(&chunk);
                    self.scroll_to_bottom = true;
                }
                Ok(AgentMsg::Done) => {
                    // Move streamed text into messages
                    if !self.streaming_text.is_empty() {
                        self.messages.push(ChatMsg {
                            role: "assistant".into(),
                            content: self.streaming_text.clone(),
                        });
                    }
                    self.streaming_text.clear();
                    self.is_running = false;
                    self.scroll_to_bottom = true;
                    self.focus_input = true;
                    return true;
                }
                Ok(AgentMsg::Error(msg)) => {
                    self.messages.push(ChatMsg {
                        role: "assistant".into(),
                        content: msg,
                    });
                    self.streaming_text.clear();
                    self.is_running = false;
                    self.scroll_to_bottom = true;
                    self.focus_input = true;
                    return true;
                }
                Err(_) => return false,
            }
        }
    }
}

// ── Agent runner ───────────────────────────────────────────────────────

fn run_agent(
    req: &AgentRequest,
    stream_tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    let cfg = match req.provider {
        ProviderKind::OpenAi => LlmConfig::openai_with(
            non_empty_str(&req.api_key),
            req.base_url.clone(),
            req.model.clone(),
        ),
        ProviderKind::Anthropic => LlmConfig::anthropic_with(
            non_empty_str(&req.api_key),
            req.base_url.clone(),
            req.model.clone(),
        ),
        ProviderKind::Mock => LlmConfig::mock(),
    };

    let mut client = AnyLlmClient::from_config(cfg)?;
    let mut handler = ToolDispatcher::new(req.paths.clone(), req.paths.temp.clone());
    let tools_schema = req.paths.load_tools_schema().unwrap_or_default();
    let system_prompt = req
        .paths
        .system_prompt()
        .unwrap_or_else(|_| "You are a helpful assistant.".into());

    agent_runner_loop(
        &mut client,
        system_prompt,
        req.prompt.clone(),
        &mut handler,
        tools_schema,
        70,
        false,
        Some(stream_tx),
    )?;
    Ok(())
}

fn non_empty_str(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

// ── eframe::App ────────────────────────────────────────────────────────

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_agent();

        if self.is_running {
            ctx.request_repaint();
        }

        // ── Style ──────────────────────────────────────────────────
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.visuals.window_fill = BG;
        style.visuals.panel_fill = BG;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.widgets.noninteractive.bg_fill = BG_CARD;
        style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
        style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, BORDER);
        style.visuals.widgets.inactive.bg_fill = BG_CARD;
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, BORDER);
        style.visuals.widgets.hovered.bg_fill = ACCENT_BG;
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ACCENT);
        style.visuals.widgets.active.bg_fill = ACCENT_BG;
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, ACCENT);
        style.visuals.selection.bg_fill = ACCENT_BG;
        style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
        ctx.set_style(style);

        // ── Settings popup ─────────────────────────────────────────
        if self.show_settings {
            egui::Window::new("\u{2699}\u{fe0f} \u{8bbe}\u{7f6e}")
                .collapsible(false)
                .resizable(true)
                .default_width(360.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.add_space(8.0);

                    label_bold(ui, "Provider");
                    ui.add_space(2.0);
                    egui::ComboBox::from_id_salt("provider_select")
                        .selected_text(provider_label(&self.provider))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.provider,
                                ProviderKind::Mock,
                                "Mock (\u{65e0}\u{9700} Key)",
                            );
                            ui.selectable_value(
                                &mut self.provider,
                                ProviderKind::OpenAi,
                                "OpenAI-compatible",
                            );
                            ui.selectable_value(
                                &mut self.provider,
                                ProviderKind::Anthropic,
                                "Anthropic",
                            );
                        });

                    ui.add_space(10.0);
                    label_bold(ui, "API Key");
                    ui.add_space(2.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.api_key)
                            .password(true)
                            .hint_text("sk-...")
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(12, 8)),
                    );

                    ui.add_space(10.0);
                    label_bold(ui, "Base URL");
                    ui.add_space(2.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.base_url)
                            .hint_text("https://api.openai.com/v1")
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(12, 8)),
                    );

                    ui.add_space(10.0);
                    label_bold(ui, "Model");
                    ui.add_space(2.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.model)
                            .hint_text("gpt-4o-mini")
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(12, 8)),
                    );

                    ui.add_space(16.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(
                                    "\u{2714}\u{fe0f} \u{4fdd}\u{5b58}",
                                )
                                .size(13.0)
                                .color(egui::Color32::WHITE),
                            )
                            .fill(ACCENT)
                            .corner_radius(egui::CornerRadius::same(10))
                            .min_size(egui::vec2(ui.available_width(), 36.0)),
                        )
                        .clicked()
                    {
                        self.show_settings = false;
                    }
                });
        }

        // ── Full-screen layout ─────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            // ── Top bar ────────────────────────────────────────────
            egui::Frame::new()
                .fill(BG_WHITE)
                .stroke(egui::Stroke::new(1.0, BORDER))
                .inner_margin(egui::Margin::symmetric(20, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("\u{1f4ac} RGA Cowork")
                                .size(16.0)
                                .strong()
                                .color(TEXT),
                        );
                        ui.add_space(4.0);

                        let dot = if self.is_running {
                            ACCENT
                        } else {
                            GREEN
                        };
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, dot);

                        ui.label(
                            egui::RichText::new(format!(
                                "{} \u{00b7} {}",
                                provider_label(&self.provider),
                                if self.model.is_empty() {
                                    "default"
                                } else {
                                    &self.model
                                }
                            ))
                            .size(11.0)
                            .color(TEXT3),
                        );

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("\u{2699}\u{fe0f}")
                                                .size(16.0),
                                        )
                                        .fill(BG_CARD)
                                        .corner_radius(egui::CornerRadius::same(8))
                                        .min_size(egui::vec2(36.0, 30.0)),
                                    )
                                    .clicked()
                                {
                                    self.show_settings = !self.show_settings;
                                }

                                ui.add_space(4.0);

                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(
                                                "\u{2728} \u{65b0}\u{5bf9}\u{8bdd}",
                                            )
                                            .size(12.0)
                                            .color(egui::Color32::WHITE),
                                        )
                                        .fill(ACCENT)
                                        .corner_radius(egui::CornerRadius::same(8))
                                        .min_size(egui::vec2(90.0, 30.0)),
                                    )
                                    .clicked()
                                {
                                    self.messages.clear();
                                    self.streaming_text.clear();
                                }
                            },
                        );
                    });
                });

            // ── Remaining space = chat + composer ──────────────────
            let remaining = ui.available_size();
            let composer_h = 60.0;
            let chat_h = (remaining.y - composer_h).max(100.0);

            // ── Chat area ──────────────────────────────────────────
            ui.allocate_ui_with_layout(
                egui::vec2(remaining.x, chat_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::Frame::new().fill(BG).show(ui, |ui| {
                        ui.set_min_height(chat_h);

                        if self.messages.is_empty()
                            && self.streaming_text.is_empty()
                            && !self.is_running
                        {
                            render_empty_state(ui, chat_h);
                        } else {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    ui.add_space(12.0);
                                    for msg in &self.messages {
                                        render_message(ui, msg);
                                        ui.add_space(8.0);
                                    }

                                    // Live streaming text
                                    if self.is_running && !self.streaming_text.is_empty() {
                                        render_streaming(ui, &self.streaming_text);
                                        ui.add_space(8.0);
                                    }

                                    // Waiting spinner (before any tokens)
                                    if self.is_running && self.streaming_text.is_empty() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(24.0);
                                            ui.spinner();
                                            ui.add_space(6.0);
                                            ui.label(
                                                egui::RichText::new(
                                                    "\u{6b63}\u{5728}\u{601d}\u{8003}\u{2026}",
                                                )
                                                .size(13.0)
                                                .color(ACCENT),
                                            );
                                        });
                                    }
                                    ui.add_space(8.0);
                                });
                        }
                    });
                },
            );

            // ── Composer (always visible) ──────────────────────────
            egui::Frame::new()
                .fill(BG_WHITE)
                .stroke(egui::Stroke::new(1.0, BORDER))
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let input_id = egui::Id::new("chat_input");
                        if self.focus_input {
                            ctx.memory_mut(|m| m.request_focus(input_id));
                            self.focus_input = false;
                        }

                        let can_send = !self.input.trim().is_empty() && !self.is_running;

                        ui.add(
                            egui::TextEdit::singleline(&mut self.input)
                                .id(input_id)
                                .hint_text(
                                    "\u{8f93}\u{5165}\u{4efb}\u{52a1}\u{2026} (Enter \u{53d1}\u{9001})",
                                )
                                .desired_width(ui.available_width() - 72.0)
                                .font(egui::TextStyle::Body)
                                .text_color(TEXT)
                                .margin(egui::Margin::symmetric(14, 8)),
                        );

                        let enter_pressed =
                            ctx.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter_pressed && can_send {
                            self.send_prompt();
                        }

                        ui.add_space(6.0);
                        let btn_color = if can_send { ACCENT } else { TEXT3 };
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("\u{53d1}\u{9001}")
                                        .size(13.0)
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(btn_color)
                                .corner_radius(egui::CornerRadius::same(10))
                                .min_size(egui::vec2(56.0, 34.0)),
                            )
                            .clicked()
                            && can_send
                        {
                            self.send_prompt();
                        }
                    });
                });
        });
    }
}

// ── Render helpers ─────────────────────────────────────────────────────

fn render_empty_state(ui: &mut egui::Ui, chat_h: f32) {
    ui.vertical_centered(|ui| {
        ui.add_space(chat_h * 0.22);
        ui.label(egui::RichText::new("\u{1f331}").size(48.0));
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("\u{6b22}\u{8fce}\u{4f7f}\u{7528} RGA Cowork")
                .size(20.0)
                .strong()
                .color(TEXT),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "\u{70b9}\u{51fb}\u{53f3}\u{4e0a}\u{89d2} \u{2699}\u{fe0f} \u{914d}\u{7f6e} Provider\u{ff0c}\u{7136}\u{540e}\u{8f93}\u{5165}\u{4efb}\u{52a1}\u{5f00}\u{59cb}\u{5bf9}\u{8bdd}",
            )
            .size(13.0)
            .color(TEXT2),
        );
        ui.add_space(20.0);
        let tips = [
            ("\u{1f4a1}", "\u{8f93}\u{5165}\u{4efb}\u{52a1}\u{63cf}\u{8ff0}\u{ff0c}AI \u{4f1a}\u{81ea}\u{52a8}\u{8c03}\u{7528}\u{5de5}\u{5177}\u{5b8c}\u{6210}"),
            ("\u{26a1}", "\u{652f}\u{6301} OpenAI / Anthropic / Mock \u{4e09}\u{79cd}\u{63d0}\u{4f9b}\u{8005}"),
            ("\u{1f504}", "\u{53ef}\u{968f}\u{65f6}\u{5207}\u{6362}\u{6a21}\u{578b}\u{548c}\u{63d0}\u{4f9b}\u{8005}"),
        ];
        for (icon, tip) in &tips {
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() * 0.28);
                ui.label(egui::RichText::new(*icon).size(14.0));
                ui.label(egui::RichText::new(*tip).size(12.0).color(TEXT3));
            });
            ui.add_space(2.0);
        }
    });
}

fn render_message(ui: &mut egui::Ui, msg: &ChatMsg) {
    let is_user = msg.role == "user";
    let max_w = ui.available_width() * 0.78;

    let fill = if is_user { USER_BG } else { ASST_BG };
    let text_c = if is_user {
        egui::Color32::WHITE
    } else {
        TEXT
    };
    let border_c = if is_user {
        egui::Color32::from_rgba_premultiplied(99, 102, 241, 40)
    } else {
        BORDER
    };
    let role_c = if is_user {
        egui::Color32::from_rgba_premultiplied(255, 255, 255, 180)
    } else {
        ACCENT
    };

    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, border_c))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .outer_margin(egui::Margin::symmetric(24, 0))
        .show(ui, |ui| {
            ui.set_max_width(max_w);
            ui.label(
                egui::RichText::new(if is_user {
                    "\u{4f60}"
                } else {
                    "\u{52a9}\u{624b}"
                })
                .size(11.0)
                .strong()
                .color(role_c),
            );
            ui.add_space(3.0);
            ui.label(egui::RichText::new(&msg.content).size(13.5).color(text_c));
        });
}

/// Render a streaming bubble: light blue background, spinner, live text.
fn render_streaming(ui: &mut egui::Ui, text: &str) {
    let max_w = ui.available_width() * 0.78;
    egui::Frame::new()
        .fill(STREAM_BG)
        .stroke(egui::Stroke::new(
            1.0,
            egui::Color32::from_rgba_premultiplied(99, 102, 241, 60),
        ))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .outer_margin(egui::Margin::symmetric(24, 0))
        .show(ui, |ui| {
            ui.set_max_width(max_w);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "\u{52a9}\u{624b} \u{6b63}\u{5728}\u{8f93}\u{51fa}\u{2026}",
                    )
                    .size(11.0)
                    .strong()
                    .color(ACCENT),
                );
            });
            ui.add_space(3.0);
            ui.label(egui::RichText::new(text).size(13.5).color(TEXT));
        });
}

fn label_bold(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .strong()
            .color(TEXT2),
    );
}

fn provider_label(p: &ProviderKind) -> &'static str {
    match p {
        ProviderKind::Mock => "Mock",
        ProviderKind::OpenAi => "OpenAI",
        ProviderKind::Anthropic => "Anthropic",
    }
}

// ── Entry point ────────────────────────────────────────────────────────

pub fn run_gui(paths: RuntimePaths) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 700.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("RGA Cowork"),
        ..Default::default()
    };

    let app = ChatApp::new(paths);
    eframe::run_native(
        "RGA Cowork",
        options,
        Box::new(move |cc| {
            setup_cjk_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("egui error: {e}"))
}

fn setup_cjk_fonts(ctx: &egui::Context) {
    use egui::FontFamily;
    let mut fonts = egui::FontDefinitions::default();
    let paths = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    let mut loaded = false;
    for path in &paths {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(data)),
            );
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "cjk".to_owned());
            loaded = true;
            break;
        }
    }
    if !loaded {
        eprintln!("Warning: No CJK font found.");
    }
    ctx.set_fonts(fonts);
}
