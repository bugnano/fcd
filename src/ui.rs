use ratatui::{prelude::*, widgets::*};

use crate::component::Component;
use crate::text_viewer::TextViewer;

pub fn render_app<B: Backend>(f: &mut Frame<B>, component: &mut TextViewer) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(f.size());

    let block = Block::default()
        .title(Span::styled(
            "TODO: File name",
            Style::default().fg(Color::Black),
        ))
        .style(Style::default().bg(Color::Cyan));
    f.render_widget(block, chunks[0]);

    component.render(f, &chunks[1]);

    let block = Block::default()
        .title(Span::styled(
            "TODO: Bottom bar",
            Style::default().fg(Color::Black),
        ))
        .style(Style::default().bg(Color::Cyan));
    f.render_widget(block, chunks[2]);
}
