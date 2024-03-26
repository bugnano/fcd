use std::rc::Rc;

use anyhow::Result;
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
    widgets::{button::Button, check_box::CheckBox, input::Input, radio_box::RadioBox},
};

#[derive(Debug, Copy, Clone)]
pub enum SearchType {
    Normal,
    Regex,
    Wildcard,
}

#[derive(Debug, Clone)]
pub struct TextSearch {
    pub search_string: String,
    pub search_type: SearchType,
    pub case_sensitive: bool,
    pub backwards: bool,
    pub whole_words: bool,
}

#[derive(Debug)]
pub struct DlgTextSearch {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    input: Input,
    radio: RadioBox,
    check_boxes: Vec<CheckBox>,
    btn_ok: Button,
    btn_cancel: Button,
    section_focus_position: usize,
    middle_focus_position: usize,
    check_focus_position: usize,
    button_focus_position: usize,
}

impl DlgTextSearch {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        text_search: &TextSearch,
    ) -> Result<DlgTextSearch> {
        Ok(DlgTextSearch {
            config: Rc::clone(config),
            pubsub_tx,
            input: Input::new(
                &Style::default()
                    .fg(config.dialog.input_fg)
                    .bg(config.dialog.input_bg),
                "",
                0,
            )?,
            radio: RadioBox::new(
                ["Normal", "Regular expression", "Wildcard search"],
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                match text_search.search_type {
                    SearchType::Normal => 0,
                    SearchType::Regex => 1,
                    SearchType::Wildcard => 2,
                },
            )?,
            check_boxes: vec![
                CheckBox::new(
                    "Case sensitive",
                    &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                    &Style::default()
                        .fg(config.dialog.focus_fg)
                        .bg(config.dialog.focus_bg),
                    text_search.case_sensitive,
                )?,
                CheckBox::new(
                    "Backwards",
                    &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                    &Style::default()
                        .fg(config.dialog.focus_fg)
                        .bg(config.dialog.focus_bg),
                    text_search.backwards,
                )?,
                CheckBox::new(
                    "Whole words",
                    &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                    &Style::default()
                        .fg(config.dialog.focus_fg)
                        .bg(config.dialog.focus_bg),
                    text_search.whole_words,
                )?,
            ],
            btn_ok: Button::new(
                "OK",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            )?,
            btn_cancel: Button::new(
                "Cancel",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            )?,
            section_focus_position: 0,
            middle_focus_position: 0,
            check_focus_position: 0,
            button_focus_position: 0,
        })
    }
}

impl Component for DlgTextSearch {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        let input_handled = match self.section_focus_position {
            0 => self.input.handle_key(key)?,
            1 => match self.middle_focus_position {
                0 => self.radio.handle_key(key)?,
                1 => self.check_boxes[self.check_focus_position].handle_key(key)?,
                _ => unreachable!(),
            },
            2 => false,
            _ => unreachable!(),
        };

        if !input_handled {
            match key {
                Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
                Key::Char('\n') | Key::Char(' ') => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    if (self.section_focus_position == 0) || (self.button_focus_position == 0) {
                        self.pubsub_tx
                            .send(PubSub::TextSearch(TextSearch {
                                search_string: self.input.value(),
                                search_type: match self.radio.value() {
                                    0 => SearchType::Normal,
                                    1 => SearchType::Regex,
                                    2 => SearchType::Wildcard,
                                    _ => unreachable!(),
                                },
                                case_sensitive: self.check_boxes[0].value(),
                                backwards: self.check_boxes[1].value(),
                                whole_words: self.check_boxes[2].value(),
                            }))
                            .unwrap();
                    }
                }
                Key::BackTab => {
                    self.section_focus_position =
                        ((self.section_focus_position as isize) - 1).rem_euclid(3) as usize;
                }
                Key::Char('\t') => {
                    self.section_focus_position = (self.section_focus_position + 1) % 3;
                }
                Key::Up | Key::Char('k') => {
                    match (self.section_focus_position, self.middle_focus_position) {
                        (1, 1) => {
                            if self.check_focus_position > 0 {
                                self.check_focus_position -= 1;
                            } else {
                                self.section_focus_position -= 1;
                            }
                        }
                        _ => {
                            if self.section_focus_position > 0 {
                                self.section_focus_position -= 1;
                            }
                        }
                    }
                }
                Key::Down | Key::Char('j') => {
                    match (self.section_focus_position, self.middle_focus_position) {
                        (1, 1) => {
                            if (self.check_focus_position + 1) < self.check_boxes.len() {
                                self.check_focus_position += 1;
                            } else {
                                self.section_focus_position += 1;
                            }
                        }
                        _ => {
                            if (self.section_focus_position + 1) < 3 {
                                self.section_focus_position += 1;
                            }
                        }
                    }
                }
                Key::Left | Key::Char('h') => match self.section_focus_position {
                    0 => (),
                    1 => self.middle_focus_position = 0,
                    2 => self.button_focus_position = 0,
                    _ => unreachable!(),
                },
                Key::Right | Key::Char('l') => match self.section_focus_position {
                    0 => (),
                    1 => self.middle_focus_position = 1,
                    2 => self.button_focus_position = 1,
                    _ => unreachable!(),
                },
                Key::Ctrl('c') => key_handled = false,
                Key::Ctrl('l') => key_handled = false,
                Key::Ctrl('z') => key_handled = false,
                _ => (),
            }
        }

        Ok(key_handled)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect(58, 12, chunk);

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
            .constraints([
                Constraint::Length(3),
                Constraint::Length(4),
                Constraint::Length(3),
            ])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    " Search ",
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
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(upper_block.inner(sections[0]));

        let label = Paragraph::new(Span::raw("Enter search string:"));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(label, upper_area[0]);
        self.input.render(
            f,
            &upper_area[1],
            match self.section_focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        // Middle section

        let middle_block = Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_set(middle_border_set)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

        let middle_sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Min(1)])
            .split(middle_block.inner(sections[1]));

        let check_sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(1); self.check_boxes.len()])
            .split(middle_sections[1]);

        f.render_widget(middle_block, sections[1]);

        self.radio.render(
            f,
            &middle_sections[0],
            match (self.section_focus_position, self.middle_focus_position) {
                (1, 0) => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        self.check_boxes
            .iter_mut()
            .enumerate()
            .for_each(|(i, check_box)| {
                check_box.render(
                    f,
                    &check_sections[i],
                    if (self.section_focus_position == 1)
                        && (self.middle_focus_position == 1)
                        && (self.check_focus_position == i)
                    {
                        Focus::Focused
                    } else {
                        Focus::Normal
                    },
                );
            });

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
                Constraint::Length(self.btn_ok.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_cancel.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_ok.width() + 1 + self.btn_cancel.width()) as u16,
                1,
                &lower_block.inner(sections[2]),
            ));

        f.render_widget(lower_block, sections[2]);
        self.btn_ok.render(
            f,
            &lower_area[0],
            match (self.section_focus_position, self.button_focus_position) {
                (2, 0) => Focus::Focused,
                (_, 0) => Focus::Active,
                _ => Focus::Normal,
            },
        );
        self.btn_cancel.render(
            f,
            &lower_area[2],
            match (self.section_focus_position, self.button_focus_position) {
                (2, 1) => Focus::Focused,
                (_, 1) => Focus::Active,
                _ => Focus::Normal,
            },
        );
    }
}
