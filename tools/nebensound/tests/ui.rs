//! The interface: what a key changes about the state, and what ends up on the
//! screen.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound::draw::draw;
use nebensound::ui::{Flow, Ui};
use nebensound_declaration::Analysis;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
};
use support::analysis;

const QUIET: &str = "[repo]\npipeline = \"none\"\n";

/// A tree with two gaps, one of them two levels deep.
fn nested() -> Analysis {
    analysis(
        QUIET,
        &["README.md", "notes.xyz", "src/b.md", "src/deep/a.rs"],
    )
}

fn press(ui: &mut Ui<'_>, keys: &[KeyCode]) -> Flow {
    let mut flow = Flow::Continue;
    for key in keys {
        flow = ui.key(*key);
    }
    flow
}

fn paths(ui: &Ui<'_>) -> Vec<String> {
    ui.rows()
        .iter()
        .map(|row| row.node.path.to_string())
        .collect()
}

fn screen(ui: &Ui<'_>) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 24)).expect("a terminal");
    terminal.draw(|frame| draw(frame, ui)).expect("drawn");
    let buffer = terminal.backend().buffer().clone();
    let area = buffer.area;
    let mut text = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

#[test]
fn the_way_to_every_gap_is_open() {
    let analysis = nested();
    let ui = Ui::new(&analysis);

    assert_eq!(
        paths(&ui),
        vec![
            "README.md",
            "notes.xyz",
            "src",
            "src/b.md",
            "src/deep",
            "src/deep/a.rs"
        ],
        "what is missing should be visible without looking for it"
    );
    assert_eq!(ui.index(), 0);
    assert!(!ui.only_gaps());
    assert!(!ui.help());
}

#[test]
fn moving_stays_inside_the_frame() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    press(&mut ui, &[KeyCode::Down; 10]);
    assert_eq!(ui.index(), 5);
    assert_eq!(
        ui.selected().expect("a selection").path.to_string(),
        "src/deep/a.rs"
    );

    press(&mut ui, &[KeyCode::Char('k'); 10]);
    assert_eq!(ui.index(), 0);
}

#[test]
fn folding_up_and_open_again() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    press(&mut ui, &[KeyCode::Char('j'), KeyCode::Char('j')]);
    assert_eq!(ui.selected().expect("a selection").path.to_string(), "src");

    press(&mut ui, &[KeyCode::Char('h')]);
    assert_eq!(paths(&ui), vec!["README.md", "notes.xyz", "src"]);

    press(&mut ui, &[KeyCode::Enter]);
    assert_eq!(paths(&ui).len(), 6);
}

#[test]
fn folding_up_steps_out_when_there_is_nothing_open_here() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    // src/b.md lies one level down and is a file.
    press(&mut ui, &[KeyCode::Down, KeyCode::Down, KeyCode::Down]);
    assert_eq!(
        ui.selected().expect("a selection").path.to_string(),
        "src/b.md"
    );

    press(&mut ui, &[KeyCode::Left]);
    assert_eq!(ui.selected().expect("a selection").path.to_string(), "src");

    // At the top there is no level left to step out of.
    press(&mut ui, &[KeyCode::Left, KeyCode::Left]);
    assert_eq!(ui.selected().expect("a selection").path.to_string(), "src");
}

#[test]
fn unfolding_does_nothing_at_a_file() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    let before = paths(&ui);
    press(&mut ui, &[KeyCode::Right]);

    assert_eq!(paths(&ui), before);
}

#[test]
fn g_jumps_from_gap_to_gap_and_around_again() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    press(&mut ui, &[KeyCode::Char('g')]);
    assert_eq!(
        ui.selected().expect("a selection").path.to_string(),
        "notes.xyz"
    );

    press(&mut ui, &[KeyCode::Char('g')]);
    assert_eq!(
        ui.selected().expect("a selection").path.to_string(),
        "src/deep/a.rs"
    );

    press(&mut ui, &[KeyCode::Char('g')]);
    assert_eq!(
        ui.selected().expect("a selection").path.to_string(),
        "notes.xyz",
        "after the last one comes the first again"
    );
}

#[test]
fn g_finds_the_gap_even_when_it_is_folded_away() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    // Fold everything up first, then jump: the way unfolds by itself.
    press(
        &mut ui,
        &[
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Char('g'),
        ],
    );

    assert_eq!(
        ui.selected().expect("a selection").path.to_string(),
        "src/deep/a.rs"
    );
}

#[test]
fn without_a_gap_nothing_jumps() {
    let analysis = analysis(QUIET, &["README.md"]);
    let mut ui = Ui::new(&analysis);

    press(&mut ui, &[KeyCode::Char('g')]);

    assert_eq!(ui.index(), 0);
}

#[test]
fn the_filter_shows_only_the_gaps() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    press(&mut ui, &[KeyCode::Char('f')]);
    assert!(ui.only_gaps());
    assert_eq!(paths(&ui), vec!["notes.xyz", "src/deep/a.rs"]);

    press(&mut ui, &[KeyCode::Char('f')]);
    assert!(!ui.only_gaps());
    assert_eq!(paths(&ui).len(), 6);
}

#[test]
fn the_help_opens_and_closes_on_the_next_key() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    assert_eq!(press(&mut ui, &[KeyCode::Char('?')]), Flow::Continue);
    assert!(ui.help());

    assert_eq!(press(&mut ui, &[KeyCode::Char('j')]), Flow::Continue);
    assert!(!ui.help());

    press(&mut ui, &[KeyCode::Char('?')]);
    assert_eq!(
        press(&mut ui, &[KeyCode::Char('q')]),
        Flow::Quit,
        "q ends the run out of the help as well"
    );
}

#[test]
fn q_and_esc_quit_and_nothing_else_does() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    assert_eq!(press(&mut ui, &[KeyCode::Char('x')]), Flow::Continue);
    assert_eq!(ui.index(), 0);
    assert_eq!(press(&mut ui, &[KeyCode::Esc]), Flow::Quit);
    assert_eq!(press(&mut ui, &[KeyCode::Char('q')]), Flow::Quit);
}

#[test]
fn only_the_press_counts() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    let press = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert_eq!(ui.event(&press), Flow::Quit);

    // Windows reports the release as well. Counted, every key would arrive
    // twice.
    let release = Event::Key(KeyEvent {
        code: KeyCode::Char('q'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
    });
    assert_eq!(ui.event(&release), Flow::Continue);

    assert_eq!(ui.event(&Event::Resize(80, 24)), Flow::Continue);
}

#[test]
fn an_empty_repository_does_not_break_the_interface() {
    let analysis = analysis(QUIET, &[]);
    let mut ui = Ui::new(&analysis);

    assert!(ui.rows().is_empty());
    assert!(ui.selected().is_none());
    press(
        &mut ui,
        &[KeyCode::Left, KeyCode::Right, KeyCode::Down, KeyCode::Up],
    );
    assert_eq!(ui.index(), 0);

    assert!(screen(&ui).contains("holds nothing"));
}

#[test]
fn the_screen_shows_tree_node_and_count() {
    let analysis = nested();
    let ui = Ui::new(&analysis);
    let screen = screen(&ui);

    assert!(screen.contains("Tree"), "{screen}");
    assert!(screen.contains("Node"), "{screen}");
    assert!(screen.contains("README.md"), "{screen}");
    assert!(screen.contains("Coverage"), "{screen}");
    assert!(screen.contains("4 files, 2 gaps"), "{screen}");
}

#[test]
fn the_screen_shows_the_help_and_the_gaps() {
    let analysis = nested();
    let mut ui = Ui::new(&analysis);

    press(&mut ui, &[KeyCode::Char('?')]);
    let help = screen(&ui);
    assert!(help.contains("Keys"), "{help}");
    assert!(help.contains("quit"), "{help}");

    press(&mut ui, &[KeyCode::Char(' '), KeyCode::Char('f')]);
    let gaps = screen(&ui);
    assert!(gaps.contains("Gaps"), "{gaps}");
    assert!(gaps.contains("src/deep/a.rs"), "{gaps}");
}

#[test]
fn the_screen_also_shows_what_is_excused() {
    let analysis = analysis(QUIET, &["LICENSE", "README.md"]);
    let ui = Ui::new(&analysis);
    let screen = screen(&ui);

    assert!(screen.contains("LICENSE"), "{screen}");
}
