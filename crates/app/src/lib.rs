//! The desktop side of Pomodoro: the tray icon (StatusNotifierItem over
//! D-Bus), notifications and the loop that drives [`pomodoro_core::Pomodoro`].

pub mod clock;
pub mod notifier;
pub mod runner;
pub mod tray;
pub mod update;
