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
    widgets::{button::Button, check_box::CheckBox, input::Input},
};

#[derive(Debug, Clone)]
pub struct HexSearch {
    pub search_string: String,
    pub hexadecimal: bool,
    pub backwards: bool,
}

#[derive(Debug)]
pub struct DlgHexSearch {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    input: Input,
    check_boxes: Vec<CheckBox>,
    btn_ok: Button,
    btn_cancel: Button,
    section_focus_position: usize,
    check_focus_position: usize,
    button_focus_position: usize,
}

impl DlgHexSearch {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        hex_search: &HexSearch,
    ) -> Result<DlgHexSearch> {
        Ok(DlgHexSearch {
            config: Rc::clone(config),
            pubsub_tx,
            input: Input::new(
                &Style::default()
                    .fg(config.dialog.input_fg)
                    .bg(config.dialog.input_bg),
            )?,
            check_boxes: vec![
                CheckBox::new(
                    "Hexadecimal",
                    &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                    &Style::default()
                        .fg(config.dialog.focus_fg)
                        .bg(config.dialog.focus_bg),
                    hex_search.hexadecimal,
                )?,
                CheckBox::new(
                    "Backwards",
                    &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                    &Style::default()
                        .fg(config.dialog.focus_fg)
                        .bg(config.dialog.focus_bg),
                    hex_search.backwards,
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
            check_focus_position: 0,
            button_focus_position: 0,
        })
    }
}

impl Component for DlgHexSearch {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        let input_handled = match self.section_focus_position {
            0 => self.input.handle_key(key)?,
            1 => self.check_boxes[self.check_focus_position].handle_key(key)?,
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
                            .send(PubSub::HexSearch(HexSearch {
                                search_string: self.input.value(),
                                hexadecimal: self.check_boxes[0].value(),
                                backwards: self.check_boxes[1].value(),
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
                    if self.section_focus_position > 0 {
                        self.section_focus_position -= 1;
                    }
                }
                Key::Down | Key::Char('j') => {
                    if (self.section_focus_position + 1) < 3 {
                        self.section_focus_position += 1;
                    }
                }
                Key::Left | Key::Char('h') => match self.section_focus_position {
                    0 => (),
                    1 => self.check_focus_position = 0,
                    2 => self.button_focus_position = 0,
                    _ => unreachable!(),
                },
                Key::Right | Key::Char('l') => match self.section_focus_position {
                    0 => (),
                    1 => self.check_focus_position = 1,
                    2 => self.button_focus_position = 1,
                    _ => unreachable!(),
                },
                Key::Char(_) | Key::F(_) => (),
                _ => key_handled = false,
            }
        }

        Ok(key_handled)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect(58, 10, chunk);

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
                Constraint::Length(2),
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

        let check_sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Min(1)])
            .split(middle_block.inner(sections[1]));

        f.render_widget(middle_block, sections[1]);

        self.check_boxes
            .iter_mut()
            .enumerate()
            .for_each(|(i, check_box)| {
                check_box.render(
                    f,
                    &check_sections[i],
                    if (self.section_focus_position == 1) && (self.check_focus_position == i) {
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
