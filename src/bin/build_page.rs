//! Inject the compact run tracks into the HTML template to produce a single page.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
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

/// Write a file, creating its folder first when it is missing.
fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(path, contents)
}

/// Read the template and the data, embed one in the other and write the page.
fn run(args: &Args) -> Result<(), String> {
    let read = |path: &PathBuf| {
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let page = embed(&read(&args.template)?, &read(&args.data)?)?;
    write_file(&args.output, &page).map_err(|error| format!("{}: {error}", args.output.display()))
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

    /// The page lands in a folder that does not exist yet.
    #[test]
    fn write_file_creates_missing_folders() {
        let root = std::env::temp_dir().join(format!("build_page_test_{}", std::process::id()));
        let page = root.join("site").join("index.html");
        write_file(&page, "<html></html>").unwrap();
        assert_eq!(fs::read_to_string(&page).unwrap(), "<html></html>");
        fs::remove_dir_all(&root).unwrap();
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
