use std::{fmt, path::PathBuf};

use crate::component::Component;

pub trait Panel {
    fn get_selected_file(&self) -> Option<PathBuf> {
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
