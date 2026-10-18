use crate::{Category, CategorySelection};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Level {
    #[default]
    Off = 0,
    Basic = 1,
    Detailed = 2,
}

impl TryFrom<u32> for Level {
    type Error = ConfigError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Off),
            1 => Ok(Self::Basic),
            2 => Ok(Self::Detailed),
            _ => Err(ConfigError::Level),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub level: Level,
    pub categories: CategorySelection,
    pub max_buffer_bytes: usize,
    pub producer_sequence_id: u32,
    pub track_uuid: u64,
    pub track_name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            level: Level::Off,
            categories: CategorySelection::All,
            max_buffer_bytes: 1024 * 1024,
            producer_sequence_id: 1,
            track_uuid: 0x1_0000_0001,
            track_name: "ring3 engine".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Level,
    Buffer,
    Sequence,
    Track,
    Categories,
}

impl Config {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if !(4096..=64 * 1024 * 1024).contains(&self.max_buffer_bytes) {
            return Err(ConfigError::Buffer);
        }
        if self.producer_sequence_id == 0 {
            return Err(ConfigError::Sequence);
        }
        if self.track_uuid == 0
            || self.track_uuid as u32 > u32::MAX - 256
            || self.track_name.len() > 256
        {
            return Err(ConfigError::Track);
        }
        self.categories.validate()?;
        Ok(())
    }

    pub(crate) fn accepts(&self, level: Level, category: Category) -> bool {
        level != Level::Off && level <= self.level && self.categories.allows(category)
    }
}
