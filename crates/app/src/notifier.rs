//! The "time's up" notification (org.freedesktop.Notifications).
//!
//! GNOME hides normal notifications after a few seconds whatever timeout is
//! asked for, so it is sent as *critical* (stays until closed) and the app
//! closes it itself after the configured 15 seconds, or when "Hide
//! notification" is clicked.

use notify_rust::{Notification, NotificationHandle, Timeout, Urgency};
use pomodoro_core::Notice;

pub trait Notifier {
    fn show(&mut self, notice: &Notice);
    fn close(&mut self);
    /// A normal notification the desktop hides by itself (update available).
    fn announce(&mut self, notice: &Notice);
}

#[derive(Default)]
pub struct DesktopNotifier {
    shown: Option<NotificationHandle>,
}

impl Notifier for DesktopNotifier {
    fn show(&mut self, notice: &Notice) {
        self.close();
        let result = Notification::new()
            .appname("Pomodoro")
            .summary(&notice.summary)
            .body(&notice.body)
            .icon("pomodoro")
            .urgency(Urgency::Critical)
            .timeout(Timeout::Never)
            .show();
        match result {
            Ok(handle) => self.shown = Some(handle),
            Err(e) => eprintln!("pomodoro: could not show the notification: {e}"),
        }
    }

    fn close(&mut self) {
        if let Some(handle) = self.shown.take() {
            handle.close();
        }
    }

    fn announce(&mut self, notice: &Notice) {
        let result = Notification::new()
            .appname("Pomodoro")
            .summary(&notice.summary)
            .body(&notice.body)
            .icon("pomodoro")
            .show();
        if let Err(e) = result {
            eprintln!("pomodoro: could not show the notification: {e}");
        }
    }
}
