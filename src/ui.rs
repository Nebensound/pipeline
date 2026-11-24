//! The interface as state: what is visible, what is selected, and what a key
//! changes about it.
//!
//! Nothing here draws and nothing here reads a key from the terminal. The state
//! is a value, and that is why every key press can be checked without a
//! terminal being involved.

use std::collections::BTreeSet;

use crate::Analysis;
use crate::annotate::Node;
use crate::path::RelativePath;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};

/// Whether the interface keeps running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// It keeps running.
    Continue,
    /// It is done.
    Quit,
}

/// One visible line.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    /// The node it is about.
    pub node: &'a Node,
    /// How deep it lies, measured from the topmost visible level.
    pub depth: usize,
    /// Whether it is unfolded.
    pub open: bool,
}

/// What the interface is showing.
#[derive(Debug, Clone)]
pub struct Ui<'a> {
    analysis: &'a Analysis,
    open: BTreeSet<RelativePath>,
    selected: usize,
    only_gaps: bool,
    help: bool,
}

impl<'a> Ui<'a> {
    /// The interface for this analysis.
    ///
    /// Unfolded is the way to every gap: what is missing should be visible
    /// without looking for it.
    #[must_use]
    pub fn new(analysis: &'a Analysis) -> Self {
        let mut open = BTreeSet::new();
        for (node, _) in analysis.tree.gaps() {
            reveal(&node.path, &mut open);
        }
        Self {
            analysis,
            open,
            selected: 0,
            only_gaps: false,
            help: false,
        }
    }

    /// The analysis it is showing.
    #[must_use]
    pub fn analysis(&self) -> &'a Analysis {
        self.analysis
    }

    /// Whether only the gaps are shown right now.
    #[must_use]
    pub fn only_gaps(&self) -> bool {
        self.only_gaps
    }

    /// Whether the help is open right now.
    #[must_use]
    pub fn help(&self) -> bool {
        self.help
    }

    /// Which line is selected.
    #[must_use]
    pub fn index(&self) -> usize {
        self.selected
    }

    /// The visible lines.
    #[must_use]
    pub fn rows(&self) -> Vec<Row<'a>> {
        if self.only_gaps {
            return self
                .analysis
                .tree
                .gaps()
                .into_iter()
                .map(|(node, _)| Row {
                    node,
                    depth: 0,
                    open: false,
                })
                .collect();
        }
        let mut rows = Vec::new();
        collect(self.analysis.tree.nodes(), 0, &self.open, &mut rows);
        rows
    }

    /// The selected node, when there is one.
    #[must_use]
    pub fn selected(&self) -> Option<&'a Node> {
        self.rows().get(self.selected).map(|row| row.node)
    }

    /// What an event from the terminal changes.
    ///
    /// Only the press counts, not the release: Windows reports both, and a key
    /// would otherwise arrive twice. Everything else — a new size, the mouse —
    /// leaves the state as it is.
    pub fn event(&mut self, event: &Event) -> Flow {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key.code),
            _ => Flow::Continue,
        }
    }

    /// What one key press changes.
    pub fn key(&mut self, key: KeyCode) -> Flow {
        if self.help {
            // The help closes on any key; q still ends the run.
            self.help = false;
            return if key == KeyCode::Char('q') {
                Flow::Quit
            } else {
                Flow::Continue
            };
        }
        match key {
            KeyCode::Char('q') | KeyCode::Esc => return Flow::Quit,
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.open_here(),
            KeyCode::Char('h') | KeyCode::Left => self.close_here(),
            KeyCode::Char('g') => self.next_gap(),
            KeyCode::Char('f') => self.toggle_filter(),
            KeyCode::Char('?') => self.help = true,
            _ => {}
        }
        Flow::Continue
    }

    fn step(&mut self, delta: isize) {
        let last = self.rows().len().saturating_sub(1);
        let moved = self.selected.saturating_add_signed(delta);
        self.selected = moved.min(last);
    }

    fn open_here(&mut self) {
        if let Some(row) = self.rows().get(self.selected)
            && !row.node.children().is_empty()
        {
            self.open.insert(row.node.path.clone());
        }
    }

    /// Fold up — and when there is nothing to fold here, step out one level.
    fn close_here(&mut self) {
        let rows = self.rows();
        let Some(row) = rows.get(self.selected) else {
            return;
        };
        if row.open {
            self.open.remove(&row.node.path);
            return;
        }
        let depth = row.depth;
        if let Some(parent) = rows[..self.selected]
            .iter()
            .rposition(|above| above.depth < depth)
        {
            self.selected = parent;
        }
    }

    /// To the next gap, unfolding the way there.
    fn next_gap(&mut self) {
        let gaps: Vec<RelativePath> = self
            .analysis
            .tree
            .gaps()
            .into_iter()
            .map(|(node, _)| node.path.clone())
            .collect();
        let here = self.selected().map(|node| node.path.clone());
        let next = gaps
            .iter()
            .find(|gap| here.as_ref().is_none_or(|path| *gap > path))
            .or_else(|| gaps.first());
        let Some(next) = next.cloned() else {
            return;
        };
        reveal(&next, &mut self.open);
        // After unfolding, the line is there. Were it not, the selection stays
        // where it was: jumping is no reason to lose anything.
        let found = self.rows().iter().position(|row| row.node.path == next);
        self.selected = found.unwrap_or(self.selected);
    }

    fn toggle_filter(&mut self) {
        self.only_gaps = !self.only_gaps;
        self.step(0);
    }
}

/// Unfolds the way to a path — the folders above it, not the path itself.
fn reveal(path: &RelativePath, open: &mut BTreeSet<RelativePath>) {
    let mut walked = RelativePath::root();
    for segment in path.segments().iter().rev().skip(1).rev() {
        walked = walked.join(segment);
        open.insert(walked.clone());
    }
}

fn collect<'a>(
    nodes: &'a [Node],
    depth: usize,
    open: &BTreeSet<RelativePath>,
    rows: &mut Vec<Row<'a>>,
) {
    for node in nodes {
        let is_open = open.contains(&node.path);
        rows.push(Row {
            node,
            depth,
            open: is_open,
        });
        if is_open {
            collect(node.children(), depth + 1, open, rows);
        }
    }
}
