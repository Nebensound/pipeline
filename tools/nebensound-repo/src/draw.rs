//! What the interface looks like.
//!
//! The right-hand side is literally the same answer `explain` gives on the
//! command line. Writing it twice would mean keeping two answers to one
//! question.

use crate::annotate::Coverage;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};

use crate::report;
use crate::ui::{Row, Ui};

const KEYS: &str = "\
  j / k, down / up   select
  l / right, enter   unfold
  h / left           fold up, otherwise step out one level
  g                  to the next gap
  f                  show only the gaps
  ?                  this list
  q                  quit";

/// Draws the current state.
pub fn draw(frame: &mut Frame, ui: &Ui) {
    let [body, status] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(frame.area());

    if ui.help() {
        frame.render_widget(
            Paragraph::new(KEYS).block(Block::bordered().title(" Keys ")),
            body,
        );
    } else {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(body);

        let rows = ui.rows();
        let items: Vec<ListItem<'static>> =
            rows.iter().map(|row| item(row, ui.only_gaps())).collect();
        let title = if ui.only_gaps() { " Gaps " } else { " Tree " };
        let list = List::new(items)
            .block(Block::bordered().title(title))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        let mut state = ListState::default().with_selected(Some(ui.index()));
        frame.render_stateful_widget(list, left, &mut state);

        let detail = ui
            .selected()
            .and_then(|node| report::explain(ui.analysis(), &node.path))
            .unwrap_or_else(|| "The repository holds nothing.".to_owned());
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(" Node ")),
            right,
        );
    }

    frame.render_widget(Paragraph::new(status_line(ui)), status);
}

fn item(row: &Row<'_>, flat: bool) -> ListItem<'static> {
    let label = if flat {
        report::shown(&row.node.path.to_string())
    } else {
        // A node always has a last segment: the root is not one.
        report::shown(row.node.path.file_name().unwrap_or_default())
    };
    let indent = if flat {
        String::new()
    } else {
        "  ".repeat(row.depth)
    };

    let (marker, style) = match row.node.coverage() {
        None => (
            if row.open { "\u{25be} " } else { "\u{25b8} " },
            Style::new().fg(Color::Blue),
        ),
        Some(Coverage::Covered(_)) => ("\u{2713} ", Style::new().fg(Color::Green)),
        Some(Coverage::Uncovered(_)) => ("\u{d7} ", Style::new().fg(Color::Red)),
        Some(Coverage::Unknown) => ("? ", Style::new().fg(Color::Yellow)),
        Some(Coverage::Excluded { .. }) => ("\u{2013} ", Style::new().fg(Color::DarkGray)),
    };

    let suffix = if row.node.coverage().is_none() {
        "/"
    } else {
        ""
    };
    ListItem::new(Line::from(vec![
        Span::raw(indent),
        Span::styled(marker.to_owned(), style),
        Span::styled(format!("{label}{suffix}"), style),
    ]))
}

fn status_line(ui: &Ui) -> Line<'static> {
    let hint = if ui.help() {
        "any key closes this"
    } else if ui.only_gaps() {
        "f show all   ? keys   q quit"
    } else {
        "g next gap   f gaps only   ? keys   q quit"
    };
    Line::from(format!(" {}   {hint}", report::summary(ui.analysis())))
}
