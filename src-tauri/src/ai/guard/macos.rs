//! A watchdog child instead of a Job Object: it sleeps in `kevent` until the app or the server
//! exits, so it costs no wakeups.

use std::process::{Child, Command, Stdio};

use super::WATCHDOG_FLAG;

pub struct LifetimeGuard {
    watchdog: Child,
}

impl Drop for LifetimeGuard {
    fn drop(&mut self) {
        let _ = self.watchdog.kill();
        let _ = self.watchdog.wait();
    }
}

pub fn prepare(_command: &mut Command) {}

pub fn attach(child: &Child) -> Result<LifetimeGuard, String> {
    // Best effort: a refused nice only means the server competes a little harder.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, child.id() as libc::id_t, 10);
    }
    let exe = std::env::current_exe().map_err(|error| format!("no path to self: {error}"))?;
    let watchdog = Command::new(exe)
        .arg(WATCHDOG_FLAG)
        .arg(std::process::id().to_string())
        .arg(child.id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start the watchdog: {error}"))?;
    Ok(LifetimeGuard { watchdog })
}

/// Exits when either process does; kills the child first if the app went first.
pub fn run_watchdog(app: u32, child: u32) -> i32 {
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        return 1;
    }
    // A pid that is already gone fails to register (ESRCH), which counts as having exited.
    let watch = |pid: u32| {
        let change = libc::kevent {
            ident: pid as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) >= 0 }
    };
    let kill_child = || unsafe {
        libc::kill(child as libc::pid_t, libc::SIGKILL);
    };
    if !watch(child) {
        return 0;
    }
    if !watch(app) {
        kill_child();
        return 0;
    }
    loop {
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };
        let ready =
            unsafe { libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null()) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return 1;
        }
        if ready > 0 {
            if event.ident == app as libc::uintptr_t {
                kill_child();
            }
            return 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn sleeper(seconds: &str) -> Child {
        Command::new("sleep").arg(seconds).spawn().unwrap()
    }

    fn wait_gone(child: &mut Child) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn the_child_is_killed_when_the_app_exits_first() {
        let mut app = sleeper("0.3");
        let mut child = sleeper("60");
        assert_eq!(run_watchdog(app.id(), child.id()), 0);
        assert!(wait_gone(&mut child), "the server outlived the app");
        let _ = app.wait();
    }

    #[test]
    fn the_watchdog_leaves_when_the_server_exits_first() {
        let mut app = sleeper("60");
        let mut child = sleeper("0.3");
        assert_eq!(run_watchdog(app.id(), child.id()), 0);
        assert!(app.try_wait().unwrap().is_none(), "the app was touched");
        let _ = app.kill();
        let _ = app.wait();
        let _ = child.wait();
    }

    #[test]
    fn an_app_that_is_already_gone_takes_the_child_with_it() {
        let mut app = sleeper("0");
        app.wait().unwrap();
        let mut child = sleeper("60");
        assert_eq!(run_watchdog(app.id(), child.id()), 0);
        assert!(wait_gone(&mut child));
    }
}
