use std::fmt;

use crate::component::Component;

pub trait CommandBar {
    fn is_focusable(&self) -> bool;
}

pub trait CommandBarComponent: Component + CommandBar {}

impl fmt::Debug for dyn CommandBarComponent + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dyn CommandBarComponent")
    }
}
