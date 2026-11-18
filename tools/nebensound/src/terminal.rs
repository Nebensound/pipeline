//! Raw mode and the real key presses.
//!
//! What can only happen at a real terminal lives here: raw mode, the alternate
//! screen, a blocking read. The rest of the interface is a value and is checked
//! without a terminal; this part is checked from inside a pseudo terminal — the
//! same path a person takes.

use std::io;

use nebensound_declaration::Analysis;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event;

use crate::draw::draw;
use crate::output::Output;
use crate::ui::{Flow, Ui};

/// Opens the interface and hands the terminal back the way it was.
///
/// # Errors
///
/// Returns what the terminal failed at, and says so when there is none.
/// Without that question first, the interface would write control codes into a
/// log in a pipeline and nobody would see why.
pub fn open(analysis: &Analysis, output: Output) -> io::Result<()> {
    match output {
        Output::Elsewhere => Err(io::Error::other(
            "the output does not go to a terminal; `check` is the command for a log",
        )),
        Output::Terminal => ratatui::try_init().and_then(|mut terminal| {
            let seen = run(&mut terminal, analysis);
            // Restored either way. What the run says wins: an error from it is
            // the cause, an error while restoring only the consequence.
            seen.and(ratatui::try_restore())
        }),
    }
}

fn run(terminal: &mut DefaultTerminal, analysis: &Analysis) -> io::Result<()> {
    let mut ui = Ui::new(analysis);
    loop {
        let step = terminal
            .draw(|frame| draw(frame, &ui))
            .and_then(|_| event::read())
            .map(|event| ui.event(&event));
        match step {
            // Carrying on is the only case that stays here: q ends the run, and
            // a terminal that is gone ends it too.
            Ok(Flow::Continue) => {}
            other => return other.map(|_| ()),
        }
    }
}
