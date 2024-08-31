use std::rc::Rc;

use ratatui::{prelude::*, widgets::*};

use crate::{
    app::PubSub,
    component::{Component, Focus},
    palette::Palette,
};

#[derive(Debug)]
pub struct ButtonBar {
    palette: Rc<Palette>,
    labels: Vec<String>,
}

impl ButtonBar {
    pub fn new<T: IntoIterator<Item = U>, U: AsRef<str>>(
        palette: &Rc<Palette>,
        labels: T,
    ) -> ButtonBar {
        ButtonBar {
            palette: Rc::clone(palette),
            labels: labels
                .into_iter()
                .map(|label| String::from(label.as_ref()))
                .collect(),
        }
    }
}

impl Component for ButtonBar {
    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::ButtonLabels(labels) => self.labels.clone_from(labels),
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let label_width =
            (chunk.width.saturating_sub(2 * self.labels.len() as u16)) / (self.labels.len() as u16);

        let mut excess_width = chunk
            .width
            .saturating_sub((label_width + 2) * self.labels.len() as u16);

        let nth = match excess_width {
            0 => 0,
            w => self.labels.len() / (w as usize),
        };

        let widths: Vec<Constraint> = self
            .labels
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
            .collect();

        let items = Row::new(self.labels.iter().enumerate().flat_map(|(i, label)| {
            [
                Span::styled(format!("{:2}", i + 1), self.palette.hotkey),
                Span::styled(label, self.palette.selected),
            ]
        }));

        let table = Table::new([items], &widths)
            .block(Block::default().style(self.palette.selected))
            .column_spacing(0);

        f.render_widget(table, *chunk);
    }
}
