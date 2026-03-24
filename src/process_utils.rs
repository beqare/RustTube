use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    sync::mpsc::Sender,
    thread,
};

use crate::app_model::WorkerEvent;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    child_slot: Option<Arc<Mutex<Option<u32>>>>,
) -> Result<(bool, String), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not launch process: {error}"))?;
    let child_id = child.id();

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

    if let Some(slot) = &child_slot {
        let mut child_guard = slot.lock().map_err(|_| "failed to lock child process handle".to_owned())?;
        *child_guard = Some(child_id);
    }

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
        *child_guard = None;
    }

    Ok((status.success(), output))
}

pub fn cancel_child_process(slot: &Arc<Mutex<Option<u32>>>) -> Result<bool, String> {
    let pid = {
        let guard = slot.lock().map_err(|_| "failed to lock child process handle".to_owned())?;
        let Some(pid) = *guard else {
            return Ok(false);
        };
        pid
    };

    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("taskkill");
        configure_background_command(&mut command);
        let status = command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|error| format!("failed to stop process tree: {error}"))?;

        if !status.success() {
            return Err(format!("failed to stop process tree for PID {pid}"));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|error| format!("failed to stop process: {error}"))?;

        if !status.success() {
            return Err(format!("failed to stop process for PID {pid}"));
        }
    }

    let mut guard = slot.lock().map_err(|_| "failed to lock child process handle".to_owned())?;
    let Some(current_pid) = *guard else {
        return Ok(false);
    };
    if current_pid == pid {
        *guard = None;
    }

    Ok(true)
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
