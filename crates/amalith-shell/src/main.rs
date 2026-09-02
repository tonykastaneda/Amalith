//! Amalith — binary entry point. The shell itself lives in the library
//! ([`amalith_shell::app`]); this just launches it.

// Windows: a GUI app, not a console app — don't spawn a terminal window
// behind it. Debug builds keep the console so `println!` / panics show.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

fn main() {
    amalith_shell::app::run();
}
