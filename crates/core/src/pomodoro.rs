//! What the app does: menu actions and clock ticks go in, side effects
//! (show/close the notification, write the autostart entry, quit) come out.
//! The tray and the notification daemon only carry these out.

use std::time::Duration;

use crate::timer::{Phase, Timer, format_remaining};
use crate::update::{self, Release};

/// How often an installed app looks for a new release.
pub const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(12 * 3600);
/// The first check waits a little: the network may still be coming up at
/// login.
pub const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(30);

/// The menu of the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Start a full period, or resume a stopped one.
    Start,
    /// Pause the countdown.
    Stop,
    /// Back to the full duration, not running.
    Reset,
    /// Close the "time's up" notification before its 15 seconds are over.
    HideNotice,
    /// Turn "Start at login" on or off.
    ToggleAutostart,
    /// The update menu item: install, retry, or restart into the new version.
    Update,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    ShowNotice(Notice),
    CloseNotice,
    SetAutostart(bool),
    /// Look on GitHub for a newer release (in the background).
    CheckUpdate,
    /// Download, verify and install this release (in the background).
    InstallUpdate(Release),
    /// "Pomodoro 0.2.0 is available" (a normal notification).
    AnnounceUpdate(Notice),
    /// Start the new version and quit.
    Restart,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub summary: String,
    pub body: String,
}

/// Everything the tray shows.  The app pushes it to the tray only when it
/// changed.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub phase: Phase,
    /// First (disabled) menu line, e.g. "Working: 12:34 left".
    pub status: String,
    /// Label of the Start item: "Start (20 min)" or "Resume (12:34)".
    pub start_label: String,
    pub can_start: bool,
    pub can_stop: bool,
    pub can_reset: bool,
    pub notice_visible: bool,
    pub autostart: bool,
    /// The update menu item (label, clickable), shown when there is one.
    pub update: Option<(String, bool)>,
    /// The part of the period that is left, in [`ICON_STEPS`] steps, so the
    /// icon is redrawn only when it would look different.
    pub icon_step: u32,
}

/// The icon's pie has this many distinct sizes.
pub const ICON_STEPS: u32 = 60;

pub struct Pomodoro {
    timer: Timer,
    notice_for: Duration,
    notice_shown_at: Option<Duration>,
    autostart: bool,
    update: update::Status,
    /// When to look for an update next; `None`: never (development builds,
    /// or `check_updates = false`).
    next_update_check: Option<Duration>,
    announced: Option<update::Version>,
}

impl Pomodoro {
    pub fn new(work: Duration, notice_for: Duration, autostart: bool) -> Self {
        Self {
            timer: Timer::new(work),
            notice_for,
            notice_shown_at: None,
            autostart,
            update: update::Status::None,
            next_update_check: None,
            announced: None,
        }
    }

    /// Look for updates from `first` on, then every [`UPDATE_CHECK_EVERY`].
    pub fn with_update_checks(mut self, first: Duration) -> Self {
        self.next_update_check = Some(first);
        self
    }

    /// A report from the update worker.
    pub fn update_event(&mut self, event: update::Event) -> Vec<Effect> {
        self.update.apply(event);
        match &self.update {
            update::Status::Available(r) if self.announced != Some(r.version) => {
                self.announced = Some(r.version);
                vec![Effect::AnnounceUpdate(Notice {
                    summary: format!("Pomodoro {} is available", r.version),
                    body: format!(
                        "You have {}. Install it from the tray icon's menu.",
                        env!("CARGO_PKG_VERSION")
                    ),
                })]
            }
            _ => vec![],
        }
    }

    pub fn timer(&self) -> &Timer {
        &self.timer
    }

    pub fn handle(&mut self, action: Action, now: Duration) -> Vec<Effect> {
        let mut effects = Vec::new();
        match action {
            Action::Start => {
                self.hide_notice(&mut effects);
                self.timer.start(now);
            }
            Action::Stop => self.timer.stop(now),
            Action::Reset => {
                self.hide_notice(&mut effects);
                self.timer.reset();
            }
            Action::HideNotice => self.hide_notice(&mut effects),
            Action::ToggleAutostart => {
                self.autostart = !self.autostart;
                effects.push(Effect::SetAutostart(self.autostart));
            }
            Action::Update => match &self.update {
                update::Status::Available(r) | update::Status::Failed { release: r, .. } => {
                    let r = r.clone();
                    self.update = update::Status::Downloading {
                        release: r.clone(),
                        percent: 0,
                    };
                    effects.push(Effect::InstallUpdate(r));
                }
                update::Status::Installed(_) => {
                    self.hide_notice(&mut effects);
                    effects.push(Effect::Restart);
                }
                _ => {}
            },
            Action::Quit => {
                self.hide_notice(&mut effects);
                effects.push(Effect::Quit);
            }
        }
        effects
    }

    /// Call regularly (a few times a second).
    pub fn tick(&mut self, now: Duration) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Some(shown) = self.notice_shown_at
            && now >= shown + self.notice_for
        {
            self.hide_notice(&mut effects);
        }
        if self.timer.tick(now) {
            if self.notice_shown_at.is_some() {
                effects.push(Effect::CloseNotice);
            }
            self.notice_shown_at = Some(now);
            effects.push(Effect::ShowNotice(self.notice()));
        }
        if let Some(at) = self.next_update_check
            && now >= at
        {
            self.next_update_check = Some(now + UPDATE_CHECK_EVERY);
            if self.update.may_check() {
                effects.push(Effect::CheckUpdate);
            }
        }
        effects
    }

    /// The setting was changed outside the menu (or writing it failed).
    pub fn set_autostart(&mut self, on: bool) {
        self.autostart = on;
    }

    fn hide_notice(&mut self, effects: &mut Vec<Effect>) {
        if self.notice_shown_at.take().is_some() {
            effects.push(Effect::CloseNotice);
        }
    }

    fn notice(&self) -> Notice {
        Notice {
            summary: "Time's up!".into(),
            body: format!(
                "{} of work done. Take a break.",
                minutes_text(self.timer.duration())
            ),
        }
    }

    pub fn view(&self, now: Duration) -> View {
        let phase = self.timer.phase();
        let left = format_remaining(self.timer.remaining(now));
        let full = minutes_text(self.timer.duration());
        let status = match phase {
            Phase::Idle => format!("Ready: {full}"),
            Phase::Running => format!("Working: {left} left"),
            Phase::Paused => format!("Stopped: {left} left"),
            Phase::Finished => "Time's up!".to_string(),
        };
        let start_label = match phase {
            Phase::Paused => format!("Resume ({left})"),
            _ => format!("Start ({full})"),
        };
        View {
            phase,
            status,
            start_label,
            can_start: phase != Phase::Running,
            can_stop: phase == Phase::Running,
            can_reset: phase != Phase::Idle,
            notice_visible: self.notice_shown_at.is_some(),
            autostart: self.autostart,
            update: self.update.menu_item(),
            icon_step: (self.timer.fraction_left(now) * f64::from(ICON_STEPS)).ceil() as u32,
        }
    }
}

/// "20 min", "1 min", "90 sec".
pub fn minutes_text(d: Duration) -> String {
    let secs = d.as_secs();
    if secs.is_multiple_of(60) {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} sec")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);
    const NOTICE: Duration = Duration::from_secs(15);

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    fn app() -> Pomodoro {
        Pomodoro::new(20 * MIN, NOTICE, true)
    }

    fn shown(effects: &[Effect]) -> bool {
        effects.iter().any(|e| matches!(e, Effect::ShowNotice(_)))
    }

    #[test]
    fn idle_view() {
        let v = app().view(s(0));
        assert_eq!(v.phase, Phase::Idle);
        assert_eq!(v.status, "Ready: 20 min");
        assert_eq!(v.start_label, "Start (20 min)");
        assert!(v.can_start && !v.can_stop && !v.can_reset && !v.notice_visible);
        assert!(v.autostart);
        assert_eq!(v.icon_step, ICON_STEPS);
    }

    #[test]
    fn full_cycle_shows_notice_for_15_seconds() {
        let mut p = app();
        assert!(p.handle(Action::Start, s(0)).is_empty());
        let v = p.view(s(61));
        assert_eq!(v.status, "Working: 18:59 left");
        assert!(!v.can_start && v.can_stop && v.can_reset);

        assert!(p.tick(20 * MIN - s(1)).is_empty());
        let fx = p.tick(20 * MIN);
        assert_eq!(
            fx,
            vec![Effect::ShowNotice(Notice {
                summary: "Time's up!".into(),
                body: "20 min of work done. Take a break.".into(),
            })]
        );
        let v = p.view(20 * MIN);
        assert_eq!(v.phase, Phase::Finished);
        assert_eq!(v.status, "Time's up!");
        assert!(v.notice_visible && v.can_start && v.can_reset);
        assert_eq!(v.icon_step, 0);

        assert!(
            p.tick(20 * MIN + NOTICE - s(1)).is_empty(),
            "still visible at 14 s"
        );
        assert_eq!(p.tick(20 * MIN + NOTICE), vec![Effect::CloseNotice]);
        assert!(!p.view(20 * MIN + NOTICE).notice_visible);
        assert!(p.tick(30 * MIN).is_empty(), "closed only once");
    }

    #[test]
    fn hide_closes_the_notice_early() {
        let mut p = app();
        p.handle(Action::Start, s(0));
        p.tick(20 * MIN);
        assert_eq!(
            p.handle(Action::HideNotice, 20 * MIN + s(3)),
            vec![Effect::CloseNotice]
        );
        assert!(p.tick(20 * MIN + NOTICE).is_empty(), "no second close");
        assert!(p.handle(Action::HideNotice, 21 * MIN).is_empty());
    }

    #[test]
    fn start_or_reset_also_closes_the_notice() {
        for action in [Action::Start, Action::Reset] {
            let mut p = app();
            p.handle(Action::Start, s(0));
            p.tick(20 * MIN);
            assert_eq!(p.handle(action, 20 * MIN + s(1)), vec![Effect::CloseNotice]);
        }
    }

    #[test]
    fn stop_resume_and_reset() {
        let mut p = app();
        p.handle(Action::Start, s(0));
        p.handle(Action::Stop, 5 * MIN);
        let v = p.view(50 * MIN);
        assert_eq!(v.status, "Stopped: 15:00 left");
        assert_eq!(v.start_label, "Resume (15:00)");
        assert!(v.can_start && !v.can_stop && v.can_reset);
        assert_eq!(v.icon_step, 45);
        assert!(
            p.tick(50 * MIN).is_empty(),
            "a stopped timer never runs out"
        );

        p.handle(Action::Start, 50 * MIN);
        assert!(shown(&p.tick(65 * MIN)));

        p.handle(Action::Reset, 66 * MIN);
        assert_eq!(p.view(66 * MIN).status, "Ready: 20 min");
    }

    #[test]
    fn toggle_autostart() {
        let mut p = app();
        assert_eq!(
            p.handle(Action::ToggleAutostart, s(0)),
            vec![Effect::SetAutostart(false)]
        );
        assert!(!p.view(s(0)).autostart);
        assert_eq!(
            p.handle(Action::ToggleAutostart, s(0)),
            vec![Effect::SetAutostart(true)]
        );
        p.set_autostart(false);
        assert!(!p.view(s(0)).autostart);
    }

    #[test]
    fn quit_closes_a_visible_notice() {
        let mut p = app();
        assert_eq!(p.handle(Action::Quit, s(0)), vec![Effect::Quit]);
        p.handle(Action::Start, s(0));
        p.tick(20 * MIN);
        assert_eq!(
            p.handle(Action::Quit, 20 * MIN),
            vec![Effect::CloseNotice, Effect::Quit]
        );
    }

    #[test]
    fn icon_step_changes_rarely() {
        let mut p = app();
        p.handle(Action::Start, s(0));
        assert_eq!(p.view(s(0)).icon_step, 60);
        assert_eq!(p.view(s(19)).icon_step, 60);
        assert_eq!(p.view(s(20)).icon_step, 59);
        assert_eq!(p.view(20 * MIN - s(1)).icon_step, 1);
    }

    fn release(minor: u64) -> Release {
        Release {
            version: update::Version(9, minor, 0),
            page_url: String::new(),
            deb_name: format!("pomodoro_9.{minor}.0_amd64.deb"),
            deb_url: String::new(),
            deb_size: 3_000_000,
            sha256: None,
        }
    }

    fn checks(effects: &[Effect]) -> bool {
        effects.contains(&Effect::CheckUpdate)
    }

    #[test]
    fn update_checks_only_when_enabled() {
        let mut p = app();
        assert!(!checks(&p.tick(100 * UPDATE_CHECK_EVERY)));
        let mut p = app().with_update_checks(s(30));
        assert!(!checks(&p.tick(s(29))));
        assert!(checks(&p.tick(s(30))));
        assert!(!checks(&p.tick(s(31))));
        assert!(checks(&p.tick(s(30) + UPDATE_CHECK_EVERY)));
    }

    #[test]
    fn found_update_is_announced_once_and_installed_from_the_menu() {
        let mut p = app().with_update_checks(s(30));
        assert_eq!(p.view(s(0)).update, None);
        assert!(
            p.handle(Action::Update, s(0)).is_empty(),
            "nothing to do yet"
        );

        let fx = p.update_event(update::Event::Found(release(1)));
        assert!(
            matches!(&fx[..], [Effect::AnnounceUpdate(n)] if n.summary == "Pomodoro 9.1.0 is available")
        );
        assert!(
            p.update_event(update::Event::Found(release(1))).is_empty(),
            "announced once"
        );
        assert_eq!(
            p.view(s(0)).update,
            Some(("Install update 9.1.0 (3.0 MB)…".into(), true))
        );

        assert_eq!(
            p.handle(Action::Update, s(0)),
            vec![Effect::InstallUpdate(release(1))]
        );
        assert_eq!(
            p.view(s(0)).update,
            Some(("Downloading 9.1.0… 0%".into(), false))
        );
        assert!(
            p.handle(Action::Update, s(0)).is_empty(),
            "no second install"
        );
        assert!(!checks(&p.tick(s(30))), "no check while installing");

        p.update_event(update::Event::Installing);
        p.update_event(update::Event::Installed);
        assert_eq!(
            p.view(s(0)).update,
            Some(("Restart to use 9.1.0".into(), true))
        );
        assert_eq!(p.handle(Action::Update, s(0)), vec![Effect::Restart]);
    }

    #[test]
    fn failed_update_can_be_retried() {
        let mut p = app();
        p.update_event(update::Event::Found(release(1)));
        p.handle(Action::Update, s(0));
        p.update_event(update::Event::Failed("cancelled".into()));
        assert_eq!(
            p.handle(Action::Update, s(0)),
            vec![Effect::InstallUpdate(release(1))]
        );
    }

    #[test]
    fn minutes_text_formats() {
        assert_eq!(minutes_text(20 * MIN), "20 min");
        assert_eq!(minutes_text(s(90)), "90 sec");
    }
}
