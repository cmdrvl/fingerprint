#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let code = fingerprint::run();
    std::process::ExitCode::from(code)
}
