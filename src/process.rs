//! A task-scoped process runner. CLI calls outside a task retain their synchronous API.
use anyhow::{Context, Result, bail, ensure};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct Control {
    generation: AtomicUsize,
    progress: Mutex<String>,
}
impl Control {
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
    pub fn generation(&self) -> usize {
        self.generation.load(Ordering::SeqCst)
    }
    pub fn progress(&self) -> String {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn report(&self, text: String) {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = text;
    }
}
#[derive(Clone)]
struct TaskContext {
    control: Arc<Control>,
    generation: usize,
}
thread_local! { static TASK: RefCell<Option<TaskContext>> = const { RefCell::new(None) }; }
struct Scope(Option<TaskContext>);
impl Drop for Scope {
    fn drop(&mut self) {
        TASK.with(|task| *task.borrow_mut() = self.0.take());
    }
}
pub fn controlled<T>(control: Arc<Control>, generation: usize, run: impl FnOnce() -> T) -> T {
    let previous = TASK.with(|task| {
        task.replace(Some(TaskContext {
            control,
            generation,
        }))
    });
    let _scope = Scope(previous);
    run()
}

pub fn output(command: &mut Command) -> Result<Output> {
    output_with_input(command, None)
}
pub fn output_with_input(command: &mut Command, input: Option<&[u8]>) -> Result<Output> {
    let task = TASK.with(|task| task.borrow().clone());
    if let Some(task) = &task {
        ensure!(
            task.control.generation() == task.generation,
            "Operation cancelled"
        );
    }
    if task.is_none() && input.is_none() {
        return command.output().context("Could not execute command");
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    #[cfg(unix)]
    if task.is_some() {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    if task.is_some() {
        use std::os::windows::process::CommandExt;
        command.creation_flags(
            windows_sys::Win32::System::Threading::CREATE_SUSPENDED
                | windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP,
        );
    }
    let mut child = ReapedChild(command.spawn().context("Could not start command")?);
    let tree = if task.is_some() {
        match ProcessTree::new(&child) {
            Ok(tree) => Some(tree),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    } else {
        None
    };
    if let Some(task) = &task {
        task.control.report(format!(
            "Running {}",
            command.get_program().to_string_lossy()
        ));
    }
    let stdout = child.stdout.take().context("Command stdout unavailable")?;
    let stderr = child.stderr.take().context("Command stderr unavailable")?;
    let out = thread::spawn(move || drain(stdout, None));
    let progress = task.as_ref().map(|task| task.control.clone());
    let err = thread::spawn(move || drain(stderr, progress));
    let writer = input.map(|input| {
        let mut stdin = child.stdin.take().expect("piped stdin");
        let input = input.to_vec();
        thread::spawn(move || stdin.write_all(&input))
    });
    let waited = loop {
        if let Some(task) = &task
            && task.control.generation() != task.generation
        {
            if let Some(tree) = &tree {
                tree.request_stop();
                let deadline = Instant::now() + Duration::from_millis(750);
                while Instant::now() < deadline {
                    if child.try_wait().is_ok_and(|status| status.is_some()) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                tree.terminate();
            } else {
                let _ = child.kill();
            }
            break child.wait().context("Could not reap cancelled command");
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                if let Some(tree) = &tree {
                    tree.terminate();
                } else {
                    let _ = child.kill();
                }
                let _ = child.wait();
                break Err(error.into());
            }
        }
    };
    // Close descendants holding inherited pipes, even if the top-level command exited first.
    if let Some(tree) = &tree {
        tree.terminate();
    }
    let stdout = out
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader panicked"))
        .and_then(|result| result.map_err(Into::into));
    let stderr = err
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader panicked"))
        .and_then(|result| result.map_err(Into::into));
    let written = writer.map(|writer| {
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("stdin writer panicked"))
            .and_then(|result| result.map_err(Into::into))
    });
    let status = waited?;
    if let Some(task) = &task
        && task.control.generation() != task.generation
    {
        bail!("Operation cancelled; command process tree stopped");
    }
    if status.success()
        && let Some(written) = written
    {
        written?;
    }
    Ok(Output {
        status,
        stdout: stdout?,
        stderr: stderr?,
    })
}
struct ReapedChild(Child);
impl std::ops::Deref for ReapedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for ReapedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for ReapedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn drain(mut pipe: impl Read, progress: Option<Arc<Control>>) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let count = pipe.read(&mut chunk)?;
        if count == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(control) = &progress {
            let tail = &bytes[bytes.len().saturating_sub(1024)..];
            if let Some(line) = String::from_utf8_lossy(tail)
                .split(['\n', '\r'])
                .rfind(|line| !line.trim().is_empty())
            {
                control.report(line.chars().take(200).collect());
            }
        }
    }
}

#[cfg(unix)]
struct ProcessTree {
    group: libc::pid_t,
    terminated: std::cell::Cell<bool>,
}
#[cfg(unix)]
impl ProcessTree {
    fn new(child: &Child) -> Result<Self> {
        Ok(Self {
            group: child.id() as libc::pid_t,
            terminated: std::cell::Cell::new(false),
        })
    }
    fn request_stop(&self) {
        unsafe {
            libc::kill(-self.group, libc::SIGTERM);
        }
    }
    fn terminate(&self) {
        if self.terminated.replace(true) {
            return;
        }
        // The group was created specifically for this command; never signal the TUI group.
        unsafe {
            libc::kill(-self.group, libc::SIGKILL);
        }
    }
}
#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(windows)]
struct ProcessTree {
    job: windows_sys::Win32::Foundation::HANDLE,
    group: u32,
}
#[cfg(windows)]
impl ProcessTree {
    fn new(child: &Child) -> Result<Self> {
        use std::mem::{size_of, zeroed};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::*,
            System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
        };
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            ensure!(
                !job.is_null(),
                "Cannot create command Job Object: {}",
                std::io::Error::last_os_error()
            );
            let tree = Self {
                job,
                group: child.id(),
            };
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            ensure!(
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32
                ) != 0,
                "Cannot configure command Job Object"
            );
            ensure!(
                AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) != 0,
                "Cannot assign command to Job Object"
            );
            // Spawn suspended so no descendant can escape before assignment to the job.
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            ensure!(
                snapshot != INVALID_HANDLE_VALUE,
                "Cannot inspect suspended command threads"
            );
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut found = false;
            let mut more = Thread32First(snapshot, &mut entry);
            while more != 0 {
                if entry.th32OwnerProcessID == child.id() {
                    let handle = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !handle.is_null() {
                        found = ResumeThread(handle) != u32::MAX;
                        CloseHandle(handle);
                    }
                    break;
                }
                more = Thread32Next(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
            ensure!(
                found,
                "Cannot resume command thread after Job Object assignment"
            );
            Ok(tree)
        }
    }
    fn request_stop(&self) {
        unsafe {
            windows_sys::Win32::System::Console::GenerateConsoleCtrlEvent(
                windows_sys::Win32::System::Console::CTRL_BREAK_EVENT,
                self.group,
            );
        }
    }
    fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}
