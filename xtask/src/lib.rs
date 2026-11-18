//! A temporary xtask for the acceptance run of the central workflow's xtask
//! step: `check --all` passes, so that the run shows a passing xtask in
//! `Check`.

/// Runs the task the arguments name and returns what it reports.
///
/// # Errors
///
/// An unknown task is named.
pub fn run(args: &[String]) -> Result<&'static str, String> {
    match args {
        [task, all] if task == "check" && all == "--all" => Ok("check --all passed"),
        [help] if help == "help" => Ok("usage: cargo xtask check --all"),
        _ => Err(format!("unknown task: {}", args.join(" "))),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn check_all_passes() {
        assert_eq!(run(&args(&["check", "--all"])), Ok("check --all passed"));
    }

    #[test]
    fn help_names_the_task() {
        assert_eq!(run(&args(&["help"])), Ok("usage: cargo xtask check --all"));
    }

    #[test]
    fn an_unknown_task_is_named() {
        assert_eq!(run(&args(&["probe"])).unwrap_err(), "unknown task: probe");
    }
}
