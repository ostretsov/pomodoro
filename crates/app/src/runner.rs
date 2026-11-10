//! Carries out what [`Pomodoro`] decides.

use std::path::PathBuf;
use std::time::Duration;

use pomodoro_core::autostart::Autostart;
use pomodoro_core::update::{Event, Release};
use pomodoro_core::{Action, Effect, Pomodoro, View};

use crate::notifier::Notifier;

/// What reaches the main loop: menu clicks and update progress.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Action(Action),
    Update(Event),
}

/// Where "Start at login" is stored.
pub trait AutostartStore {
    fn set(&mut self, on: bool) -> std::io::Result<()>;
}

pub struct XdgAutostart {
    pub autostart: Autostart,
    pub exec: PathBuf,
}

impl AutostartStore for XdgAutostart {
    fn set(&mut self, on: bool) -> std::io::Result<()> {
        self.autostart.set(on, &self.exec)
    }
}

/// Background update work; results come back as [`Input::Update`].
pub trait Updates {
    fn check(&mut self);
    fn install(&mut self, release: Release);
    /// Arranges for the new version to start once this process exits.
    fn restart(&mut self) -> anyhow::Result<()>;
}

#[derive(Debug, PartialEq)]
pub enum Step {
    /// Keep going; `Some` when the tray needs the new view.
    Continue(Option<View>),
    Quit,
}

pub struct Runner {
    app: Pomodoro,
    notifier: Box<dyn Notifier>,
    autostart: Box<dyn AutostartStore>,
    updates: Box<dyn Updates>,
    clock: Box<dyn Fn() -> Duration>,
    shown: Option<View>,
}

impl Runner {
    pub fn new(
        app: Pomodoro,
        notifier: Box<dyn Notifier>,
        autostart: Box<dyn AutostartStore>,
        updates: Box<dyn Updates>,
        clock: Box<dyn Fn() -> Duration>,
    ) -> Self {
        Self {
            app,
            notifier,
            autostart,
            updates,
            clock,
            shown: None,
        }
    }

    pub fn view(&self) -> View {
        self.app.view((self.clock)())
    }

    /// One turn of the loop: an optional input, then a clock tick.
    pub fn step(&mut self, input: Option<Input>) -> Step {
        let now = (self.clock)();
        let mut effects = match input {
            Some(Input::Action(a)) => self.app.handle(a, now),
            Some(Input::Update(e)) => self.app.update_event(e),
            None => Vec::new(),
        };
        effects.extend(self.app.tick(now));
        for effect in effects {
            match effect {
                Effect::ShowNotice(notice) => self.notifier.show(&notice),
                Effect::CloseNotice => self.notifier.close(),
                Effect::AnnounceUpdate(notice) => self.notifier.announce(&notice),
                Effect::SetAutostart(on) => {
                    if let Err(e) = self.autostart.set(on) {
                        eprintln!("pomodoro: could not change the autostart entry: {e}");
                        self.app.set_autostart(!on);
                    }
                }
                Effect::CheckUpdate => self.updates.check(),
                Effect::InstallUpdate(r) => self.updates.install(r),
                Effect::Restart => match self.updates.restart() {
                    Ok(()) => return Step::Quit,
                    Err(e) => eprintln!("pomodoro: cannot restart, start it again yourself: {e}"),
                },
                Effect::Quit => return Step::Quit,
            }
        }
        let view = self.app.view(now);
        if self.shown.as_ref() == Some(&view) {
            Step::Continue(None)
        } else {
            self.shown = Some(view.clone());
            Step::Continue(Some(view))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use pomodoro_core::update::Version;
    use pomodoro_core::{Notice, Phase};

    use super::*;

    type Log = Rc<RefCell<Vec<String>>>;

    struct FakeNotifier(Log);

    impl Notifier for FakeNotifier {
        fn show(&mut self, n: &Notice) {
            self.0.borrow_mut().push(format!("show {}", n.summary));
        }
        fn close(&mut self) {
            self.0.borrow_mut().push("close".into());
        }
        fn announce(&mut self, n: &Notice) {
            self.0.borrow_mut().push(format!("announce {}", n.summary));
        }
    }

    struct FakeAutostart {
        on: Rc<Cell<bool>>,
        fail: bool,
    }

    impl AutostartStore for FakeAutostart {
        fn set(&mut self, on: bool) -> std::io::Result<()> {
            if self.fail {
                return Err(std::io::Error::other("read-only"));
            }
            self.on.set(on);
            Ok(())
        }
    }

    struct FakeUpdates(Log);

    impl Updates for FakeUpdates {
        fn check(&mut self) {
            self.0.borrow_mut().push("check".into());
        }
        fn install(&mut self, r: Release) {
            self.0.borrow_mut().push(format!("install {}", r.version));
        }
        fn restart(&mut self) -> anyhow::Result<()> {
            self.0.borrow_mut().push("restart".into());
            Ok(())
        }
    }

    struct Fixture {
        runner: Runner,
        time: Rc<Cell<u64>>,
        autostart: Rc<Cell<bool>>,
        /// Notifications and update work, in order.
        log: Log,
    }

    fn fixture(autostart_fails: bool, app: Pomodoro) -> Fixture {
        let time = Rc::new(Cell::new(0u64));
        let autostart = Rc::new(Cell::new(true));
        let log = Log::default();
        let t = time.clone();
        let runner = Runner::new(
            app,
            Box::new(FakeNotifier(log.clone())),
            Box::new(FakeAutostart {
                on: autostart.clone(),
                fail: autostart_fails,
            }),
            Box::new(FakeUpdates(log.clone())),
            Box::new(move || Duration::from_secs(t.get())),
        );
        Fixture {
            runner,
            time,
            autostart,
            log,
        }
    }

    fn minute_app() -> Pomodoro {
        Pomodoro::new(Duration::from_secs(60), Duration::from_secs(15), true)
    }

    fn act(a: Action) -> Option<Input> {
        Some(Input::Action(a))
    }

    #[test]
    fn reports_the_view_only_when_it_changes() {
        let mut f = fixture(false, minute_app());
        assert!(matches!(f.runner.step(None), Step::Continue(Some(_))));
        assert_eq!(f.runner.step(None), Step::Continue(None));
        f.runner.step(act(Action::Start));
        f.time.set(1);
        let Step::Continue(Some(v)) = f.runner.step(None) else {
            panic!("the countdown changed")
        };
        assert_eq!(v.status, "Working: 00:59 left");
        assert_eq!(f.runner.step(None), Step::Continue(None));
    }

    #[test]
    fn shows_and_closes_the_notification() {
        let mut f = fixture(false, minute_app());
        f.runner.step(act(Action::Start));
        f.time.set(60);
        f.runner.step(None);
        assert_eq!(f.runner.view().phase, Phase::Finished);
        assert_eq!(*f.log.borrow(), ["show Time's up!"]);
        f.time.set(74);
        f.runner.step(None);
        assert_eq!(f.log.borrow().len(), 1);
        f.time.set(75);
        f.runner.step(None);
        assert_eq!(*f.log.borrow(), ["show Time's up!", "close"]);
    }

    #[test]
    fn hide_from_the_menu() {
        let mut f = fixture(false, minute_app());
        f.runner.step(act(Action::Start));
        f.time.set(60);
        f.runner.step(None);
        f.runner.step(act(Action::HideNotice));
        assert_eq!(*f.log.borrow(), ["show Time's up!", "close"]);
        assert!(!f.runner.view().notice_visible);
    }

    #[test]
    fn toggles_autostart_and_reverts_on_failure() {
        let mut f = fixture(false, minute_app());
        f.runner.step(act(Action::ToggleAutostart));
        assert!(!f.autostart.get());
        assert!(!f.runner.view().autostart);

        let mut f = fixture(true, minute_app());
        f.runner.step(act(Action::ToggleAutostart));
        assert!(f.autostart.get());
        assert!(
            f.runner.view().autostart,
            "the checkbox shows what is really on disk"
        );
    }

    #[test]
    fn update_check_install_and_restart() {
        let app = minute_app().with_update_checks(Duration::from_secs(30));
        let mut f = fixture(false, app);
        f.time.set(30);
        f.runner.step(None);
        let release = Release {
            version: Version(9, 0, 0),
            page_url: String::new(),
            deb_name: "pomodoro_9.0.0_amd64.deb".into(),
            deb_url: String::new(),
            deb_size: 1,
            sha256: None,
        };
        f.runner.step(Some(Input::Update(Event::Found(release))));
        f.runner.step(act(Action::Update));
        f.runner.step(Some(Input::Update(Event::Installing)));
        f.runner.step(Some(Input::Update(Event::Installed)));
        assert_eq!(f.runner.step(act(Action::Update)), Step::Quit);
        assert_eq!(
            *f.log.borrow(),
            [
                "check",
                "announce Pomodoro 9.0.0 is available",
                "install 9.0.0",
                "restart"
            ]
        );
    }

    #[test]
    fn quit() {
        let mut f = fixture(false, minute_app());
        assert_eq!(f.runner.step(act(Action::Quit)), Step::Quit);
    }
}
