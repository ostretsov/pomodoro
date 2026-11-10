//! End-to-end over D-Bus, on a private bus (`dbus-daemon` must be
//! installed): a fake tray host (StatusNotifierWatcher) and a fake
//! notification daemon stand in for GNOME.  Checks that the tray registers,
//! that its menu sends actions when clicked, and what the notification looks
//! like on the wire.
//!
//! One test function only: it points DBUS_SESSION_BUS_ADDRESS of the whole
//! process at the private bus.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ksni::blocking::TrayMethods;
use pomodoro::notifier::{DesktopNotifier, Notifier};
use pomodoro::runner::Input;
use pomodoro::tray::Tray;
use pomodoro_core::{Action, Notice, Pomodoro};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

type Log = Arc<Mutex<Vec<String>>>;

struct Notifications {
    log: Log,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        _replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let urgency = hints.get("urgency").and_then(|v| u8::try_from(v).ok());
        self.log.lock().unwrap().push(format!(
            "notify app={app_name} icon={app_icon} summary={summary} body={body} \
             urgency={urgency:?} timeout={expire_timeout}"
        ));
        42
    }

    fn close_notification(&self, id: u32) {
        self.log.lock().unwrap().push(format!("close {id}"));
    }

    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into()]
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("fake".into(), "test".into(), "1".into(), "1.2".into())
    }
}

struct Watcher {
    items: Log,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, service: String) {
        self.items.lock().unwrap().push(service);
    }

    fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }
}

struct Bus(Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn private_bus() -> Bus {
    let mut child = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("dbus-daemon is installed");
    let mut address = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    // SAFETY: the only test in this binary; nothing else reads the
    // environment concurrently yet.
    unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address.trim()) };
    Bus(child)
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// (id, label, enabled) of the top-level menu items.
fn menu_items(menu: &Proxy) -> Vec<(i32, String, bool)> {
    let (_rev, (_id, _props, children)): (u32, Layout) = menu
        .call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
        .unwrap();
    children
        .into_iter()
        .map(|child| {
            let (id, props, _): Layout = child.try_into().unwrap();
            let label = props
                .get("label")
                .map(|v| String::try_from(v.try_clone().unwrap()).unwrap())
                .unwrap_or_default();
            let enabled = props
                .get("enabled")
                .is_none_or(|v| bool::try_from(v).unwrap());
            (id, label, enabled)
        })
        .collect()
}

#[test]
fn tray_and_notifications_over_dbus() {
    let _bus = private_bus();
    let log: Log = Default::default();
    let items: Log = Default::default();
    let _server = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .name("org.kde.StatusNotifierWatcher")
        .unwrap()
        .serve_at(
            "/org/freedesktop/Notifications",
            Notifications { log: log.clone() },
        )
        .unwrap()
        .serve_at(
            "/StatusNotifierWatcher",
            Watcher {
                items: items.clone(),
            },
        )
        .unwrap()
        .build()
        .unwrap();

    // The notification: critical (GNOME keeps it), closed by the app.
    let mut notifier = DesktopNotifier::default();
    notifier.show(&Notice {
        summary: "Time's up!".into(),
        body: "20 min of work done. Take a break.".into(),
    });
    notifier.close();
    notifier.close(); // nothing left to close
    assert_eq!(
        *log.lock().unwrap(),
        [
            "notify app=Pomodoro icon=pomodoro summary=Time's up! \
             body=20 min of work done. Take a break. urgency=Some(2) timeout=0",
            "close 42",
        ]
    );

    // The tray registers with the host.
    let s = Duration::from_secs;
    let mut app = Pomodoro::new(s(1200), s(15), true);
    let (tx, rx) = mpsc::channel();
    let handle = Tray::new(app.view(s(0)), tx).spawn().expect("tray starts");
    let service = wait_for("the tray to register", || {
        items.lock().unwrap().first().cloned()
    });

    let client = Connection::session().unwrap();
    let item = Proxy::new(
        &client,
        service.clone(),
        "/StatusNotifierItem",
        "org.kde.StatusNotifierItem",
    )
    .unwrap();
    assert_eq!(item.get_property::<String>("Title").unwrap(), "Pomodoro");
    assert_eq!(item.get_property::<String>("Id").unwrap(), "pomodoro");
    assert!(
        item.get_property::<bool>("ItemIsMenu").unwrap(),
        "a click opens the menu"
    );
    let icons: Vec<(i32, i32, Vec<u8>)> = item.get_property("IconPixmap").unwrap();
    assert_eq!(
        icons.iter().map(|i| i.0).collect::<Vec<_>>(),
        [16, 22, 32, 48]
    );

    let menu_path: zbus::zvariant::OwnedObjectPath = item.get_property("Menu").unwrap();
    let menu = Proxy::new(
        &client,
        service.clone(),
        menu_path,
        "com.canonical.dbusmenu",
    )
    .unwrap();
    let menu_now = menu_items(&menu);
    let labels: Vec<_> = menu_now.iter().map(|i| i.1.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Ready: 20 min",
            "",
            "Start (20 min)",
            "Stop",
            "Reset",
            "Hide notification",
            "",
            "Start at login",
            "Quit",
        ]
    );

    // Clicking "Start" in the panel's menu reaches the app.
    let start = menu_now.iter().find(|i| i.1 == "Start (20 min)").unwrap().0;
    let _: () = menu
        .call("Event", &(start, "clicked", Value::from(0i32), 0u32))
        .unwrap();
    let action = rx.recv_timeout(s(5)).expect("the click arrives");
    let Input::Action(action) = action else {
        panic!("{action:?}")
    };
    assert_eq!(action, Action::Start);

    // The app's new state shows up in the menu.
    app.handle(action, s(0));
    let view = app.view(s(60));
    handle.update(|t| t.view = view);
    wait_for("the menu to update", || {
        let items = menu_items(&menu);
        (items[0].1 == "Working: 19:00 left").then_some(())
    });
    let items = menu_items(&menu);
    assert!(!items[2].2, "Start disabled while running");
    assert_eq!((items[3].1.as_str(), items[3].2), ("Stop", true));

    handle.shutdown().wait();
}
