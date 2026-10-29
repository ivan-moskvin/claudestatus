//! The bridge is a process of its own, and nobody is going to start it by hand.
//! The status line starts it — but only when a Times Gate really is on the
//! network. No device, no process.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use divoomkit::{ClockFace, Device};

use crate::i18n::t;
use crate::panel::config::Config;
use crate::paths;

const LOCK: &str = "divoom.pid";

/// Checked on every call of the status line: it has to go unnoticed.
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);

/// Raised by a signal handler; the bridge reads it and leaves the loop, so that
/// the screen is given its clock face back on the way out. A handler may touch
/// nothing else — this flag is all it is allowed to do.
static STOPPING: AtomicBool = AtomicBool::new(false);

pub fn stopping() -> bool {
    STOPPING.load(Ordering::Relaxed)
}

/// Asks the system to raise the flag instead of killing us outright, so the
/// bridge can put the clock face back before it goes.
#[cfg(unix)]
pub fn catch_signals() {
    unsafe extern "C" fn raise_flag(_signal: i32) {
        STOPPING.store(true, Ordering::Relaxed);
    }
    let handler = raise_flag as *const () as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

#[cfg(windows)]
pub fn catch_signals() {}

fn lock_path() -> Result<PathBuf, String> {
    paths::file(LOCK)
}

/// Starts the bridge in the background if the device is reachable and the
/// bridge is not up yet. Called from the status line, so it keeps quiet: no
/// trouble here is worth spoiling the line.
pub fn ensure_running() {
    let Ok(config) = Config::load() else { return };
    if config.ip.is_empty() || !config.enabled() || running() || !reachable(&config.ip) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };

    let _ = std::process::Command::new(exe)
        .arg("divoom")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn running() -> bool {
    matches!(recorded_pid(), Some(pid) if alive(pid))
}

pub fn take_lock() -> Result<(), String> {
    if running() {
        return Err(t("the bridge is already running").into());
    }
    let path = lock_path()?;
    std::fs::write(path, format!("{}\n", std::process::id())).map_err(|err| err.to_string())
}

/// Whether the pid file still points at us. A bridge that was replaced by a
/// newer one, or whose file was removed by `divoom off`, has to go: otherwise it
/// keeps drawing on a screen nobody expects it to touch, and stopping it is a
/// hunt for a process nobody remembers starting.
pub fn owns_lock() -> bool {
    match lock_path() {
        Err(_) => true,
        Ok(path) => match std::fs::read_to_string(path) {
            Err(_) => false,
            Ok(data) => data.trim().parse::<u32>() == Ok(std::process::id()),
        },
    }
}

pub fn drop_lock() {
    if let Ok(path) = lock_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Stops the bridge and cleans up after it. Called from `clistatus
/// uninstall`: without this the bridge outlives the deleted binary, while the
/// screen keeps the panel and goes into endless loading once the bridge does
/// die.
pub fn stop() {
    let Ok(path) = lock_path() else { return };

    if let Some(pid) = recorded_pid() {
        terminate(pid);
        for _ in 0..30 {
            if !alive(pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if alive(pid) {
            force_kill(pid);
            // It had no chance to put the clock face back, so we do it for it.
            restore();
        }
        println!("{}", t("The Divoom bridge is stopped"));
    }
    let _ = std::fs::remove_file(path);
}

/// Gives the screen back its previous clock face from what the config
/// remembers. Keeps quiet: during an uninstall the device may be switched off,
/// and that is no reason to fail.
pub fn restore() {
    let Ok(config) = Config::load() else { return };
    if config.ip.is_empty() || config.prev_clock_id == 0 {
        return;
    }
    let Ok(ip) = config.ip.parse() else { return };

    let face = ClockFace {
        id: config.prev_clock_id,
        independence: config.prev_independence,
    };
    let _ = Device::at(ip).set_clock_face(config.lcd_index, face);
}

fn recorded_pid() -> Option<u32> {
    let data = std::fs::read_to_string(lock_path().ok()?).ok()?;
    let pid = data.trim().parse::<u32>().ok()?;
    (pid > 0).then_some(pid)
}

/// Does the device answer on its port. No real command is sent: the status line
/// must not wait for the firmware to reply.
fn reachable(ip: &str) -> bool {
    let Ok(addrs) = (ip, 80u16).to_socket_addrs() else {
        return false;
    };
    addrs
        .collect::<Vec<SocketAddr>>()
        .iter()
        .any(|addr| TcpStream::connect_timeout(addr, PROBE_TIMEOUT).is_ok())
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // Signal zero delivers nothing and only asks whether the process is there.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(unix)]
fn terminate(pid: u32) {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
}

#[cfg(unix)]
fn force_kill(pid: u32) {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let asked = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        asked != 0 && code == STILL_ACTIVE as u32
    }
}

/// Windows has no signals: the bridge cannot be asked to put the clock face
/// back, so it is ended outright and `stop` restores the screen itself.
#[cfg(windows)]
fn terminate(pid: u32) {
    force_kill(pid);
}

#[cfg(windows)]
fn force_kill(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return;
        }
        TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_a_process_that_is_there_from_one_that_is_not() {
        assert!(alive(std::process::id()), "we are running");

        // A process that exited: its pid answers nothing. The shell that ends
        // right away is spelled differently on every system.
        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", "exit"]);
            command
        } else {
            let mut command = std::process::Command::new("/bin/sh");
            command.args(["-c", "exit 0"]);
            command
        };

        let mut child = command.spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!alive(pid));
    }

    #[test]
    fn finds_nobody_at_an_address_nothing_listens_on() {
        // 192.0.2.0/24 is reserved for documentation and routes nowhere.
        assert!(!reachable("192.0.2.7"));
        assert!(!reachable("not an address at all"));
    }
}
