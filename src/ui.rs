use ratatui::{prelude::*, widgets::*};

pub fn render_app<B: Backend>(f: &mut Frame<B>, items: &[Row], state: &mut TableState) {
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

    let widths = [
        Constraint::Length((items.len().to_string().len() + 1) as u16),
        Constraint::Percentage(100),
    ];
    let items = Table::new(Vec::from(items))
        .block(Block::default().style(Style::default().bg(Color::Blue)))
        .widths(&widths)
        .column_spacing(0)
        .highlight_style(
            Style::default()
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        );

    // We can now render the item list
    f.render_stateful_widget(items, chunks[1], state);

    let block = Block::default()
        .title(Span::styled(
            "TODO: Bottom bar",
            Style::default().fg(Color::Black),
        ))
        .style(Style::default().bg(Color::Cyan));
    f.render_widget(block, chunks[2]);
}
