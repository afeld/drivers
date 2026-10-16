use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

static ADD_ODBC_TEST_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)add_odbc_test\s*\((.*?)\)").unwrap());
static CATCH2_TEST_NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"TEST_CASE(?:_METHOD)?\s*\(\s*(?:\w+\s*,\s*)?"([^"]+)""#).unwrap()
});

pub fn validate(workspace_root: &Path) -> Result<Vec<String>> {
    let tests_dir = workspace_root.join("odbc_tests/tests");
    if !tests_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut registered: Vec<PathBuf> = Vec::new();
    for cmake in cmake_lists(&tests_dir)? {
        let content = fs::read_to_string(&cmake)
            .with_context(|| format!("failed to read {}", cmake.display()))?;
        let cmake_dir = cmake.parent().unwrap_or(Path::new(""));
        for source in add_odbc_test_sources(&content) {
            registered.push(cmake_dir.join(source));
        }
    }

    let mut issues = unlisted_integration_sources(workspace_root, &tests_dir, &registered);
    issues.extend(duplicate_catch2_names(workspace_root, &registered)?);
    issues.sort();
    Ok(issues)
}

fn cmake_lists(tests_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut lists = Vec::new();
    for entry in WalkDir::new(tests_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if entry.file_type().is_file() && entry.file_name() == "CMakeLists.txt" {
            lists.push(entry.path().to_path_buf());
        }
    }
    Ok(lists)
}

fn add_odbc_test_sources(cmake: &str) -> Vec<String> {
    let stripped = strip_cmake_comments(cmake);
    let mut sources = Vec::new();
    for caps in ADD_ODBC_TEST_REGEX.captures_iter(&stripped) {
        let mut args = caps[1].split_whitespace();
        args.next();
        sources.extend(args.map(str::to_string));
    }
    sources
}

fn strip_cmake_comments(cmake: &str) -> String {
    cmake
        .lines()
        .map(|line| match line.find('#') {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unlisted_integration_sources(
    workspace_root: &Path,
    tests_dir: &Path,
    registered: &[PathBuf],
) -> Vec<String> {
    let integration_dir = tests_dir.join("integration");
    if !integration_dir.is_dir() {
        return Vec::new();
    }
    let registered_rel: HashSet<PathBuf> = registered.iter().map(|path| normalize(path)).collect();
    let mut issues = Vec::new();
    for entry in WalkDir::new(&integration_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "cpp"))
    {
        let path = normalize(entry.path());
        if !registered_rel.contains(&path) {
            issues.push(format!(
                "integration source {} is not passed to add_odbc_test",
                display(workspace_root, entry.path())
            ));
        }
    }
    issues
}

fn duplicate_catch2_names(workspace_root: &Path, registered: &[PathBuf]) -> Result<Vec<String>> {
    let mut names: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for source in registered {
        if !source.is_file() {
            continue;
        }
        let content = fs::read_to_string(source)
            .with_context(|| format!("failed to read {}", source.display()))?;
        for caps in CATCH2_TEST_NAME_REGEX.captures_iter(&content) {
            names
                .entry(caps[1].to_string())
                .or_default()
                .push(source.clone());
        }
    }

    let mut issues = Vec::new();
    for (name, mut paths) in names {
        if paths.len() < 2 {
            continue;
        }
        paths.sort();
        let listed = paths
            .iter()
            .map(|path| display(workspace_root, path))
            .collect::<Vec<_>>()
            .join(", ");
        issues.push(format!(
            "Catch2 test name \"{name}\" is registered more than once: {listed}"
        ));
    }
    Ok(issues)
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn display(workspace_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(workspace_root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn should_accept_unique_names_and_a_complete_integration_list() {
        let root = tempfile::tempdir().unwrap();
        let tests = root.path().join("odbc_tests/tests");
        write(
            &tests.join("integration/CMakeLists.txt"),
            "add_odbc_test(integration_auth authentication/login.cpp)\n",
        );
        write(
            &tests.join("integration/authentication/login.cpp"),
            "TEST_CASE(\"should log in\", \"[integration]\") {}\n",
        );
        write(
            &tests.join("e2e/CMakeLists.txt"),
            "add_odbc_test(e2e_auth authentication/login.cpp)\n",
        );
        write(
            &tests.join("e2e/authentication/login.cpp"),
            "TEST_CASE(\"should log in from e2e\", \"[e2e]\") {}\n",
        );
        write(
            &tests.join("capture_harness/CMakeLists.txt"),
            "add_odbc_test(capture_harness capture.cpp)\n",
        );

        let issues = validate(root.path()).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn should_report_a_duplicate_name_and_an_unlisted_integration_source() {
        let root = tempfile::tempdir().unwrap();
        let tests = root.path().join("odbc_tests/tests");
        write(
            &tests.join("integration/CMakeLists.txt"),
            "add_odbc_test(integration_one one.cpp)\n",
        );
        write(
            &tests.join("integration/one.cpp"),
            "TEST_CASE(\"should match\", \"[integration]\") {}\n",
        );
        write(
            &tests.join("integration/two.cpp"),
            "TEST_CASE(\"should match\", \"[integration]\") {}\n",
        );
        write(
            &tests.join("e2e/CMakeLists.txt"),
            "add_odbc_test(e2e_one one.cpp)\n",
        );
        write(
            &tests.join("e2e/one.cpp"),
            "TEST_CASE_METHOD(Fixture, \"should match\", \"[e2e]\") {}\n",
        );

        let issues = validate(root.path()).unwrap();
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("is not passed to add_odbc_test")
                    && issue.contains("integration/two.cpp")),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("should match") && issue.contains("e2e/one.cpp")),
            "{issues:?}"
        );
    }
}
