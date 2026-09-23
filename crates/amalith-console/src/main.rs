//! `Amalith.com` — the console front door for the Windows install.
//!
//! `Amalith.exe` is a GUI-subsystem program (so double-clicking it doesn't
//! flash a console), and cmd / PowerShell don't wait for GUI programs: typing
//! `Amalith script foo.jsx` would hand the prompt straight back, interleave
//! the script's output with whatever's typed next, and lose its exit code.
//!
//! The installer puts this console program beside `Amalith.exe` as
//! `Amalith.com`. Windows resolves a bare `Amalith` to `.com` before `.exe`
//! (PATHEXT order), so terminals run this instead. For `script` it runs the
//! real app with the same console handles, waits, and returns its exit code.
//! For anything else — opening the editor, with or without files — it
//! launches the app and returns at once, just as `Amalith.exe` would.
//! The same trick Visual Studio uses with `devenv.com` / `devenv.exe`.

use std::ffi::OsString;
use std::process::{Command, ExitCode, Stdio};

/// Must match `SCRIPT_COMMAND` in amalith-shell's `main.rs`.
const SCRIPT_COMMAND: &str = "script";

fn main() -> ExitCode {
    let app = match std::env::current_exe() {
        Ok(me) => me.with_file_name("Amalith.exe"),
        Err(e) => {
            eprintln!("Amalith: can't locate Amalith.exe: {e}");
            return ExitCode::FAILURE;
        }
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    if args.first().is_none_or(|first| first != SCRIPT_COMMAND) {
        // Opening the editor: detach and give the prompt back immediately.
        return match Command::new(&app)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Amalith: can't start {}: {e}", app.display());
                ExitCode::FAILURE
            }
        };
    }

    // Headless run: inherit our console (or redirected) handles so the
    // app's output lands exactly where this program's would have.
    let mut child = match Command::new(&app).args(&args).spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Amalith: can't start {}: {e}", app.display());
            return ExitCode::FAILURE;
        }
    };
    // Ctrl+C or closing the terminal kills this process; the job makes sure
    // the headless app goes with it instead of running on invisibly.
    let _job = kill_with_us(&child);
    match child.wait() {
        Ok(status) => match status.code() {
            Some(0) => ExitCode::SUCCESS,
            // Windows exit codes are 32-bit; clamp so a failure never wraps
            // around to 0.
            Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1).max(1)),
            None => ExitCode::FAILURE,
        },
        Err(e) => {
            eprintln!("Amalith: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Put `child` in a job object that terminates it when the job's last handle
/// closes — which happens when this process exits for any reason. Returns
/// the job handle to keep alive; `None` (no guarantee, but still works) if
/// any call fails.
#[cfg(windows)]
fn kill_with_us(child: &std::process::Child) -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // SAFETY: a fresh unnamed job, a zeroed plain-data struct sized exactly
    // as passed, and the child's live process handle; every result checked.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 || AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
            return None;
        }
        Some(job)
    }
}

/// Only Windows ships this program; the stub keeps the workspace building
/// on macOS and Linux.
#[cfg(not(windows))]
fn kill_with_us(_child: &std::process::Child) -> Option<()> {
    None
}
