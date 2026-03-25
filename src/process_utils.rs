use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    sync::mpsc::Sender,
    thread,
};

use crate::app_model::WorkerEvent;

#[cfg(target_os = "windows")]
use std::{
    mem::{size_of, zeroed},
    os::windows::{io::AsRawHandle, process::CommandExt},
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    },
};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug)]
pub struct ActiveProcess {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub pid: u32,
    #[cfg(target_os = "windows")]
    job_handle: HANDLE,
}

#[cfg(target_os = "windows")]
unsafe impl Send for ActiveProcess {}

#[cfg(target_os = "windows")]
unsafe impl Sync for ActiveProcess {}

#[cfg(target_os = "windows")]
impl Drop for ActiveProcess {
    fn drop(&mut self) {
        if !self.job_handle.is_null() {
            unsafe {
                CloseHandle(self.job_handle);
            }
        }
    }
}

pub fn configure_background_command(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

pub fn run_command_streaming(command: Command, sender: &Sender<WorkerEvent>) -> Result<(bool, String), String> {
    run_command_streaming_with_handle(command, sender, None)
}

pub fn run_command_streaming_with_handle(
    mut command: Command,
    sender: &Sender<WorkerEvent>,
    child_slot: Option<Arc<Mutex<Option<ActiveProcess>>>>,
) -> Result<(bool, String), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not launch process: {error}"))?;

    if let Some(slot) = &child_slot {
        let mut child_guard = slot
            .lock()
            .map_err(|_| {
                terminate_child_best_effort(&mut child);
                "failed to lock child process handle".to_owned()
            })?;
        *child_guard = match register_active_process(&child) {
            Ok(active_process) => Some(active_process),
            Err(error) => {
                terminate_child_best_effort(&mut child);
                return Err(error);
            }
        };
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not capture stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "could not capture stderr".to_owned())?;

    let combined_output = Arc::new(Mutex::new(String::new()));

    let stdout_handle = spawn_stream_reader(stdout, sender.clone(), Arc::clone(&combined_output));
    let stderr_handle = spawn_stream_reader(stderr, sender.clone(), Arc::clone(&combined_output));

    let status = child
        .wait()
        .map_err(|error| format!("failed while waiting for process: {error}"))?;

    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let output = match Arc::try_unwrap(combined_output) {
        Ok(buffer) => buffer.into_inner().unwrap_or_default(),
        Err(buffer) => buffer.lock().map(|text| text.clone()).unwrap_or_default(),
    };

    if let Some(slot) = &child_slot
        && let Ok(mut child_guard) = slot.lock()
    {
        let _ = child_guard.take();
    }

    Ok((status.success(), output))
}

fn terminate_child_best_effort(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub fn cancel_child_process(slot: &Arc<Mutex<Option<ActiveProcess>>>) -> Result<bool, String> {
    let mut guard = slot.lock().map_err(|_| "failed to lock child process handle".to_owned())?;
    let Some(active_process) = guard.take() else {
        return Ok(false);
    };

    #[cfg(not(target_os = "windows"))]
    {
        let status = Command::new("kill")
            .args(["-TERM", &active_process.pid.to_string()])
            .status()
            .map_err(|error| format!("failed to stop process: {error}"))?;

        if !status.success() {
            return Err(format!("failed to stop process for PID {}", active_process.pid));
        }
    }

    drop(active_process);
    Ok(true)
}

pub fn clear_active_process(slot: &Arc<Mutex<Option<ActiveProcess>>>) {
    if let Ok(mut guard) = slot.lock() {
        let _ = guard.take();
    }
}

#[cfg(target_os = "windows")]
fn register_active_process(child: &Child) -> Result<ActiveProcess, String> {
    let job_handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job_handle.is_null() {
        return Err("failed to create Windows job object".to_owned());
    }

    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    let ok = unsafe {
        SetInformationJobObject(
            job_handle,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        unsafe {
            CloseHandle(job_handle);
        }
        return Err("failed to configure Windows job object".to_owned());
    }

    let process_handle = child.as_raw_handle() as HANDLE;
    let ok = unsafe { AssignProcessToJobObject(job_handle, process_handle) };
    if ok == 0 {
        unsafe {
            CloseHandle(job_handle);
        }
        return Err("failed to attach process to Windows job object".to_owned());
    }

    Ok(ActiveProcess {
        pid: child.id(),
        job_handle,
    })
}

#[cfg(not(target_os = "windows"))]
fn register_active_process(child: &Child) -> Result<ActiveProcess, String> {
    Ok(ActiveProcess { pid: child.id() })
}

fn spawn_stream_reader<R: Read + Send + 'static>(
    mut reader: R,
    sender: Sender<WorkerEvent>,
    output: Arc<Mutex<String>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 2048];

        loop {
            let bytes_read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => break,
            };

            let chunk = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

            if let Ok(mut text) = output.lock() {
                text.push_str(&chunk);
            }

            let _ = sender.send(WorkerEvent::LogChunk(chunk));
        }
    })
}
