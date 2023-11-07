use anyhow::Result;
use ratatui::prelude::*;

use log::debug;

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
    pub highlight: Highlight,
}

pub fn load_config() -> Result<Config> {
    let config: Config = toml::from_str(include_str!("../config/fcv-config.toml"))?;

    debug!("{:?}", config);

    Ok(config)
}
