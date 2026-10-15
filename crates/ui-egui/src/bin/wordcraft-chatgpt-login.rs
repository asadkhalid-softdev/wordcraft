//! WordCraft's standalone browser login helper. Never prints credential contents.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

fn main() -> std::process::ExitCode {
    let args: Result<Vec<_>, _> = std::env::args_os().skip(1).map(|arg| arg.into_string()).collect();
    let result = args.map_err(|_| "Arguments must be valid Unicode".to_owned()).and_then(|args| wordcraft_ui_egui::chatgpt::run_login_helper(&args));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ChatGPT sign-in: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
