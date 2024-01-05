use std::fs;

use anyhow::{Context, Result};
use ratatui::prelude::*;

use clap::crate_name;
use serde::Deserialize;
use serde_with::{serde_as, DisplayFromStr};

#[serde_as]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Ui {
    #[serde_as(as = "DisplayFromStr")]
    pub hotkey_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub hotkey_bg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub selected_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub selected_bg: Color,

    #[serde_as(as = "DisplayFromStr")]
    pub marked_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub markselect_fg: Color,

    pub use_shadows: bool,

    #[serde_as(as = "DisplayFromStr")]
    pub shadow_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub shadow_bg: Color,
}

#[serde_as]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Panel {
    #[serde_as(as = "DisplayFromStr")]
    pub fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub bg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub reverse_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub reverse_bg: Color,
}

#[serde_as]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Error {
    #[serde_as(as = "DisplayFromStr")]
    pub fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub bg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub title_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub focus_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub focus_bg: Color,
}

#[serde_as]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Dialog {
    #[serde_as(as = "DisplayFromStr")]
    pub fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub bg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub title_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub focus_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub focus_bg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub placeholder_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub input_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub input_bg: Color,
}

#[serde_as]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Viewer {
    pub tab_size: u8,

    #[serde_as(as = "DisplayFromStr")]
    pub lineno_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub hex_even_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub hex_odd_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub hex_text_even_fg: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub hex_text_odd_fg: Color,
}

#[serde_as]
#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Highlight {
    #[serde_as(as = "DisplayFromStr")]
    pub base00: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub base03: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub base05: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub base08: Color,
    #[serde_as(as = "DisplayFromStr")]
    pub base09: Color,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "base0A")]
    pub base0a: Color,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "base0B")]
    pub base0b: Color,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "base0C")]
    pub base0c: Color,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "base0D")]
    pub base0d: Color,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "base0E")]
    pub base0e: Color,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "base0F")]
    pub base0f: Color,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct Config {
    pub ui: Ui,
    pub panel: Panel,
    pub error: Error,
    pub dialog: Dialog,
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
