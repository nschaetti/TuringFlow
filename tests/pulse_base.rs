use turingflow::pulse::{Container, PulseApp, TextBox, Theme};

#[test]
fn vstack_renders_multiple_lines() {
    let root = Container::vstack()
        .push(TextBox::new("Hello"))
        .push(TextBox::new("World"));

    let output = PulseApp::new(root).render_to_string();
    assert!(output.contains("[ Hello ]\n[ World ]"));
}

#[test]
fn hstack_renders_on_single_line() {
    let root = Container::hstack()
        .with_spacing(3)
        .push(TextBox::new("One"))
        .push(TextBox::new("Two"));

    let output = PulseApp::new(root).render_to_string();
    assert!(output.contains("[ One ]   [ Two ]"));
}

#[test]
fn theme_file_can_be_loaded() {
    let theme = Theme::from_file("config/pulse-default.yaml").expect("theme should load");
    assert_eq!(theme.colors.primary, "cyan");
    assert_eq!(theme.widgets.textbox_prefix, "[ ");
}
