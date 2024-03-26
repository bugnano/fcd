use std::{
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    component::{Component, Focus},
    fm::entry::Entry,
};

pub trait Panel {
    fn change_focus(&mut self, _focus: Focus) {}

    fn get_selected_entry(&self) -> Option<Entry> {
        None
    }

    fn get_cwd(&self) -> Option<PathBuf> {
        None
    }

    fn chdir(&mut self, _cwd: &Path) {}
}

impl fmt::Debug for dyn Panel + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dyn Panel")
    }
}

pub trait PanelComponent: Component + Panel {}

impl fmt::Debug for dyn PanelComponent + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dyn PanelComponent")
    }
}
