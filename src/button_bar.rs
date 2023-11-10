use anyhow::Result;
use ratatui::{prelude::*, widgets::*};

use crate::{component::Component, config::Config};

const LABELS: &[&str] = &[
    " ",      //"Help",
    "UnWrap", //
    "Quit",   //
    "Hex",    //"Ascii",
    "Goto",   //
    " ",      //
    "Search", //
    " ",      //"Raw",
    " ",      //"Format",
    "Quit",   //
];

#[derive(Debug)]
pub struct ButtonBar {
    config: Config,
    state: TableState,
}

impl ButtonBar {
    pub fn new(config: &Config) -> Result<ButtonBar> {
        Ok(ButtonBar {
            config: *config,
            state: TableState::default(),
        })
    }
}

impl Component for ButtonBar {
    fn render(&mut self, f: &mut Frame, chunk: &Rect) {
        let label_width = (chunk.width.saturating_sub(2 * LABELS.len() as u16)) / (LABELS.len() as u16);
        let mut excess_width = chunk.width.saturating_sub((label_width + 2) * LABELS.len() as u16);
        let nth = match excess_width {
            0 => 0,
            w => LABELS.len() / (w as usize),
        };

        let widths = LABELS
            .iter()
            .enumerate()
            .flat_map(|(i, _)| {
                [
                    Constraint::Length(2),
                    Constraint::Length(if nth == 0 || excess_width == 0 {
                        label_width
                    } else if i % nth == 0 {
                        excess_width -= 1;

                        label_width + 1
                    } else {
                        label_width
                    }),
                ]
            })
            .collect::<Vec<_>>();

        let items = Row::new(LABELS.iter().enumerate().flat_map(|(i, label)| {
            [
                Span::styled(
                    format!("{:2}", i + 1),
                    Style::default()
                        .fg(self.config.ui.hotkey_fg)
                        .bg(self.config.ui.hotkey_bg),
                ),
                Span::styled(
                    label.to_string(),
                    Style::default()
                        .fg(self.config.ui.selected_fg)
                        .bg(self.config.ui.selected_bg),
                ),
            ]
        }));

        let items = Table::new([items])
            .block(Block::default().style(Style::default().bg(self.config.ui.selected_bg)))
            .widths(&widths)
            .column_spacing(0);

        f.render_stateful_widget(items, *chunk, &mut self.state);
    }
}
