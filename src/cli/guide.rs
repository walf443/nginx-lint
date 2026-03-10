use std::process::ExitCode;

pub fn run_guide() -> ExitCode {
    let guide = include_str!("../../docs/guide.md");
    eprint!("{}", guide);
    ExitCode::SUCCESS
}
