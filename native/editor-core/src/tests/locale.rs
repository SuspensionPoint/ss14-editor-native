use super::*;

fn parsed(text: &str) -> HashMap<String, String> {
    let mut into = HashMap::new();
    parse_ftl(text, &mut into);
    into
}

#[test]
fn plain_messages_are_read_by_their_keys() {
    let text = "signal-port-name-pressed = Pressed\nsignal-port-description-pressed = This port is invoked whenever the transmitter is activated.\n\nsignal-port-name-doorbolt=Door bolt\n";
    let read = parsed(text);
    assert_eq!(read.get("signal-port-name-pressed").map(String::as_str), Some("Pressed"));
    assert_eq!(read.get("signal-port-name-doorbolt").map(String::as_str), Some("Door bolt"));
    assert_eq!(read.len(), 3);
}

#[test]
fn comments_terms_attributes_and_placeables_are_left_out() {
    let text = "\
## A group comment
# signal-port-name-commented = Not this
-brand = Starlight
ent-Thing = A thing
    .desc = What it is.
signal-port-name-brand = { -brand } port
signal-port-name-count = { $count ->
    [one] One
   *[other] Many
}
signal-port-name-after = After
";
    let read = parsed(text);
    let mut keys: Vec<&str> = read.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["ent-Thing", "signal-port-name-after"]);
    // The attribute does not join the text.
    assert_eq!(read["ent-Thing"], "A thing");
}

#[test]
fn a_text_may_go_on_over_indented_lines() {
    let read = parsed("long-name =\n    The first part\n    and the second\nnext = Next\n");
    assert_eq!(read["long-name"], "The first part and the second");
    assert_eq!(read["next"], "Next");
}

#[test]
fn a_key_read_twice_keeps_its_first_text() {
    let mut into = HashMap::new();
    parse_ftl("signal-port-name-toggle = Toggle\n", &mut into);
    parse_ftl("signal-port-name-toggle = Switch\n", &mut into);
    assert_eq!(into["signal-port-name-toggle"], "Toggle");
}

#[test]
fn windows_line_ends_are_read_as_well() {
    let read = parsed("signal-port-name-open = Open\r\nsignal-port-name-close = Close\r\n");
    assert_eq!((read["signal-port-name-open"].as_str(), read["signal-port-name-close"].as_str()), ("Open", "Close"));
}

#[test]
fn a_locale_of_text_answers_its_keys() {
    let locale = Locale::of(parsed("signal-port-name-timer = Timer\n"));
    assert_eq!(locale.get("signal-port-name-timer"), Some("Timer"));
    assert_eq!(locale.get("signal-port-name-nothing"), None);
}

/// Reads the fork's English files. Run by hand: `cargo test -p editor-core --release fork_s_port_names -- --ignored`.
#[test]
#[ignore]
fn the_fork_s_port_names() {
    let fork = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../upstream/starlight");
    let read = read_locale(&fork);
    let ports = read.keys().filter(|key| key.starts_with("signal-port-name-")).count();
    println!("{} messages, {ports} port names", read.len());
    assert_eq!(read.get("signal-port-name-pressed").map(String::as_str), Some("Pressed"));
    assert_eq!(read.get("signal-port-name-doorbolt").map(String::as_str), Some("Door bolt"));
}
