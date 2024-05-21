use std::{cmp::max, rc::Rc};

use crossbeam_channel::Sender;
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
    app::{centered_rect, render_shadow, PubSub},
    component::{Component, Focus},
    config::Config,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgQuestion {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    btn_yes: Button,
    btn_no: Button,
    title: String,
    question: String,
    on_yes: PubSub,
    focus_position: usize,
}

impl DlgQuestion {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        title: &str,
        question: &str,
        on_yes: &PubSub,
    ) -> DlgQuestion {
        DlgQuestion {
            config: Rc::clone(config),
            pubsub_tx,
            btn_yes: Button::new(
                "Yes",
                &Style::default().fg(config.error.fg).bg(config.error.bg),
                &Style::default()
                    .fg(config.error.focus_fg)
                    .bg(config.error.focus_bg),
                &Style::default()
                    .fg(config.error.title_fg)
                    .bg(config.error.bg),
            ),
            btn_no: Button::new(
                "No",
                &Style::default().fg(config.error.fg).bg(config.error.bg),
                &Style::default()
                    .fg(config.error.focus_fg)
                    .bg(config.error.focus_bg),
                &Style::default()
                    .fg(config.error.title_fg)
                    .bg(config.error.bg),
            ),
            title: format!(" {} ", title),
            question: String::from(question),
            on_yes: on_yes.clone(),
            focus_position: 0,
        }
    }
}

impl Component for DlgQuestion {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
            }
            Key::Char('\n') | Key::Char(' ') => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                match self.focus_position {
                    0 => self.pubsub_tx.send(self.on_yes.clone()).unwrap(),
                    1 => {}
                    _ => unreachable!(),
                }
            }
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
        let area = centered_rect(max(self.question.width() + 6, 21) as u16, 7, chunk);

        f.render_widget(Clear, area);
        f.render_widget(
            Block::default().style(
                Style::default()
                    .fg(self.config.error.fg)
                    .bg(self.config.error.bg),
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
            .constraints([Constraint::Length(2), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    tilde_layout(&self.title, sections[0].width as usize),
                    Style::default().fg(self.config.error.title_fg),
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .fg(self.config.error.fg)
                    .bg(self.config.error.bg),
            );

        let upper_area = upper_block.inner(sections[0]);

        let question = Paragraph::new(Span::raw(tilde_layout(
            &self.question,
            upper_area.width as usize,
        )));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(question, upper_area);

        // Lower section

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.error.fg)
                    .bg(self.config.error.bg),
            );

        let lower_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.btn_yes.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_no.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_yes.width() + 1 + self.btn_no.width()) as u16,
                1,
                &lower_block.inner(sections[1]),
            ));

        f.render_widget(lower_block, sections[1]);
        self.btn_yes.render(
            f,
            &lower_area[0],
            match self.focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_no.render(
            f,
            &lower_area[2],
            match self.focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
