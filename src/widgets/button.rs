use ratatui::{prelude::*, widgets::*};

pub fn render(
    f: &mut Frame,
    chunk: &Rect,
    label: &str,
    style: &Style,
    style_text: &Style,
    focused: bool,
) {
    let button = Paragraph::new(Line::from(vec![
        Span::styled("[ ", *style),
        Span::styled(label, *style_text),
        Span::styled(" ]", *style),
    ]));

    f.render_widget(button, *chunk);

    if focused && chunk.width > 2 {
        f.set_cursor(chunk.x + 2, chunk.y);
    }
}
