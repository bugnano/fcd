use std::cmp::max;

use ratatui::prelude::*;

use unicode_normalization::UnicodeNormalization;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn tilde_layout(text: &str, max_width: usize) -> String {
    if text.width() <= max_width {
        return String::from(text);
    }

    let norm_text: String = text.nfkc().collect();

    let full_width = max(max_width.saturating_sub(1), 2);
    let max_left = full_width / 2;
    let max_right = full_width - max_left;

    let mut left_width = 0;
    let left_str: String = norm_text
        .chars()
        .take_while(|c| {
            let width = c.width().unwrap_or(0);

            if (left_width + width) <= max_left {
                left_width += width;

                true
            } else {
                false
            }
        })
        .collect();

    let mut right_width = 0;
    let right_str: String = norm_text
        .chars()
        .rev()
        .take_while(|c| {
            let width = c.width().unwrap_or(0);

            if (right_width + width) <= max_right {
                right_width += width;

                true
            } else {
                false
            }
        })
        .collect::<Vec<char>>()
        .iter()
        .rev()
        .collect();

    let mut final_width = 0;
    left_str
        .chars()
        .chain(std::iter::once('~'))
        .chain(right_str.chars())
        .take_while(|c| {
            let width = c.width().unwrap_or(0);

            if (final_width + width) <= max_width {
                final_width += width;

                true
            } else {
                false
            }
        })
        .collect()
}

pub fn tilde_layout_styled(
    styled_text: &[(String, Style)],
    max_width: usize,
) -> Vec<(String, Style)> {
    if styled_text
        .iter()
        .map(|(text, _style)| text.width())
        .sum::<usize>()
        <= max_width
    {
        return Vec::from(styled_text);
    }

    let norm_text: Vec<(String, Style)> = styled_text
        .iter()
        .map(|(text, style)| (text.nfkc().collect(), *style))
        .collect();

    let full_width = max(max_width.saturating_sub(1), 2);
    let max_left = full_width / 2;
    let max_right = full_width - max_left;

    let mut left_width = 0;
    let left_str: Vec<(String, Style)> = norm_text
        .iter()
        .map_while(|(text, style)| {
            if left_width >= max_left {
                return None;
            }

            let width = text.width();

            if (left_width + width) <= max_left {
                left_width += width;

                return Some((String::from(text), *style));
            }

            Some((
                text.chars()
                    .take_while(|c| {
                        let width = c.width().unwrap_or(0);

                        if (left_width + width) <= max_left {
                            left_width += width;

                            true
                        } else {
                            false
                        }
                    })
                    .collect(),
                *style,
            ))
        })
        .collect();

    let mut right_width = 0;
    let right_str: Vec<(String, Style)> = norm_text
        .iter()
        .rev()
        .map_while(|(text, style)| {
            if right_width >= max_right {
                return None;
            }

            let width = text.width();

            if (right_width + width) <= max_right {
                right_width += width;

                return Some((text.chars().rev().collect(), *style));
            }

            Some((
                text.chars()
                    .rev()
                    .take_while(|c| {
                        let width = c.width().unwrap_or(0);

                        if (right_width + width) <= max_right {
                            right_width += width;

                            true
                        } else {
                            false
                        }
                    })
                    .collect(),
                *style,
            ))
        })
        .collect::<Vec<(Vec<char>, Style)>>()
        .iter()
        .rev()
        .map(|(text, style)| (text.iter().rev().collect(), *style))
        .collect();

    let tilde_style = match (left_str.is_empty(), right_str.is_empty()) {
        (false, _) => left_str[left_str.len() - 1].1,
        (_, false) => right_str[0].1,
        _ => Style::default(),
    };

    let mut final_width = 0;
    left_str
        .iter()
        .chain(std::iter::once(&(String::from("~"), tilde_style)))
        .chain(right_str.iter())
        .map_while(|(text, style)| {
            if final_width >= max_width {
                return None;
            }

            let width = text.width();

            if (final_width + width) <= max_width {
                final_width += width;

                return Some((String::from(text), *style));
            }

            Some((
                text.chars()
                    .take_while(|c| {
                        let width = c.width().unwrap_or(0);

                        if (final_width + width) <= max_width {
                            final_width += width;

                            true
                        } else {
                            false
                        }
                    })
                    .collect(),
                *style,
            ))
        })
        .collect()
}
