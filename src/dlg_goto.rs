use anyhow::Result;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow},
    component::Component,
    config::Config,
};

#[derive(Debug)]
pub struct DlgGoto {
    config: Config,
    label: String,
}

impl DlgGoto {
    pub fn new(config: &Config, label: &str) -> Result<DlgGoto> {
        Ok(DlgGoto {
            config: *config,
            label: String::from(label),
        })
    }
}

impl Component for DlgGoto {
    fn render(&mut self, f: &mut Frame, chunk: &Rect) {
        let area = centered_rect(30, 7, chunk);

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        let upper_inner = Rect::new(
            sections[0].x + 2,
            sections[0].y + 1,
            sections[0].width.saturating_sub(4),
            sections[0].height.saturating_sub(1),
        );

        let len_label = self.label.width() as u16;
        let upper_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(len_label),
                Constraint::Length(upper_inner.width.saturating_sub(len_label)),
            ])
            .split(upper_inner);

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    " Goto ",
                    Style::default().fg(self.config.dialog.title_fg),
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

        let label = Paragraph::new(Span::styled(
            &self.label,
            Style::default()
                .fg(self.config.dialog.fg)
                .bg(self.config.dialog.bg),
        ));

        let edit = Paragraph::new(Span::styled(
            "",
            Style::default()
                .fg(self.config.dialog.input_fg)
                .bg(self.config.dialog.input_bg),
        ))
        .block(
            Block::default().style(
                Style::default()
                    .fg(self.config.dialog.input_fg)
                    .bg(self.config.dialog.input_bg),
            ),
        );

        let txt_ok = "OK";
        let len_ok = txt_ok.width() as u16;
        let btn_ok = Paragraph::new(Line::from(vec![
            Span::styled(
                "[ ",
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            ),
            Span::styled(
                txt_ok,
                Style::default()
                    .fg(self.config.dialog.title_fg)
                    .bg(self.config.dialog.bg),
            ),
            Span::styled(
                " ]",
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            ),
        ]));

        let txt_cancel = "Cancel";
        let len_cancel = txt_cancel.width() as u16;
        let btn_cancel = Paragraph::new(Line::from(vec![
            Span::styled(
                "[ ",
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            ),
            Span::styled(
                txt_cancel,
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            ),
            Span::styled(
                " ]",
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            ),
        ]));

        let lower_inner = centered_rect(
            len_ok + 4 + 1 + len_cancel + 4,
            sections[1].height.saturating_sub(2),
            &sections[1],
        );

        let lower_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(len_ok + 4),
                Constraint::Length(1),
                Constraint::Length(len_cancel + 4),
                Constraint::Min(0),
            ])
            .split(lower_inner);

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(symbols::border::Set {
                top_left: symbols::line::NORMAL.vertical_right,
                top_right: symbols::line::NORMAL.vertical_left,
                ..symbols::border::PLAIN
            })
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

        f.render_widget(Clear, area);
        f.render_widget(
            Block::default().style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            ),
            area,
        );
        if self.config.ui.use_shadows {
            render_shadow(
                f,
                &area,
                &Style::default()
                    .bg(self.config.ui.shadow_bg)
                    .fg(self.config.ui.shadow_fg),
            );
        }

        f.render_widget(upper_block, sections[0]);
        f.render_widget(label, upper_area[0]);
        f.render_widget(edit, upper_area[1]);

        f.render_widget(lower_block, sections[1]);
        f.render_widget(btn_ok, lower_area[0]);
        f.render_widget(btn_cancel, lower_area[2]);
    }
}
