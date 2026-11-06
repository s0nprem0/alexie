use std::sync::Arc;
use zbus::blocking::{Connection, Proxy};
use zbus::interface;

pub struct DaemonIface;

#[interface(name = "org.alexie.Daemon1")]
impl DaemonIface {
    fn ping(&self) -> String {
        "pong".to_string()
    }

    fn run(&self, command: &str) -> String {
        // Delegate to the same handler used by the socket.
        crate::handle_command(command)
    }
}

pub fn run_dbus_server() {
    // Run on the session bus. Blocking.
    let conn = match Connection::session() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dbus: failed to connect to session bus: {}", e);
            return;
        }
    };
    // Request a well-known name.
    if let Err(e) = conn.request_name("org.alexie.Daemon") {
        eprintln!("dbus: failed to request name: {}", e);
        return;
    }
    // Register object at /org/alexie/Daemon.
    if let Err(e) = conn.object_server().at("/org/alexie/Daemon", DaemonIface) {
        eprintln!("dbus: failed to register object: {}", e);
        return;
    }
    eprintln!("alexie dbus service registered on session bus (org.alexie.Daemon)");
    // Keep the connection alive; zbus dispatches method calls on a background
    // thread. Block forever.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
