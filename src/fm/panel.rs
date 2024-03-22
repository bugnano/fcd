use std::{fmt, path::PathBuf};

use crate::{component::Component, fm::entry::Entry};

pub trait Panel {
    fn get_selected_file(&self) -> Option<PathBuf> {
        None
    }

    fn get_selected_entry(&self) -> Option<Entry> {
        None
    }

    fn get_cwd(&self) -> Option<PathBuf> {
        None
    }
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
