// No console window: this is a GUI binary, and a flashing black window before
// the installer paints is the first thing a user sees.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Lunar Launcher's installer.
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

use iced::widget::{column, container, progress_bar, row, text};
use iced::{Alignment, Background, Color, Element, Length, Subscription, Task, Theme};

// --- Cyber Network palette --------------------------------------------------
// Same tokens as the launcher's cyber.css, so the installer and the thing it
// installs look like one product.
const BG: Color = Color::from_rgb(0.020, 0.027, 0.039); // #05070a
const INK: Color = Color::from_rgb(0.914, 0.957, 0.957); // #e9f4f4
const DIM: Color = Color::from_rgb(0.576, 0.659, 0.678); // #93a8ad
const MUTE: Color = Color::from_rgb(0.357, 0.427, 0.459); // #5b6d75
const EMERALD: Color = Color::from_rgb(0.208, 0.851, 0.478); // #35d97a
const REDSTONE: Color = Color::from_rgb(1.000, 0.239, 0.310); // #ff3d4f

#[derive(Debug, Clone)]
enum Message {
    Progress(install::Progress),
    Launch,
}

enum Stage {
    Working { fraction: f32, detail: String },
    Done(std::path::PathBuf),
    Failed(String),
}

struct Installer {
    stage: Stage,
}

impl Default for Installer {
    fn default() -> Self {
        Self {
            stage: Stage::Working {
                fraction: 0.0,
                detail: String::from("Preparing"),
            },
        }
    }
}

impl Installer {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
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
                self.stage = Stage::Failed(err);
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
        let mark = container(text("CN").size(11).color(BG))
            .width(26)
            .height(26)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(|_| container::Style {
                background: Some(Background::Color(EMERALD)),
                ..Default::default()
            });

        let brand = column![
            text("CYBER NETWORK").size(15).color(INK),
            text("LUNAR LAUNCHER SETUP").size(9).color(MUTE),
        ]
        .spacing(3);

        let header = row![mark, brand].spacing(12).align_y(Alignment::Center);

        let body: Element<'_, Message> = match &self.stage {
            Stage::Working { fraction, detail } => column![
                text("INSTALLING").size(22).color(INK),
                // The closure parameter is deliberately unannotated. Writing
                // `|_: &Theme|` binds it to one concrete lifetime rather than
                // leaving it higher-ranked, and `iced::application` then
                // rejects the whole `view` with "implementation of `Fn` is not
                // general enough" — an error that points at the builder call
                // and says nothing about this line.
                progress_bar(0.0..=1.0, *fraction).style(|_| progress_bar::Style {
                    background: Background::Color(Color::from_rgb(0.06, 0.08, 0.10)),
                    bar: Background::Color(EMERALD),
                    border: Default::default(),
                }),
                text(detail.clone()).size(11).color(DIM),
            ]
            .spacing(14)
            .into(),

            Stage::Done(_) => column![
                text("INSTALLED").size(22).color(INK),
                text("Starting Lunar Launcher…").size(11).color(DIM),
            ]
            .spacing(10)
            .into(),

            Stage::Failed(err) => column![
                text("INSTALL FAILED").size(22).color(REDSTONE),
                // Shown in full rather than summarised: this window is the only
                // place the reason ever appears, and there is no log yet —
                // nothing has been installed to write one.
                text(err.clone()).size(11).color(DIM),
            ]
            .spacing(10)
            .into(),
        };

        container(
            column![header, body].spacing(28).padding(28).width(Length::Fill),
        )
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
        .title("Lunar Launcher Setup")
        .window_size((460.0, 240.0))
        .resizable(false)
        .subscription(Installer::subscription)
        .theme(theme)
        .run()
}
