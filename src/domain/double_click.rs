//! Tells a double click on the tray icon apart from two separate clicks.
//! Slint only reports single clicks on the tray icon, so two clicks within
//! the system double-click time count as one double click (S24).

#[derive(Debug, Default)]
pub struct DoubleClick {
    first_click_ms: Option<u64>,
}

impl DoubleClick {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a click at `now_ms`. Returns true when it completes a double
    /// click: it comes at most `window_ms` after the previous click. The
    /// pair is used up, so a third quick click starts a new pair.
    pub fn click(&mut self, now_ms: u64, window_ms: u64) -> bool {
        match self.first_click_ms.take() {
            Some(first) if now_ms.saturating_sub(first) <= window_ms => true,
            _ => {
                self.first_click_ms = Some(now_ms);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: u64 = 500;

    #[test]
    fn two_quick_clicks_are_a_double_click() {
        let mut d = DoubleClick::new();
        assert!(!d.click(1_000, WINDOW));
        assert!(d.click(1_300, WINDOW));
    }

    #[test]
    fn the_second_click_may_come_exactly_at_the_limit() {
        let mut d = DoubleClick::new();
        assert!(!d.click(1_000, WINDOW));
        assert!(d.click(1_500, WINDOW));
    }

    #[test]
    fn slow_clicks_are_separate_clicks() {
        let mut d = DoubleClick::new();
        assert!(!d.click(1_000, WINDOW));
        assert!(!d.click(1_501, WINDOW));
        // The slow click starts a new pair.
        assert!(d.click(1_700, WINDOW));
    }

    #[test]
    fn a_third_quick_click_starts_a_new_pair() {
        let mut d = DoubleClick::new();
        assert!(!d.click(1_000, WINDOW));
        assert!(d.click(1_200, WINDOW));
        assert!(!d.click(1_400, WINDOW));
        assert!(d.click(1_600, WINDOW));
    }
}
