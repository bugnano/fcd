use std::fs;

use anyhow::{Context, Result};
use ratatui::prelude::*;

use clap::crate_name;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Options {
    pub opener: String,
    pub pager: String,
    pub editor: String,

    pub show_button_bar: bool,
    pub use_shadows: bool,
    pub use_internal_viewer: bool,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Ui {
    pub hotkey_fg: Color,
    pub hotkey_bg: Color,
    pub selected_fg: Color,
    pub selected_bg: Color,

    pub marked_fg: Color,
    pub markselect_fg: Color,

    pub shadow_fg: Color,
    pub shadow_bg: Color,

    pub error_fg: Color,
    pub error_bg: Color,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Panel {
    pub fg: Color,
    pub bg: Color,
    pub reverse_fg: Color,
    pub reverse_bg: Color,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Error {
    pub fg: Color,
    pub bg: Color,
    pub title_fg: Color,
    pub focus_fg: Color,
    pub focus_bg: Color,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Dialog {
    pub fg: Color,
    pub bg: Color,
    pub title_fg: Color,
    pub focus_fg: Color,
    pub focus_bg: Color,
    pub placeholder_fg: Color,
    pub input_fg: Color,
    pub input_bg: Color,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct FileManager {
    pub directory_fg: Color,
    pub dir_symlink_fg: Color,
    pub executable_fg: Color,
    pub symlink_fg: Color,
    pub stalelink_fg: Color,
    pub device_fg: Color,
    pub special_fg: Color,
    pub archive_fg: Color,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Viewer {
    pub tab_size: u8,

    pub lineno_fg: Color,
    pub hex_even_fg: Color,
    pub hex_odd_fg: Color,
    pub hex_text_even_fg: Color,
    pub hex_text_odd_fg: Color,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Highlight {
    pub base00: Color,
    pub base03: Color,
    pub base05: Color,
    pub base08: Color,
    pub base09: Color,
    #[serde(rename = "base0A")]
    pub base0a: Color,
    #[serde(rename = "base0B")]
    pub base0b: Color,
    #[serde(rename = "base0C")]
    pub base0c: Color,
    #[serde(rename = "base0D")]
    pub base0d: Color,
    #[serde(rename = "base0E")]
    pub base0e: Color,
    #[serde(rename = "base0F")]
    pub base0f: Color,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub options: Options,
    pub ui: Ui,
    pub panel: Panel,
    pub error: Error,
    pub dialog: Dialog,
    pub file_manager: FileManager,
    pub viewer: Viewer,
    pub highlight: Highlight,
}

pub fn load_config() -> Result<Config> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix(crate_name!())
        .context("Config: failed to create directory")?;
    let config_file_name = format!("{}-config.toml", crate_name!());

    if let Some(filename) = xdg_dirs.find_config_file(&config_file_name) {
        Ok(toml::from_str(
            &fs::read_to_string(filename).context("Config: failed to read config file")?,
        )
        .context("Config: failed to parse config file")?)
    } else {
        let default_config_str = include_str!("../config/config.toml");
        let default_config: Config =
            toml::from_str(default_config_str).context("Config: failed to parse config file")?;

        if let Ok(filename) = xdg_dirs.place_config_file(&config_file_name) {
            fs::write(filename, default_config_str)
                .context("Config: failed to write config file")?;
        }

        Ok(default_config)
    }
}
