use anyhow::Result;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow},
    component::Component,
    config::Config,
    widgets::{button, input::Input},
};

#[derive(Debug)]
pub struct DlgGoto {
    config: Config,
    label: String,
    input: Input,
    section_focus_position: u16,
    button_focus_position: u16,
}

impl DlgGoto {
    pub fn new(config: &Config, label: &str) -> Result<DlgGoto> {
        Ok(DlgGoto {
            config: *config,
            label: String::from(label),
            input: Input::new(
                &Style::default()
                    .fg(config.dialog.input_fg)
                    .bg(config.dialog.input_bg),
                true,
            )?,
            section_focus_position: 0,
            button_focus_position: 0,
        })
    }
}

impl Component for DlgGoto {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        let input_handled = if self.section_focus_position == 0 {
            self.input.handle_key(key)?
        } else {
            false
        };

        if !input_handled {
            match key {
                Key::Up | Key::Char('k') => {
                    self.section_focus_position = 0;
                    self.input.focused = true;
                }
                Key::Down | Key::Char('j') => {
                    self.section_focus_position = 1;
                    self.input.focused = false;
                }
                Key::Left | Key::Char('h') => {
                    if self.section_focus_position == 1 {
                        self.button_focus_position = 0;
                    }
                }
                Key::Right | Key::Char('l') => {
                    if self.section_focus_position == 1 {
                        self.button_focus_position = 1;
                    }
                }
                _ => key_handled = false,
            }
        }

        Ok(key_handled)
    }

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

        let (style_btn_selected, style_btn_selected_text, style_btn) =
            if self.section_focus_position == 1 {
                (
                    Style::default()
                        .fg(self.config.dialog.focus_fg)
                        .bg(self.config.dialog.focus_bg),
                    Style::default()
                        .fg(self.config.dialog.focus_fg)
                        .bg(self.config.dialog.focus_bg),
                    Style::default()
                        .fg(self.config.dialog.fg)
                        .bg(self.config.dialog.bg),
                )
            } else {
                (
                    Style::default()
                        .fg(self.config.dialog.fg)
                        .bg(self.config.dialog.bg),
                    Style::default()
                        .fg(self.config.dialog.title_fg)
                        .bg(self.config.dialog.bg),
                    Style::default()
                        .fg(self.config.dialog.fg)
                        .bg(self.config.dialog.bg),
                )
            };

        let (style_ok, style_ok_text, focus_ok) = if self.button_focus_position == 0 {
            (
                style_btn_selected,
                style_btn_selected_text,
                self.section_focus_position == 1,
            )
        } else {
            (style_btn, style_btn, false)
        };

        let txt_ok = "OK";
        let len_ok = txt_ok.width() as u16;

        let (style_cancel, style_cancel_text, focus_cancel) = if self.button_focus_position == 1 {
            (
                style_btn_selected,
                style_btn_selected_text,
                self.section_focus_position == 1,
            )
        } else {
            (style_btn, style_btn, false)
        };

        let txt_cancel = "Cancel";
        let len_cancel = txt_cancel.width() as u16;

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
        self.input.render(f, &upper_area[1]);

        f.render_widget(lower_block, sections[1]);
        button::render(
            f,
            &lower_area[0],
            txt_ok,
            &style_ok,
            &style_ok_text,
            focus_ok,
        );
        button::render(
            f,
            &lower_area[2],
            txt_cancel,
            &style_cancel,
            &style_cancel_text,
            focus_cancel,
        );
    }
}
