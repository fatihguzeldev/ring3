#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerThreadingModel32 {
    Apartment,
    Free,
    Both,
    Neutral,
    MainSta,
}
