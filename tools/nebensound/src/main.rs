//! The binary. It passes the arguments on and returns what the tool reports —
//! everything checkable lives in the library beside it.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out = std::io::stdout().lock();
    ExitCode::from(nebensound::run(std::env::args_os(), &mut out))
}
