//! `aura doc` generates API pages from checked sources.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock follows the unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn aura(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aura"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("aura should start")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const SHAPES: &str = concat!(
    "\"\"\"Plane shapes.\"\"\"\n",
    "public class Square:\n",
    "    \"\"\"A square.\"\"\"\n",
    "    public side: float64\n",
    "    \"\"\"Length of one side.\"\"\"\n",
    "public def scale(shape: Square, factor: float64) -> Square:\n",
    "    \"\"\"Return a scaled copy.\n",
    "\n",
    "    Args:\n",
    "        factor: The multiplier.\n",
    "    \"\"\"\n",
    "    return Square(side=shape.side * factor)\n",
    "def helper():\n",
    "    \"\"\"Private and omitted.\"\"\"\n",
);

fn package(prefix: &str) -> TempDir {
    let temp = TempDir::new(prefix);
    fs::create_dir_all(temp.path.join("lib/geometry")).expect("dirs");
    fs::write(temp.path.join("lib/geometry/shapes.au"), SHAPES).expect("shapes");
    fs::write(
        temp.path.join("lib/main.au"),
        "from geometry.shapes import Square, scale\ndef main():\n    print(scale(Square(side=1.0), 2.0).side)\n",
    )
    .expect("main");
    temp
}

#[test]
fn doc_writes_markdown_for_one_file_to_stdout() {
    let temp = package("aura-doc-stdout");
    let output = aura(&temp.path, &["doc", "lib/geometry/shapes.au"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let page = stdout(&output);
    assert!(
        page.starts_with("# Module `shapes`\n\nPlane shapes.\n"),
        "{page}"
    );
    assert!(page.contains("\n## Class `Square`\n\n```aura\nclass Square\n```\n\nA square.\n"));
    assert!(page.contains("- `side: float64`: Length of one side.\n"));
    assert!(page.contains("```aura\ndef scale(shape: Square, factor: float64) -> Square\n```"));
    assert!(page.contains("- `factor: float64`: The multiplier.\n"));
    assert!(!page.contains("helper"));
}

#[test]
fn doc_writes_one_page_per_module_under_out() {
    let temp = package("aura-doc-out");
    let output = aura(&temp.path, &["doc", "lib", "--out", "api"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let markdown = fs::read_to_string(temp.path.join("api/geometry.shapes.md")).expect("page");
    assert!(markdown.starts_with("# Module `geometry.shapes`\n"));
    let main = fs::read_to_string(temp.path.join("api/main.md")).expect("main page");
    assert_eq!(
        main,
        "# Module `main`\n\nThis module has no public declarations.\n"
    );
    assert!(stdout(&output).contains("geometry.shapes.md"));

    let output = aura(&temp.path, &["doc", "--json", "--out", "json", "lib"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(temp.path.join("json/geometry.shapes.json")).expect("json"),
    )
    .expect("valid JSON");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["module"], "geometry.shapes");
    assert_eq!(json["doc"], "Plane shapes.");
    assert_eq!(
        json["declarations"][0]["fields"][0]["doc"],
        "Length of one side."
    );
    assert_eq!(
        json["declarations"][1]["parameters"][1]["doc"],
        "The multiplier."
    );

    let output = aura(&temp.path, &["doc", "--json", "lib/geometry/shapes.au"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output).lines().count(), 1);
}

#[test]
fn doc_reports_check_errors_and_bad_arguments() {
    let temp = package("aura-doc-errors");
    fs::write(
        temp.path.join("broken.au"),
        "def main():\n    print(missing)\n",
    )
    .expect("broken");
    let output = aura(&temp.path, &["doc", "broken.au"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("unknown name `missing`"),
        "{}",
        stderr(&output)
    );

    let output = aura(&temp.path, &["doc", "--bogus"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown aura doc option `--bogus`"));

    let output = aura(&temp.path, &["doc", "--out"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--out requires a directory"));

    let output = aura(&temp.path, &["doc", "missing-dir"]);
    assert_eq!(output.status.code(), Some(1));

    fs::create_dir_all(temp.path.join("other/geometry")).expect("dup dir");
    fs::write(temp.path.join("other/geometry/shapes.au"), SHAPES).expect("dup");
    let output = aura(
        &temp.path,
        &["doc", "lib/geometry/shapes.au", "other/geometry/shapes.au"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("two modules named `shapes`"));
}
