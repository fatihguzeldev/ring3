use super::Point32;

// menu bitmap metrics are distinct from font-based owner-draw padding.
pub(crate) struct MenuMetrics32;
impl MenuMetrics32 {
    pub(crate) fn check_size() -> Point32 {
        Point32 { x: 15, y: 15 }
    }
    pub(crate) fn menu_height() -> i32 {
        20
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_metrics_keep_check_bitmaps_distinct_from_menu_row_height() {
        assert_eq!(MenuMetrics32::check_size(), Point32 { x: 15, y: 15 });
        assert_eq!(MenuMetrics32::menu_height(), 20);
    }
}
