pub const MINIMUM_PERIOD_MS: u32 = 1;
pub const MAXIMUM_PERIOD_MS: u32 = 65535;
pub const TIMERR_NOCANDO: u32 = 97;

#[derive(Default)]
pub struct TimerPeriods32 {
    requests: Vec<(u32, u32)>,
}
impl TimerPeriods32 {
    pub fn begin(&mut self, period: u32) -> u32 {
        if !(MINIMUM_PERIOD_MS..=MAXIMUM_PERIOD_MS).contains(&period) {
            return TIMERR_NOCANDO;
        }
        if let Some((_, count)) = self.requests.iter_mut().find(|(value, _)| *value == period) {
            let Some(next) = count.checked_add(1) else {
                return TIMERR_NOCANDO;
            };
            *count = next;
        } else {
            if self.requests.try_reserve(1).is_err() {
                return TIMERR_NOCANDO;
            }
            self.requests.push((period, 1));
        }
        0
    }
    pub fn end(&mut self, period: u32) -> u32 {
        let Some(index) = self.requests.iter().position(|(value, _)| *value == period) else {
            return TIMERR_NOCANDO;
        };
        self.requests[index].1 -= 1;
        if self.requests[index].1 == 0 {
            self.requests.swap_remove(index);
        }
        0
    }
    pub fn minimum(&self) -> Option<u32> {
        self.requests.iter().map(|(period, _)| *period).min()
    }
    pub fn clear(&mut self) {
        self.requests.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn balanced_periods_keep_lowest_live_request_and_invalid_requests_are_neutral() {
        let mut periods = TimerPeriods32::default();
        assert_eq!(periods.begin(10), 0);
        assert_eq!(periods.begin(1), 0);
        assert_eq!(periods.begin(1), 0);
        assert_eq!(periods.begin(0), 97);
        assert_eq!(periods.begin(65536), 97);
        assert_eq!(periods.minimum(), Some(1));
        assert_eq!(periods.end(1), 0);
        assert_eq!(periods.minimum(), Some(1));
        assert_eq!(periods.end(1), 0);
        assert_eq!(periods.minimum(), Some(10));
        assert_eq!(periods.end(1), 97);
        assert_eq!(periods.end(10), 0);
        assert_eq!(periods.minimum(), None);
    }
}
