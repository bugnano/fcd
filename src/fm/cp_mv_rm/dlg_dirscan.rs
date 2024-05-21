use std::rc::Rc;

use crossbeam_channel::Sender;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use crate::{
    app::{centered_rect, render_shadow, PubSub},
    component::{Component, Focus},
    config::Config,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug, Copy, Clone)]
pub enum DirscanType {
    Cp,
    Mv,
    Rm,
}

#[derive(Debug)]
pub struct DlgDirscan {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    btn_abort: Button,
    btn_skip: Button,
    current: String,
    files: usize,
    total_size: Option<u64>,
    focus_position: usize,
}

impl DlgDirscan {
    pub fn new(config: &Rc<Config>, pubsub_tx: Sender<PubSub>) -> DlgDirscan {
        DlgDirscan {
            config: Rc::clone(config),
            pubsub_tx,
            btn_abort: Button::new(
                "Abort",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            btn_skip: Button::new(
                "Skip",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            current: String::from(""),
            files: 0,
            total_size: None,
            focus_position: 0,
        }
    }
}

impl Component for DlgDirscan {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
            }
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => todo!(),
                1 => todo!(),
                _ => unreachable!(),
            },
            Key::Left | Key::Char('h') => self.focus_position = 0,
            Key::Right | Key::Char('l') => self.focus_position = 1,
            Key::Ctrl('c') => key_handled = false,
            Key::Ctrl('l') => key_handled = false,
            Key::Ctrl('z') => key_handled = false,
            Key::Ctrl('o') => key_handled = false,
            _ => (),
        }

        key_handled
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect(chunk.width / 2, 9, chunk);

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

        let middle_border_set = symbols::border::Set {
            top_left: symbols::line::NORMAL.vertical_right,
            top_right: symbols::line::NORMAL.vertical_left,
            ..symbols::border::PLAIN
        };

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    tilde_layout(" Directory scanning ", sections[0].width as usize),
                    Style::default().fg(self.config.dialog.title_fg),
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

        let upper_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(upper_block.inner(sections[0]));

        let current = Paragraph::new(Span::raw(tilde_layout(
            &self.current,
            upper_area[0].width as usize,
        )));
        let files = Paragraph::new(Span::raw(tilde_layout(
            &format!("Files: {}", self.files),
            upper_area[1].width as usize,
        )));
        let total_size = Paragraph::new(Span::raw(tilde_layout(
            &format!(
                "Total size: {}",
                match self.total_size {
                    Some(size) => size.to_string(),
                    None => "n/a".to_string(),
                }
            ),
            upper_area[1].width as usize,
        )));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(current, upper_area[0]);
        f.render_widget(files, upper_area[1]);
        f.render_widget(total_size, upper_area[2]);

        // Lower section

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

        let lower_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.btn_abort.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_skip.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_abort.width() + 1 + self.btn_skip.width()) as u16,
                1,
                &lower_block.inner(sections[2]),
            ));

        f.render_widget(lower_block, sections[2]);
        self.btn_abort.render(
            f,
            &lower_area[0],
            match self.focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_skip.render(
            f,
            &lower_area[2],
            match self.focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
