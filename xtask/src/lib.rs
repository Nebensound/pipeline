//! A temporary xtask for the acceptance run of the central workflow's xtask
//! step: `check --all` fails on purpose, so that the run shows what a failing
//! xtask does to `Check`.

/// Runs the task the arguments name and returns what it reports.
///
/// # Errors
///
/// `check --all` fails on purpose, and an unknown task is named; `help`
/// alone succeeds.
pub fn run(args: &[String]) -> Result<&'static str, String> {
    match args {
        [task, all] if task == "check" && all == "--all" => {
            Err("check --all fails on purpose, for the acceptance run".to_owned())
        }
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
    fn check_all_fails_on_purpose() {
        let error = run(&args(&["check", "--all"])).unwrap_err();
        assert!(error.contains("on purpose"));
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
