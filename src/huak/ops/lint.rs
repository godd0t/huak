use super::make_venv_command;
use crate::{dependency::Dependency, Config, HuakResult, InstallOptions};
use std::{process::Command, str::FromStr};

pub struct LintOptions {
    /// A values vector of lint options typically used for passing on arguments.
    pub values: Option<Vec<String>>,
    pub include_types: bool,
    pub install_options: InstallOptions,
}

pub fn lint_project(config: &Config, options: &LintOptions) -> HuakResult<()> {
    let workspace = config.workspace();
    let package = workspace.current_package()?;
    let mut metadata = workspace.current_local_metadata()?;
    let python_env = workspace.resolve_python_environment()?;

    // Install `ruff` if it isn't already installed.
    let ruff_dep = Dependency::from_str("ruff")?;
    let mut lint_deps = vec![ruff_dep.clone()];
    if !python_env.contains_module("ruff")? {
        python_env.install_packages(
            &[&ruff_dep],
            &options.install_options,
            config,
        )?;
    }

    let mut terminal = config.terminal();

    if options.include_types {
        // Install `mypy` if it isn't already installed.
        let mypy_dep = Dependency::from_str("mypy")?;
        if !python_env.contains_module("mypy")? {
            python_env.install_packages(
                &[&mypy_dep],
                &options.install_options,
                config,
            )?;
        }

        // Keep track of the fact that `mypy` is a needed lint dep.
        lint_deps.push(mypy_dep);

        // Run `mypy` excluding the workspace's Python environment directory.
        let mut mypy_cmd = Command::new(python_env.python_path());
        make_venv_command(&mut mypy_cmd, &python_env)?;
        mypy_cmd
            .args(vec![
                "-m",
                "mypy",
                ".",
                "--exclude",
                python_env.name()?.as_str(),
            ])
            .current_dir(workspace.root());
        terminal.run_command(&mut mypy_cmd)?;
    }

    // Run `ruff`.
    let mut cmd = Command::new(python_env.python_path());
    let mut args = vec!["-m", "ruff", "check", "."];
    if let Some(v) = options.values.as_ref() {
        args.extend(v.iter().map(|item| item.as_str()));
    }
    make_venv_command(&mut cmd, &python_env)?;
    cmd.args(args).current_dir(workspace.root());
    terminal.run_command(&mut cmd)?;

    // Add installed lint deps (potentially both `mypy` and `ruff`) to metadata file if not already there.
    let new_lint_deps = lint_deps
        .iter()
        .filter(|dep| {
            !metadata
                .metadata()
                .contains_dependency_any(dep)
                .unwrap_or_default()
        })
        .map(|dep| dep.name())
        .collect::<Vec<_>>();

    if !new_lint_deps.is_empty() {
        for pkg in python_env
            .installed_packages()?
            .iter()
            .filter(|pkg| new_lint_deps.contains(&pkg.name()))
        {
            metadata.metadata_mut().add_optional_dependency(
                Dependency::from_str(&pkg.to_string())?,
                "dev",
            );
        }
    }

    if package.metadata() != metadata.metadata() {
        metadata.write_file()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{test_config, test_venv};
    use crate::{fs, test_resources_dir_path, Verbosity};
    use tempfile::tempdir;

    #[test]
    fn test_lint_project() {
        let dir = tempdir().unwrap();
        fs::copy_dir(
            &test_resources_dir_path().join("mock-project"),
            &dir.path().join("mock-project"),
        )
        .unwrap();
        let root = dir.path().join("mock-project");
        let cwd = root.to_path_buf();
        let config = test_config(root, cwd, Verbosity::Quiet);
        let options = LintOptions {
            values: None,
            include_types: true,
            install_options: InstallOptions { values: None },
        };

        lint_project(&config, &options).unwrap();
    }

    #[test]
    fn test_fix_project() {
        let dir = tempdir().unwrap();
        fs::copy_dir(
            &test_resources_dir_path().join("mock-project"),
            &dir.path().join("mock-project"),
        )
        .unwrap();
        let root = dir.path().join("mock-project");
        let cwd = root.to_path_buf();
        let config = test_config(root, cwd, Verbosity::Quiet);
        let ws = config.workspace();
        test_venv(&ws);
        let options = LintOptions {
            values: Some(vec![String::from("--fix")]),
            include_types: true,
            install_options: InstallOptions { values: None },
        };
        let lint_fix_filepath =
            ws.root().join("src").join("mock_project").join("fix_me.py");
        let pre_fix_str = r#"
import json # this gets removed(autofixed)


def fn():
    pass
"#;
        let expected = r#"


def fn():
    pass
"#;
        std::fs::write(&lint_fix_filepath, pre_fix_str).unwrap();

        lint_project(&config, &options).unwrap();

        let post_fix_str = std::fs::read_to_string(&lint_fix_filepath).unwrap();

        assert_eq!(post_fix_str, expected);
    }
}
