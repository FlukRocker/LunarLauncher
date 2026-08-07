// No console window: this is a GUI binary, and a flashing black window before
// the installer paints is the first thing a user sees.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Cyber Launcher's installer.
//!
//! Replaces NSIS. The reason is not that NSIS works badly — it is that NSIS
//! draws native Win32 controls, so the Cyber Network design could only ever be
//! approximated in it, and a one-click NSIS install has no page left to draw
//! anything on. Here the window *is* the design.
//!
//! The model is Discord's: one window, no wizard, no questions, no elevation.
//! It installs to %LOCALAPPDATA%, makes its shortcuts, registers for
//! Add/Remove Programs, and starts the app.
//!
//! Updates are **not** handled here. The launcher updates itself through the
//! Tauri updater, which is signed and already working; this binary exists only
//! for the first install and for uninstalling.

mod install;

use iced::widget::{button, column, image, mouse_area, container, progress_bar, row, text, Space};
use iced::{gradient, window, Alignment, Background, Border, Color, Element, Length, Radians, Subscription, Task, Theme};

// --- Cyber Network palette --------------------------------------------------
// Same tokens as the launcher's cyber.css, so the installer and the thing it
// installs look like one product.
const BG: Color = Color::from_rgb(0.020, 0.027, 0.039); // #05070a
const INK: Color = Color::from_rgb(0.914, 0.957, 0.957); // #e9f4f4
const DIM: Color = Color::from_rgb(0.576, 0.659, 0.678); // #93a8ad
const MUTE: Color = Color::from_rgb(0.357, 0.427, 0.459); // #5b6d75
const EMERALD: Color = Color::from_rgb(0.208, 0.851, 0.478); // #35d97a
const GOLD: Color = Color::from_rgb(1.000, 0.710, 0.180); // #ffb52e
const DIAMOND: Color = Color::from_rgb(0.247, 0.847, 0.941); // #3fd8f0
const REDSTONE: Color = Color::from_rgb(1.000, 0.239, 0.310); // #ff3d4f

/// `--cnm-grad`: diamond at 0%, emerald at 48%, gold at 100%.
///
/// The design's signature element. Reproduced as a real gradient rather than
/// approximated with a flat colour, which is the whole reason this window is
/// iced and not a Win32 dialog — NSIS could only ever have painted a picture
/// of it.
fn brand_gradient() -> Background {
    Background::Gradient(
        gradient::Linear::new(Radians(std::f32::consts::FRAC_PI_2))
            .add_stop(0.0, DIAMOND)
            .add_stop(0.48, EMERALD)
            .add_stop(1.0, GOLD)
            .into(),
    )
}

/// The brand mark, compiled in. See `mark` in `view`.
const LOGO: &[u8] = include_bytes!("../assets/logo.png");

/// `--cnm-line`: the hairline every panel in the design is bounded by.
const LINE: Color = Color::from_rgba(0.431, 0.784, 0.706, 0.16);

fn hairline() -> Border {
    Border { color: LINE, width: 1.0, radius: 0.0.into() }
}

/// A 1px horizontal rule.
///
/// Drawn as its own element rather than as a border, because iced borders
/// apply to all four edges — a border here would box each section instead of
/// separating them, and setting the width to zero to avoid that left the
/// design's hairlines missing entirely.
fn rule() -> Element<'static, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(LINE)),
            ..Default::default()
        })
        .into()
}

fn kv(k: &'static str, v: String) -> Element<'static, Message> {
    row![
        text(k).size(10).color(MUTE).width(120),
        text(v).size(12).color(INK),
    ]
    .align_y(Alignment::Start)
    .into()
}

fn step_chip(num: &'static str, name: &'static str, now: usize, index: usize) -> Element<'static, Message> {
    let current = now == index;
    let chip = container(
        text(num)
            .size(10)
            .color(if current { BG } else { MUTE }),
    )
    .width(24)
    .height(24)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(if current {
            brand_gradient()
        } else {
            Background::Color(Color::from_rgba(0.016, 0.027, 0.039, 0.6))
        }),
        border: if current { Border::default() } else { hairline() },
        ..Default::default()
    });

    row![
        chip,
        text(name).size(11).color(if current { INK } else { MUTE }),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn primary(label: &'static str, on_press: Message) -> button::Button<'static, Message> {
    button(text(label).size(12).color(BG))
        .on_press(on_press)
        .padding([10, 24])
        .style(|_, _| button::Style {
            background: Some(brand_gradient()),
            text_color: BG,
            border: Border::default(),
            ..Default::default()
        })
}

#[derive(Debug, Clone)]
enum Message {
    Install,
    /// Close the running launcher, then install over it.
    CloseAndInstall,
    /// The window has no system title bar, so dragging is ours to implement.
    Drag,
    Close,
    Progress(install::Progress),
    Launch,
}

enum Stage {
    /// The launcher is running, so its files cannot be replaced. Offered as a
    /// choice rather than reported as a dead end: the fix is one click and the
    /// installer can do it.
    Blocked,
    /// Shown first. One click, but a click — the user is told where this is
    /// going before anything is written, which a silent install never does.
    Confirm,
    Working { fraction: f32, detail: String },
    Done(std::path::PathBuf),
    Failed(String),
}

struct Installer {
    stage: Stage,
}

impl Default for Installer {
    fn default() -> Self {
        Self { stage: Stage::Confirm }
    }
}

impl Installer {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // `latest()` yields Option<Id>; `and_then` skips the drag when
            // there is no window, which is the case while one is closing.
            Message::Drag => window::latest().and_then(window::drag),
            Message::Close => iced::exit(),
            Message::CloseAndInstall => {
                install::close_running();
                self.stage = Stage::Working {
                    fraction: 0.0,
                    detail: String::from("Preparing"),
                };
                Task::none()
            }
            Message::Install => {
                self.stage = Stage::Working {
                    fraction: 0.0,
                    detail: String::from("Preparing"),
                };
                Task::none()
            }
            Message::Progress(install::Progress::Step(fraction, detail)) => {
                self.stage = Stage::Working { fraction, detail };
                Task::none()
            }
            Message::Progress(install::Progress::Done(exe)) => {
                self.stage = Stage::Done(exe);
                // Nothing left to decide, so the window closes itself and the
                // launcher starts. A finish page with a single button is the
                // wizard habit this design exists to avoid.
                Task::done(Message::Launch)
            }
            Message::Progress(install::Progress::Failed(err)) => {
                // The one failure with a remedy the installer can perform gets
                // its own screen and a button, rather than a message telling
                // the user to go and do it themselves.
                self.stage = if err.contains("already running") {
                    Stage::Blocked
                } else {
                    Stage::Failed(err)
                };
                Task::none()
            }
            Message::Launch => {
                if let Stage::Done(exe) = &self.stage {
                    let _ = std::process::Command::new(exe).spawn();
                }
                iced::exit()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        // Laid out to the artifact's own measurements: an 820px frame, a 56px
        // header, a 52px step rail, a 326px body and a footer — rather than
        // the palette alone on an arbitrary window.
        let mark = image(image::Handle::from_bytes(LOGO)).width(28).height(28);

        let brand = column![
            text("CYBER NETWORK").size(15).color(INK),
            text("CYBER LAUNCHER SETUP").size(9).color(MUTE),
        ]
        .spacing(3);

        let pill = container(
            row![
                container(Space::new().width(6).height(6)).style(|_| container::Style {
                    background: Some(Background::Color(EMERALD)),
                    ..Default::default()
                }),
                text(if matches!(self.stage, Stage::Done(_)) { "INSTALLED" } else { "READY" })
                    .size(10)
                    .color(DIM),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(|_| container::Style {
            border: hairline(),
            ..Default::default()
        });

        let close = button(text("\u{2715}").size(13).color(MUTE))
            .on_press(Message::Close)
            .padding([4, 10])
            .style(|_, _| button::Style {
                background: None,
                text_color: MUTE,
                border: Border::default(),
                ..Default::default()
            });

        let header = mouse_area(
            container(
                row![mark, brand, Space::new().width(Length::Fill), pill, close]
                    .spacing(12)
                    .align_y(Alignment::Center),
            )
            .height(56)
            .padding([0, 24]),
        )
        .on_press(Message::Drag);

        // Two steps, in the rail the design uses for four. An installer that
        // showed no progression at all would drop the most recognisable
        // element of the chrome.
        let step_now = if matches!(self.stage, Stage::Confirm | Stage::Blocked) { 0 } else { 1 };
        let rail = container(
            row![
                step_chip("01", "CONFIRM", step_now, 0),
                text("\u{203A}").size(14).color(MUTE),
                step_chip("02", "INSTALL", step_now, 1),
                Space::new().width(Length::Fill),
                text(format!("STEP {} OF 2", step_now + 1)).size(10).color(MUTE),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .height(52)
        .padding([0, 24])
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.016, 0.027, 0.039, 0.45))),
            ..Default::default()
        });

        let (eyebrow, title, title_colour) = match &self.stage {
            Stage::Confirm => ("// 01_CONFIRM_TARGET", "READY TO INSTALL", INK),
            Stage::Blocked => ("// 01_CONFIRM_TARGET", "CLOSE CYBER LAUNCHER", GOLD),
            Stage::Working { .. } => ("// 02_WRITE_FILES", "INSTALLING", INK),
            Stage::Done(_) => ("// 02_WRITE_FILES", "INSTALLED", INK),
            Stage::Failed(_) => ("// 02_WRITE_FILES", "INSTALL FAILED", REDSTONE),
        };

        let inner: Element<'_, Message> = match &self.stage {
            Stage::Confirm => column![
                kv("DESTINATION", install::install_dir().to_string_lossy().to_string()),
                kv("VERSION", install::VERSION.to_string()),
                kv("SIZE", format!("{:.0} MB", install::PAYLOAD.len() as f64 / 1_048_576.0)),
            ]
            .spacing(10)
            .into(),

            Stage::Blocked => text(
                "It is running, and its files cannot be replaced while it is open. \
                 The installer can close it for you.",
            )
            .size(12)
            .color(DIM)
            .into(),

            Stage::Working { fraction, detail } => column![
                progress_bar(0.0..=1.0, *fraction).style(|_| progress_bar::Style {
                    background: Background::Color(Color::from_rgb(0.06, 0.08, 0.10)),
                    bar: Background::Color(EMERALD),
                    border: Border::default(),
                }),
                text(detail.clone()).size(11).color(DIM),
            ]
            .spacing(12)
            .into(),

            Stage::Done(_) => text("Starting Cyber Launcher\u{2026}").size(12).color(DIM).into(),
            Stage::Failed(err) => text(err.clone()).size(11).color(DIM).into(),
        };

        let body = container(
            column![
                text(eyebrow).size(10).color(DIAMOND),
                text(title).size(24).color(title_colour),
                container(inner)
                    .width(Length::Fill)
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(
                            0.016, 0.027, 0.039, 0.7,
                        ))),
                        border: hairline(),
                        ..Default::default()
                    }),
            ]
            .spacing(12),
        )
        .height(Length::Fill)
        .padding(24);

        let action: Element<'_, Message> = match &self.stage {
            Stage::Confirm => primary("INSTALL", Message::Install).into(),
            Stage::Blocked => primary("CLOSE IT AND INSTALL", Message::CloseAndInstall).into(),
            _ => text("").into(),
        };

        let footer = container(
            row![
                text(match self.stage {
                    Stage::Working { .. } => "WRITING FILES",
                    _ => "PER-USER INSTALL \u{00B7} NO ADMIN REQUIRED",
                })
                .size(10)
                .color(MUTE),
                Space::new().width(Length::Fill),
                action,
            ]
            .align_y(Alignment::Center),
        )
        .height(64)
        .padding([0, 24])
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.016, 0.027, 0.039, 0.45))),
            ..Default::default()
        });

        container(column![header, rule(), rail, rule(), body, rule(), footer])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(BG)),
                text_color: Some(INK),
                ..Default::default()
            })
            .into()
    }

    /// Runs the install on a worker thread and streams progress in.
    ///
    /// A thread rather than a task: the work is blocking file I/O, and doing it
    /// on the UI thread would freeze the window for the whole install — which
    /// reads as a hang at exactly the moment the user is watching.
    fn subscription(&self) -> Subscription<Message> {
        match self.stage {
            Stage::Working { .. } => {
                Subscription::run(|| {
                    iced::stream::channel(32, |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
                        std::thread::spawn(move || {
                            install::run(|p| {
                                let _ = tx.blocking_send(p);
                            });
                        });
                        while let Some(p) = rx.recv().await {
                            use iced::futures::SinkExt;
                            let _ = output.send(Message::Progress(p)).await;
                        }
                    })
                })
            }
            _ => Subscription::none(),
        }
    }
}

/// A function item, not the closure `|_| Theme::Dark`.
///
/// A closure taking `&Installer` is inferred at one concrete lifetime, and
/// `iced::application` needs it to hold for any — so the closure form fails
/// with "implementation of `FnOnce` is not general enough", reported against
/// the whole builder chain rather than against the theme argument. A `fn` is
/// higher-ranked by construction.
fn theme(_: &Installer) -> Theme {
    Theme::Dark
}

fn main() -> iced::Result {
    // Uninstall is the same binary under a flag, so there is no second
    // executable to build, sign and keep in step.
    if std::env::args().any(|a| a == "--uninstall") {
        return match install::uninstall() {
            Ok(()) => Ok(()),
            Err(err) => {
                eprintln!("uninstall failed: {err}");
                std::process::exit(1);
            }
        };
    }

    iced::application(Installer::default, Installer::update, Installer::view)
        .title("Cyber Launcher Setup")
        .window_size((820.0, 498.0))
        .resizable(false)
        // No system title bar. Windows paints it with the user's accent
        // colour, which on a dark window is a bright band the design has no
        // way to influence; the header below replaces it.
        .decorations(false)
        .subscription(Installer::subscription)
        .theme(theme)
        .run()
}
