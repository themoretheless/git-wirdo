#![cfg(windows)]
//! Native Windows evidence: retain a descendant handle across Job Object cleanup.
use git_wirdo::process::{Control, controlled, output};
use std::{
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
};

struct CancelOnDrop(Arc<Control>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
struct Descendant(HANDLE);
impl Drop for Descendant {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

// The child test executable inherits the runner's pipes. Its descendant deliberately
// outlives the parent unless the Job Object stops it; no shell/ping timing oracle.
#[test]
#[allow(
    clippy::zombie_processes,
    reason = "Fixture intentionally leaves a descendant for the Job Object cleanup oracle"
)]
fn windows_process_fixture() {
    let Ok(mode) = std::env::var("GIT_WIRDO_WINDOWS_PROCESS_FIXTURE") else {
        return;
    };
    unsafe extern "system" fn ignore_break(_: u32) -> i32 {
        1
    }
    // Ignore graceful console stop so cancellation exercises forced job termination.
    let registered = unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ignore_break), 1)
    };
    assert_ne!(
        registered,
        0,
        "Cannot register forced-stop fixture handler: {}",
        std::io::Error::last_os_error()
    );
    if mode == "descendant" {
        thread::sleep(Duration::from_secs(30));
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "windows_process_fixture", "--nocapture"])
        .env("GIT_WIRDO_WINDOWS_PROCESS_FIXTURE", "descendant")
        .spawn()
        .unwrap();
    eprintln!("descendant-ready:{}", child.id());
    if mode == "parent-exits" {
        return;
    }
    // Both processes ignore graceful stop; only forced job cleanup ends this wait.
    let _ = child.wait();
}

#[test]
fn cancellation_and_parent_exit_stop_native_descendants_holding_pipes() {
    for cancel in [true, false] {
        let control = Arc::new(Control::default());
        let _cancel_guard = CancelOnDrop(control.clone());
        let task = control.clone();
        let worker = thread::spawn(move || {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "windows_process_fixture", "--nocapture"])
                .env(
                    "GIT_WIRDO_WINDOWS_PROCESS_FIXTURE",
                    if cancel {
                        "parent-waits"
                    } else {
                        "parent-exits"
                    },
                );
            controlled(task, 0, || output(&mut command))
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let handle = loop {
            let progress = control.progress();
            if let Some(pid) = progress
                .strip_prefix("descendant-ready:")
                .and_then(|pid| pid.parse::<u32>().ok())
            {
                let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                // A normally exiting parent can already have stopped its descendant.
                if !handle.is_null() {
                    break Some(Descendant(handle));
                }
                if !cancel {
                    break None;
                }
            }
            if !cancel && worker.is_finished() {
                break None;
            }
            assert!(
                Instant::now() < deadline && !worker.is_finished(),
                "No native descendant progress: {progress}"
            );
            thread::sleep(Duration::from_millis(10));
        };
        if cancel {
            assert!(handle.is_some());
            control.cancel();
        }
        let stopped = Instant::now();
        while !worker.is_finished() {
            assert!(
                stopped.elapsed() < Duration::from_secs(3),
                "Inherited pipes or descendant survived cleanup"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let result = worker.join().unwrap();
        if cancel {
            assert!(result.unwrap_err().to_string().contains("cancelled"));
        } else {
            let output = result.unwrap();
            assert!(output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("descendant-ready:"),
                "Fixture didn't create a descendant"
            );
        }
        if let Some(handle) = handle {
            assert_eq!(
                unsafe { WaitForSingleObject(handle.0, 1000) },
                WAIT_OBJECT_0,
                "Descendant remains executable"
            );
        }
    }
}
