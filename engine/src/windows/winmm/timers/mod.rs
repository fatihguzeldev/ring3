mod state;
#[cfg(test)]
mod tests;
pub use state::{
    MultimediaTimerCallback32, MultimediaTimerDue32, MultimediaTimerError32,
    MultimediaTimerState32, MultimediaTimerTarget32, PreparedMultimediaTimer32,
};
