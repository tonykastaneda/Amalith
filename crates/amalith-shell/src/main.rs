//! Amalith — binary entry point. The shell itself lives in the library
//! ([`amalith_shell::app`]); this just launches it.

fn main() {
    amalith_shell::app::run();
}
