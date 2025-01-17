use std::{cmp::max, rc::Rc};

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgQuestion {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    btn_yes: Button,
    btn_no: Button,
    title: String,
    question: String,
    on_yes: PubSub,
    focus_position: usize,
    btn_yes_rect: Rect,
    btn_no_rect: Rect,
}

impl DlgQuestion {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        title: &str,
        question: &str,
        on_yes: &PubSub,
    ) -> DlgQuestion {
        DlgQuestion {
            palette: Rc::clone(palette),
            pubsub_tx,
            btn_yes: Button::new(
                "Yes",
                &palette.error,
                &palette.error_focus,
                &palette.error_title,
            ),
            btn_no: Button::new(
                "No",
                &palette.error,
                &palette.error_focus,
                &palette.error_title,
            ),
            title: format!(" {} ", title),
            question: String::from(question),
            on_yes: on_yes.clone(),
            focus_position: 0,
            btn_yes_rect: Rect::default(),
            btn_no_rect: Rect::default(),
        }
    }
}

impl Component for DlgQuestion {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
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

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: Position) {
        if matches!(button, MouseButton::Left | MouseButton::Right) {
            if self.btn_yes_rect.contains(mouse_position) {
                self.focus_position = 0;

                if let MouseButton::Left = button {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    self.pubsub_tx.send(self.on_yes.clone()).unwrap();
                }
            }

            if self.btn_no_rect.contains(mouse_position) {
                self.focus_position = 1;

                if let MouseButton::Left = button {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
            }
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect(max(self.question.width() + 6, 21) as u16, 7, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.error), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

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
            .title_top(
                Line::from(Span::styled(
                    tilde_layout(&self.title, sections[0].width as usize),
                    self.palette.error_title,
                ))
                .centered(),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.error);

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
            .border_set(MIDDLE_BORDER_SET)
            .style(self.palette.error);

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

        self.btn_yes_rect = lower_area[0];
        self.btn_no_rect = lower_area[2];

        f.render_widget(lower_block, sections[1]);
        self.btn_yes.render(
            f,
            &self.btn_yes_rect,
            match self.focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_no.render(
            f,
            &self.btn_no_rect,
            match self.focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
