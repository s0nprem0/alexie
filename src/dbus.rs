use std::sync::OnceLock;
use zbus::blocking::Connection;
use zbus::interface;

static CONN: OnceLock<Connection> = OnceLock::new();

// ---------- Daemon interface ----------

pub struct DaemonIface;

#[interface(name = "org.alexie.Daemon1")]
impl DaemonIface {
    fn ping(&self) -> String {
        "pong".to_string()
    }

    fn run(&self, command: &str) -> String {
        crate::handle_command(command)
    }

    #[zbus(property)]
    fn theme(&self) -> String {
        theme_current()
    }

    #[zbus(signal)]
    async fn changed(ctxt: &zbus::SignalContext<'_>, name: &str) -> zbus::Result<()>;
}

// ---------- Monitors interface ----------

pub struct MonitorsIface;

#[interface(name = "org.alexie.Monitors1")]
impl MonitorsIface {
    fn status(&self) -> String {
        crate::monitors::handle(&["status".to_string()])
    }
    fn list(&self) -> String {
        crate::monitors::handle(&["list".to_string()])
    }
    fn set(&self, output: &str, key: &str, value: &str) -> String {
        let r = crate::monitors::handle(&["set".to_string(), output.to_string(), key.to_string(), value.to_string()]);
        emit_monitors_changed();
        r
    }
    fn scale_up(&self) -> String {
        let r = crate::monitors::handle(&["scale".to_string(), "up".to_string()]);
        emit_monitors_changed();
        r
    }
    fn scale_down(&self) -> String {
        let r = crate::monitors::handle(&["scale".to_string(), "down".to_string()]);
        emit_monitors_changed();
        r
    }
    fn scale_set(&self, value: f64) -> String {
        let r = crate::monitors::handle(&["scale".to_string(), "set".to_string(), value.to_string()]);
        emit_monitors_changed();
        r
    }
    fn scale_reset(&self) -> String {
        let r = crate::monitors::handle(&["scale".to_string(), "reset".to_string()]);
        emit_monitors_changed();
        r
    }
    fn brightness_up(&self, step: i64) -> String {
        crate::monitors::handle(&["brightness".to_string(), "up".to_string(), step.to_string()])
    }
    fn brightness_down(&self, step: i64) -> String {
        crate::monitors::handle(&["brightness".to_string(), "down".to_string(), step.to_string()])
    }
    fn brightness_set(&self, value: i64) -> String {
        crate::monitors::handle(&["brightness".to_string(), "set".to_string(), value.to_string()])
    }
    fn brightness_max(&self) -> String {
        crate::monitors::handle(&["brightness".to_string(), "max".to_string()])
    }
    fn brightness_min(&self) -> String {
        crate::monitors::handle(&["brightness".to_string(), "min".to_string()])
    }
    fn identify(&self) -> String {
        crate::monitors::handle(&["identify".to_string()])
    }
    fn wake(&self) -> String {
        crate::monitors::handle(&["wake".to_string()])
    }
    fn recover(&self) -> String {
        crate::monitors::handle(&["recover".to_string()])
    }

    #[zbus(property)]
    fn focused_monitor(&self) -> String {
        crate::monitors::focused_name()
    }

    #[zbus(signal)]
    async fn changed(ctxt: &zbus::SignalContext<'_>) -> zbus::Result<()>;
}

// ---------- Theme interface ----------

pub struct ThemeIface;

#[interface(name = "org.alexie.Theme1")]
impl ThemeIface {
    fn list(&self) -> String {
        crate::theme::handle(&["list".to_string()])
    }
    fn current(&self) -> String {
        crate::theme::handle(&["current".to_string()])
    }
    fn next(&self) -> String {
        let r = crate::theme::handle(&["next".to_string()]);
        emit_theme_changed(theme_current());
        r
    }
    fn prev(&self) -> String {
        let r = crate::theme::handle(&["prev".to_string()]);
        emit_theme_changed(theme_current());
        r
    }
    fn set(&self, name: &str) -> String {
        let r = crate::theme::handle(&["set".to_string(), name.to_string()]);
        emit_theme_changed(theme_current());
        r
    }

    #[zbus(property)]
    fn current_theme(&self) -> String {
        theme_current()
    }

    #[zbus(signal)]
    async fn changed(ctxt: &zbus::SignalContext<'_>, name: &str) -> zbus::Result<()>;
}

// ---------- Power interface ----------
pub struct PowerIface;

#[interface(name = "org.alexie.Power1")]
impl PowerIface {
    fn show(&self) -> String {
        crate::power::handle(&["show".to_string()])
    }
    fn apply(&self) -> String {
        crate::power::handle(&["apply".to_string()])
    }
    fn set(&self, source: &str, profile: &str) -> String {
        crate::power::handle(&["set".to_string(), source.to_string(), profile.to_string()])
    }
    fn charge(&self) -> String {
        crate::power::handle(&["charge".to_string()])
    }
    #[zbus(property)]
    fn source(&self) -> String {
        // parse first line of show
        crate::power::handle(&["show".to_string()])
            .lines().next().unwrap_or("").to_string()
    }
}

// ---------- helpers ----------

fn theme_current() -> String {
    // read current file
    let state = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    let p = std::path::PathBuf::from(state).join("anixie").join("current");
    if let Ok(s) = std::fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    String::new()
}

fn emit_monitors_changed() {
    if let Some(conn) = CONN.get() {
        if let Ok(iface_ref) = conn.object_server().interface::<_, MonitorsIface>("/org/alexie/Daemon") {
            let ctxt = iface_ref.signal_context();
            let _ = zbus::block_on(MonitorsIface::changed(ctxt));
        }
    }
}

fn emit_theme_changed(name: String) {
    if let Some(conn) = CONN.get() {
        if let Ok(iface_ref) = conn.object_server().interface::<_, ThemeIface>("/org/alexie/Daemon") {
            let ctxt = iface_ref.signal_context();
            let _ = zbus::block_on(ThemeIface::changed(ctxt, &name));
        }
    }
}

pub fn run_dbus_server() {
    let conn = match Connection::session() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dbus: failed to connect to session bus: {}", e);
            return;
        }
    };
    if let Err(e) = conn.request_name("org.alexie.Daemon") {
        eprintln!("dbus: failed to request name: {}", e);
        return;
    }
    // Register interfaces (multiple interfaces at the same path).
    if let Err(e) = conn.object_server().at("/org/alexie/Daemon", DaemonIface) {
        eprintln!("dbus: register DaemonIface failed: {}", e);
        return;
    }
    if let Err(e) = conn.object_server().at("/org/alexie/Daemon", MonitorsIface) {
        eprintln!("dbus: register MonitorsIface failed: {}", e);
    }
    if let Err(e) = conn.object_server().at("/org/alexie/Daemon", ThemeIface) {
        eprintln!("dbus: register ThemeIface failed: {}", e);
    }
    if let Err(e) = conn.object_server().at("/org/alexie/Daemon", PowerIface) {
        eprintln!("dbus: register PowerIface failed: {}", e);
    }
    let _ = CONN.set(conn.clone());
    eprintln!("alexie dbus service registered (org.alexie.Daemon): Daemon/Monitors/Theme interfaces");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
