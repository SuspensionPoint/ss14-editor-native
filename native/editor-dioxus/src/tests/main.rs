use std::path::Path;

/// The components with no props that read the editor and do not call `watch`, as "file: name".
/// Dioxus draws a component again when its parent does only if its props changed, so one with
/// none is drawn once and keeps what it read then. The options bar's buttons stayed put that
/// way. The bodies are read up to the first line that is a lone closing brace.
fn deaf_components(source: &Path) -> Vec<String> {
    let mut deaf = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(source).expect("src").filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|ext| ext == "rs")).collect();
    files.sort();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("source file");
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            if line.trim() != "#[component]" {
                continue;
            }
            let Some(signature) = lines.get(at + 1) else { continue };
            let Some(name) = signature.split("fn ").nth(1).and_then(|rest| rest.split('(').next()) else { continue };
            if !signature.contains(&format!("{name}() -> Element")) {
                continue;
            }
            let body: Vec<&str> = lines[at + 2..].iter().take_while(|line| **line != "}").copied().collect();
            let body = body.join("\n");
            let reads = reads_editor(&body);
            if reads && !body.contains("watch(") && !body.contains("watch_map(") {
                deaf.push(format!("{}: {name}", file.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    deaf
}

/// Whether code calls `with` or `try_with`, and not some other `..._with`.
fn reads_editor(code: &str) -> bool {
    code.match_indices("with(").any(|(at, _)| {
        let before = &code[..at];
        before.ends_with("try_") || !before.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

#[test]
fn reads_editor_tells_with_from_other_names() {
    assert!(reads_editor("let x = with(|editor| 1);"));
    assert!(reads_editor("try_with(|editor| 1)"));
    assert!(!reads_editor("name.starts_with(\"a\")"));
}

#[test]
fn every_component_with_no_props_that_reads_the_editor_watches_it() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert_eq!(deaf_components(&source), Vec::<String>::new());
}
