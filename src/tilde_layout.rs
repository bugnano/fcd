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

pub fn tilde_layout_styled(text: &[(String, Style)], max_width: usize) -> Vec<(String, Style)> {
    // TODO
    Vec::from(text)
}
