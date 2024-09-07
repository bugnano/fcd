use std::rc::Rc;

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use crate::{
    app::{Events, PubSub},
    component::{Component, Focus},
    palette::Palette,
};

#[derive(Debug)]
pub struct ButtonBar {
    palette: Rc<Palette>,
    events_tx: Sender<Events>,
    labels: Vec<String>,
    rects: Rc<[Rect]>,
}

impl ButtonBar {
    pub fn new<T: IntoIterator<Item = U>, U: AsRef<str>>(
        palette: &Rc<Palette>,
        events_tx: &Sender<Events>,
        labels: T,
    ) -> ButtonBar {
        ButtonBar {
            palette: Rc::clone(palette),
            events_tx: events_tx.clone(),
            labels: labels
                .into_iter()
                .map(|label| String::from(label.as_ref()))
                .collect(),
            rects: Rc::new([]),
        }
    }
}

impl Component for ButtonBar {
    fn handle_mouse(&mut self, button: MouseButton, mouse_position: layout::Position) {
        if let MouseButton::Left = button {
            for (i, rect) in self.rects.iter().enumerate() {
                if rect.contains(mouse_position) {
                    self.events_tx
                        .send(Events::Input(Event::Key(Key::F(((i / 2) + 1) as u8))))
                        .unwrap();
                }
            }
        }
    }

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

        self.rects = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(&widths)
            .split(*chunk);

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
