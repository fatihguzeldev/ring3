use super::{CATEGORIES, Category};
use crate::ConfigError;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CategorySelection {
    #[default]
    All,
    Only(Vec<Category>),
}

impl CategorySelection {
    pub fn from_wire(value: &str) -> Result<Self, ConfigError> {
        let selection = match value {
            "" => Self::All,
            "-" => Self::Only(Vec::new()),
            _ => Self::Only(
                value
                    .split('\n')
                    .map(|id| Category::from_id(id).ok_or(ConfigError::Categories))
                    .collect::<Result<_, _>>()?,
            ),
        };
        selection.validate()?;
        Ok(selection)
    }

    pub fn allows(&self, category: Category) -> bool {
        if !category.descriptor().selectable {
            return true;
        }
        match self {
            Self::All => true,
            Self::Only(selected) => selected.iter().any(|parent| category.is_within(*parent)),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if let Self::Only(selected) = self
            && (selected.len() > CATEGORIES.len()
                || selected.iter().enumerate().any(|(index, category)| {
                    !category.descriptor().selectable || selected[..index].contains(category)
                }))
        {
            return Err(ConfigError::Categories);
        }
        Ok(())
    }
}
