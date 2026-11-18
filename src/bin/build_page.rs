//! Inject the compact run tracks into the HTML template to produce a single page.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

const PLACEHOLDER: &str = "__RUN_DATA__";

/// Inject the compact run tracks into the HTML template to produce a single page.
#[derive(Parser)]
struct Args {
    /// Page source with a data placeholder.
    template: PathBuf,
    /// Compact run track file written by parse_runs.
    data: PathBuf,
    /// Finished page to write.
    output: PathBuf,
}

/// Return the template with the run data in place of the first placeholder.
fn embed(template: &str, data: &str) -> Result<String, String> {
    if !template.contains(PLACEHOLDER) {
        return Err("template has no data placeholder".to_owned());
    }
    if data.contains("</") {
        return Err("data contains a closing tag sequence".to_owned());
    }
    Ok(template.replacen(PLACEHOLDER, data, 1))
}

/// Read the template and the data, embed one in the other and write the page.
fn run(args: &Args) -> Result<(), String> {
    let read = |path: &PathBuf| {
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let page = embed(&read(&args.template)?, &read(&args.data)?)?;
    fs::write(&args.output, page).map_err(|error| format!("{}: {error}", args.output.display()))
}

/// Write the template with the run data embedded in place of the placeholder.
fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The data replaces the first placeholder only.
    #[test]
    fn replaces_first_placeholder() {
        let page = embed("<a>__RUN_DATA__</a>__RUN_DATA__", "{\"x\":1}").unwrap();
        assert_eq!(page, "<a>{\"x\":1}</a>__RUN_DATA__");
    }

    /// A template without a placeholder is rejected.
    #[test]
    fn rejects_missing_placeholder() {
        assert_eq!(
            embed("<a></a>", "{}").unwrap_err(),
            "template has no data placeholder"
        );
    }

    /// Data that could close the script element is rejected.
    #[test]
    fn rejects_closing_tag_in_data() {
        assert_eq!(
            embed(PLACEHOLDER, "\"</script>\"").unwrap_err(),
            "data contains a closing tag sequence"
        );
    }
}
