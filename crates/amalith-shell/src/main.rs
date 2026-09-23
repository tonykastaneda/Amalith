//! Amalith — binary entry point. The shell itself lives in the library
//! ([`amalith_shell::app`]); this launches it, or runs the headless script
//! engine when the first argument asks for it.
//!
//! `Amalith script a.jsx [b.jsx ...]` runs Illustrator-style `.jsx`
//! automation against Amalith's own document engine and exits without ever
//! creating a window. That used to be a separate `amalith-script`
//! executable, but it shares `amalith-core`/`-commands`/`-io`, parley,
//! skrifa and kurbo with the shell — only the JS engine was genuinely extra
//! — so shipping two binaries duplicated most of the second one's size.
//!
//! Anything else on the command line is treated as a document to open (see
//! [`amalith_shell::app::run`]).

// Windows: a GUI app, not a console app — don't spawn a terminal window
// behind it. Debug builds keep the console so `println!` / panics show.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::process::ExitCode;

/// Reserved first argument that selects the headless engine. A file with
/// this exact name would be shadowed by it, which is why it's a word no
/// document would plausibly be called rather than a bare path.
const SCRIPT_COMMAND: &str = "script";

fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_some_and(|first| first == SCRIPT_COMMAND) {
        return run_scripts();
    }
    amalith_shell::app::run();
    ExitCode::SUCCESS
}

/// `Amalith script <script.jsx> ...` — every argument after the subcommand
/// is a script path. They run sequentially against one shared context, so
/// `$.global` and any documents a script opened persist from one file to the
/// next, exactly as `amalith-script run` did.
fn run_scripts() -> ExitCode {
    attach_parent_console();

    let scripts: Vec<std::path::PathBuf> =
        std::env::args_os().skip(2).map(std::path::PathBuf::from).collect();
    if scripts.is_empty() {
        eprintln!("usage: Amalith {SCRIPT_COMMAND} <script.jsx> [<script2.jsx> ...]");
        return ExitCode::FAILURE;
    }
    match amalith_script::run_pipeline(&scripts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Borrow the console that launched us, so the subcommand can actually print.
///
/// A release Windows build is GUI-subsystem (see the crate attribute above),
/// which means it starts with no console and NULL standard handles — every
/// `eprintln!` here would vanish. `AttachConsole` joins the parent's console
/// and `CONOUT$` gives us a handle to install as stdout/stderr; this runs
/// before any output, which is what makes re-pointing the handles effective.
///
/// Declared by hand rather than via a binding crate: these four symbols are
/// stable Win32 ABI, so this can't drift with a dependency's module layout.
/// A no-op when there's no parent console (double-clicked, say) — output then
/// goes nowhere, exactly as it would have before.
#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use std::ffi::c_void;

    type Handle = *mut c_void;
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (DWORD)-11
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4; // (DWORD)-12
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;

    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn CreateFileA(
            file_name: *const u8,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: Handle,
        ) -> Handle;
        fn SetStdHandle(std_handle: u32, handle: Handle) -> i32;
    }

    // SAFETY: plain Win32 calls with a NUL-terminated literal and null
    // optional pointers; every return value is checked before use.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        let invalid = usize::MAX as Handle;
        let conout = CreateFileA(
            c"CONOUT$".to_bytes_with_nul().as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if !conout.is_null() && conout != invalid {
            SetStdHandle(STD_OUTPUT_HANDLE, conout);
            SetStdHandle(STD_ERROR_HANDLE, conout);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}
