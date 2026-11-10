//! Everything in Pomodoro that can be tested without a desktop session:
//! the timer, what the tray shows, the icon, settings and autostart.

pub mod autostart;
pub mod icon;
pub mod pomodoro;
pub mod settings;
pub mod timer;
pub mod update;

pub use pomodoro::{Action, Effect, Notice, Pomodoro, View};
pub use settings::Settings;
pub use timer::{Phase, Timer};
