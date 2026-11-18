use super::*;

#[test]
fn each_showing_of_the_screen_has_a_widget_of_its_own() {
    let screen = ScreenWidget::default();
    let first = screen.showing();
    // Built again while it shows, it keeps the widget Blitz already has.
    assert!(screen.showing() == first);
    // Gone, then shown for the next load: Blitz took the first widget, so a new one.
    screen.hidden();
    let second = screen.showing();
    assert!(second != first);
    assert!(screen.showing() == second);
}
