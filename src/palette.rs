use ratatui::prelude::*;

use uzers::get_effective_uid;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct Palette {
    pub hotkey: Style,
    pub selected: Style,
    pub selected_bg: Style,
    pub marked: Style,
    pub markselect: Style,
    pub shadow: Option<Style>,
    pub cmdbar_error: Style,

    pub panel: Style,
    pub panel_reverse: Style,

    pub error: Style,
    pub error_title: Style,
    pub error_focus: Style,

    pub dialog: Style,
    pub dialog_title: Style,
    pub dialog_focus: Style,
    pub dialog_input: Style,

    pub directory: Style,
    pub dir_symlink: Style,
    pub executable: Style,
    pub symlink: Style,
    pub stalelink: Style,
    pub device: Style,
    pub special: Style,
    pub archive: Style,

    pub lineno: Style,
    pub hex_even: Style,
    pub hex_odd: Style,
    pub hex_text_even: Style,
    pub hex_text_odd: Style,
    pub base00: Style,
    pub base03: Style,
    pub base05: Style,
    pub base08: Style,
    pub base09: Style,
    pub base0a: Style,
    pub base0b: Style,
    pub base0c: Style,
    pub base0d: Style,
    pub base0e: Style,
    pub base0f: Style,
}

pub fn get_palette(config: &Config) -> Palette {
    let uid = get_effective_uid();

    Palette {
        hotkey: Style::default()
            .fg(config.ui.hotkey_fg)
            .bg(config.ui.hotkey_bg),
        selected: match uid {
            0 => Style::default()
                .fg(config.ui.selected_fg_root)
                .bg(config.ui.selected_bg_root),
            _ => Style::default()
                .fg(config.ui.selected_fg_user)
                .bg(config.ui.selected_bg_user),
        },
        selected_bg: match uid {
            0 => Style::default().bg(config.ui.selected_bg_root),
            _ => Style::default().bg(config.ui.selected_bg_user),
        },
        marked: Style::default().fg(config.ui.marked_fg),
        markselect: match uid {
            0 => Style::default()
                .fg(config.ui.markselect_fg_root)
                .bg(config.ui.selected_bg_root),
            _ => Style::default()
                .fg(config.ui.markselect_fg_user)
                .bg(config.ui.selected_bg_user),
        },
        shadow: match config.options.use_shadows {
            true => Some(
                Style::default()
                    .fg(config.ui.shadow_fg)
                    .bg(config.ui.shadow_bg),
            ),
            false => None,
        },
        cmdbar_error: Style::default()
            .fg(config.ui.error_fg)
            .bg(config.ui.error_bg),

        panel: Style::default().fg(config.panel.fg).bg(config.panel.bg),
        panel_reverse: Style::default()
            .fg(config.panel.reverse_fg)
            .bg(config.panel.reverse_bg),

        error: Style::default().fg(config.error.fg).bg(config.error.bg),
        error_title: Style::default()
            .fg(config.error.title_fg)
            .bg(config.error.bg),
        error_focus: Style::default()
            .fg(config.error.focus_fg)
            .bg(config.error.focus_bg),

        dialog: Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
        dialog_title: Style::default()
            .fg(config.dialog.title_fg)
            .bg(config.dialog.bg),
        dialog_focus: Style::default()
            .fg(config.dialog.focus_fg)
            .bg(config.dialog.focus_bg),
        dialog_input: Style::default()
            .fg(config.dialog.input_fg)
            .bg(config.dialog.input_bg),

        directory: Style::default().fg(config.file_manager.directory_fg),
        dir_symlink: Style::default().fg(config.file_manager.dir_symlink_fg),
        executable: Style::default().fg(config.file_manager.executable_fg),
        symlink: Style::default().fg(config.file_manager.symlink_fg),
        stalelink: Style::default().fg(config.file_manager.stalelink_fg),
        device: Style::default().fg(config.file_manager.device_fg),
        special: Style::default().fg(config.file_manager.special_fg),
        archive: Style::default().fg(config.file_manager.archive_fg),

        lineno: Style::default().fg(config.viewer.lineno_fg),
        hex_even: Style::default().fg(config.viewer.hex_even_fg),
        hex_odd: Style::default().fg(config.viewer.hex_odd_fg),
        hex_text_even: Style::default().fg(config.viewer.hex_text_even_fg),
        hex_text_odd: Style::default().fg(config.viewer.hex_text_odd_fg),

        base00: Style::default().fg(config.highlight.base00),
        base03: Style::default().fg(config.highlight.base03),
        base05: Style::default().fg(config.highlight.base05),
        base08: Style::default().fg(config.highlight.base08),
        base09: Style::default().fg(config.highlight.base09),
        base0a: Style::default().fg(config.highlight.base0a),
        base0b: Style::default().fg(config.highlight.base0b),
        base0c: Style::default().fg(config.highlight.base0c),
        base0d: Style::default().fg(config.highlight.base0d),
        base0e: Style::default().fg(config.highlight.base0e),
        base0f: Style::default().fg(config.highlight.base0f),
    }
}

pub fn get_monochrome_palette() -> Palette {
    Palette {
        hotkey: Style::default().remove_modifier(Modifier::REVERSED),
        selected: Style::default().add_modifier(Modifier::REVERSED),
        selected_bg: Style::default().add_modifier(Modifier::REVERSED),
        marked: Style::default().add_modifier(Modifier::BOLD),
        markselect: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        shadow: None,
        cmdbar_error: Style::default().add_modifier(Modifier::BOLD),

        panel: Style::default(),
        panel_reverse: Style::default().add_modifier(Modifier::REVERSED),

        error: Style::default().add_modifier(Modifier::REVERSED),
        error_title: Style::default().add_modifier(Modifier::REVERSED),
        error_focus: Style::default().remove_modifier(Modifier::REVERSED),

        dialog: Style::default().add_modifier(Modifier::REVERSED),
        dialog_title: Style::default().add_modifier(Modifier::REVERSED),
        dialog_focus: Style::default().remove_modifier(Modifier::REVERSED),
        dialog_input: Style::default().remove_modifier(Modifier::REVERSED),

        directory: Style::default(),
        dir_symlink: Style::default(),
        executable: Style::default(),
        symlink: Style::default(),
        stalelink: Style::default(),
        device: Style::default(),
        special: Style::default(),
        archive: Style::default(),

        lineno: Style::default().add_modifier(Modifier::BOLD),
        hex_even: Style::default(),
        hex_odd: Style::default(),
        hex_text_even: Style::default(),
        hex_text_odd: Style::default(),

        base00: Style::default(),
        base03: Style::default(),
        base05: Style::default(),
        base08: Style::default(),
        base09: Style::default(),
        base0a: Style::default(),
        base0b: Style::default(),
        base0c: Style::default(),
        base0d: Style::default(),
        base0e: Style::default(),
        base0f: Style::default(),
    }
}
