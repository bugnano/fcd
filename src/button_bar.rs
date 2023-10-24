use anyhow::Result;
use ratatui::{prelude::*, widgets::*};

use crate::component::Component;

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
    state: TableState,
}

impl ButtonBar {
    pub fn new() -> Result<ButtonBar> {
        Ok(ButtonBar {
            state: TableState::default(),
        })
    }
}

impl Component for ButtonBar {
    fn render(&mut self, f: &mut Frame, chunk: &Rect) {
        let label_width = (chunk.width - (2 * LABELS.len() as u16)) / (LABELS.len() as u16);
        let mut excess_width = chunk.width - ((label_width + 2) * LABELS.len() as u16);
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
                    Style::default().fg(Color::White).bg(Color::Black),
                ),
                Span::styled(
                    label.to_string(),
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ),
            ]
        }));

        let items = Table::new([items])
            .block(Block::default().style(Style::default().bg(Color::Cyan)))
            .widths(&widths)
            .column_spacing(0);

        f.render_stateful_widget(items, *chunk, &mut self.state);
    }
}
