//! The tray icon and its menu.  Clicking an item sends an [`Action`] to the
//! main loop; the loop pushes new [`View`]s back with `Handle::update`.

use std::sync::mpsc::Sender;

use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, ToolTip};
use pomodoro_core::pomodoro::ICON_STEPS;
use pomodoro_core::{Action, View, icon};

use crate::runner::Input;

/// Pixmap sizes offered to the panel; it picks the closest one.
const ICON_SIZES: [u32; 4] = [16, 22, 32, 48];

pub struct Tray {
    pub view: View,
    tx: Sender<Input>,
}

impl Tray {
    pub fn new(view: View, tx: Sender<Input>) -> Self {
        Self { view, tx }
    }

    fn item(&self, label: &str, enabled: bool, action: Action) -> MenuItem<Self> {
        StandardItem {
            label: label.into(),
            enabled,
            activate: Box::new(move |t: &mut Self| t.send(action)),
            ..Default::default()
        }
        .into()
    }

    fn send(&self, action: Action) {
        // Fails only while the app is quitting.
        let _ = self.tx.send(Input::Action(action));
    }
}

impl ksni::Tray for Tray {
    // A click on the icon opens the menu too.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "pomodoro".into()
    }

    fn title(&self) -> String {
        "Pomodoro".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let fraction = f64::from(self.view.icon_step) / f64::from(ICON_STEPS);
        ICON_SIZES
            .iter()
            .map(|&size| Icon {
                width: size as i32,
                height: size as i32,
                data: icon::render(size, self.view.phase, fraction),
            })
            .collect()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Pomodoro".into(),
            description: self.view.status.clone(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let v = &self.view;
        let mut items = vec![
            StandardItem {
                label: v.status.clone(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            self.item(&v.start_label, v.can_start, Action::Start),
            self.item("Stop", v.can_stop, Action::Stop),
            self.item("Reset", v.can_reset, Action::Reset),
            self.item("Hide notification", v.notice_visible, Action::HideNotice),
            MenuItem::Separator,
        ];
        if let Some((label, enabled)) = &v.update {
            items.push(self.item(label, *enabled, Action::Update));
        }
        items.extend([
            CheckmarkItem {
                label: "Start at login".into(),
                checked: v.autostart,
                activate: Box::new(|t: &mut Self| t.send(Action::ToggleAutostart)),
                ..Default::default()
            }
            .into(),
            self.item("Quit", true, Action::Quit),
        ]);
        items
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use ksni::Tray as _;
    use pomodoro_core::Pomodoro;

    use super::*;

    fn tray(app: &Pomodoro, now: u64) -> (Tray, mpsc::Receiver<Input>) {
        let (tx, rx) = mpsc::channel();
        (Tray::new(app.view(Duration::from_secs(now)), tx), rx)
    }

    /// (label, enabled) of the standard items, checkmark as "[x] label".
    fn items(t: &Tray) -> Vec<(String, bool)> {
        t.menu()
            .into_iter()
            .map(|item| match item {
                MenuItem::Standard(s) => (s.label, s.enabled),
                MenuItem::Checkmark(c) => (
                    format!("[{}] {}", if c.checked { "x" } else { " " }, c.label),
                    c.enabled,
                ),
                MenuItem::Separator => ("--".into(), false),
                _ => ("?".into(), false),
            })
            .collect()
    }

    fn app() -> Pomodoro {
        Pomodoro::new(Duration::from_secs(1200), Duration::from_secs(15), true)
    }

    #[test]
    fn idle_menu() {
        let (t, _) = tray(&app(), 0);
        let s = |l: &str, e| (l.to_string(), e);
        assert_eq!(
            items(&t),
            vec![
                s("Ready: 20 min", false),
                s("--", false),
                s("Start (20 min)", true),
                s("Stop", false),
                s("Reset", false),
                s("Hide notification", false),
                s("--", false),
                s("[x] Start at login", true),
                s("Quit", true),
            ]
        );
    }

    #[test]
    fn running_menu_enables_stop_and_reset() {
        let mut p = app();
        p.handle(Action::Start, Duration::ZERO);
        let (t, _) = tray(&p, 90);
        let items = items(&t);
        assert_eq!(items[0].0, "Working: 18:30 left");
        assert_eq!(items[2], ("Start (20 min)".into(), false));
        assert_eq!(items[3], ("Stop".into(), true));
        assert_eq!(items[4], ("Reset".into(), true));
    }

    #[test]
    fn clicks_send_actions() {
        let (mut t, rx) = tray(&app(), 0);
        let expected = [
            (2, Action::Start),
            (3, Action::Stop),
            (4, Action::Reset),
            (5, Action::HideNotice),
            (7, Action::ToggleAutostart),
            (8, Action::Quit),
        ];
        for (index, action) in expected {
            match t.menu().swap_remove(index) {
                MenuItem::Standard(s) => (s.activate)(&mut t),
                MenuItem::Checkmark(c) => (c.activate)(&mut t),
                _ => panic!("item {index} is not clickable"),
            }
            assert_eq!(rx.try_recv(), Ok(Input::Action(action)));
        }
    }

    #[test]
    fn update_item_appears_when_there_is_one() {
        let mut p = app();
        p.update_event(pomodoro_core::update::Event::Found(
            pomodoro_core::update::Release {
                version: pomodoro_core::update::Version(9, 0, 0),
                page_url: String::new(),
                deb_name: "pomodoro_9.0.0_amd64.deb".into(),
                deb_url: String::new(),
                deb_size: 2_000_000,
                sha256: None,
            },
        ));
        let (mut t, rx) = tray(&p, 0);
        assert_eq!(
            items(&t)[7],
            ("Install update 9.0.0 (2.0 MB)…".into(), true)
        );
        assert_eq!(items(&t).len(), 10);
        match t.menu().swap_remove(7) {
            MenuItem::Standard(s) => (s.activate)(&mut t),
            _ => panic!("not clickable"),
        }
        assert_eq!(rx.try_recv(), Ok(Input::Action(Action::Update)));
    }

    #[test]
    fn icon_has_every_size_and_tooltip_shows_status() {
        let (t, _) = tray(&app(), 0);
        let icons = t.icon_pixmap();
        assert_eq!(icons.len(), ICON_SIZES.len());
        for i in icons {
            assert_eq!(i.data.len(), (i.width * i.height * 4) as usize);
        }
        assert_eq!(t.tool_tip().description, "Ready: 20 min");
    }
}
