use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!("usage: amalith-script run <script.jsx> [<script2.jsx> ...]");
        return ExitCode::FAILURE;
    };

    match cmd.as_str() {
        "run" => {
            let scripts: Vec<String> = args.collect();
            if scripts.is_empty() {
                eprintln!("usage: amalith-script run <script.jsx> [<script2.jsx> ...]");
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
        other => {
            eprintln!("unknown command `{other}` (expected `run`)");
            ExitCode::FAILURE
        }
    }
}
