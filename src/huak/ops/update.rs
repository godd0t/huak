use crate::{
    dependency::{dependency_iter, Dependency},
    Config, HuakResult, InstallOptions,
};
use std::str::FromStr;

pub struct UpdateOptions {
    pub install_options: InstallOptions,
}

pub fn update_project_dependencies(
    dependencies: Option<Vec<String>>,
    config: &Config,
    options: &UpdateOptions,
) -> HuakResult<()> {
    let workspace = config.workspace();
    let package = workspace.current_package()?;
    let mut metadata = workspace.current_local_metadata()?;
    let python_env = workspace.resolve_python_environment()?;

    // Collect dependencies to update if they are listed in the metadata file.
    if let Some(it) = dependencies.as_ref() {
        let deps = dependency_iter(it)
            .filter_map(|dep| {
                if metadata
                    .metadata()
                    .contains_dependency_any(&dep)
                    .unwrap_or_default()
                {
                    Some(dep)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if deps.is_empty() {
            return Ok(());
        }

        python_env.update_packages(&deps, &options.install_options, config)?;
    } else {
        let mut deps = metadata
            .metadata()
            .dependencies()
            .map(|reqs| reqs.iter().map(Dependency::from).collect::<Vec<_>>())
            .unwrap_or(Vec::new());

        if let Some(odeps) = metadata.metadata().optional_dependencies() {
            odeps.values().for_each(|reqs| {
                deps.extend(
                    reqs.iter().map(Dependency::from).collect::<Vec<_>>(),
                )
            });
        }

        deps.dedup();
        python_env.update_packages(&deps, &options.install_options, config)?;
    }

    // Get all groups from the metadata file to include in the removal process.
    let mut groups = Vec::new();
    if let Some(deps) = metadata.metadata().optional_dependencies() {
        groups.extend(deps.keys().map(|key| key.to_string()));
    }

    for pkg in python_env.installed_packages()? {
        let dep = &Dependency::from_str(&pkg.to_string())?;
        if metadata.metadata().contains_dependency(dep)? {
            metadata.metadata_mut().remove_dependency(dep);
            metadata.metadata_mut().add_dependency(dep.clone())
        }
        for g in groups.iter() {
            if metadata.metadata().contains_optional_dependency(dep, g)? {
                metadata.metadata_mut().remove_optional_dependency(dep, g);
                metadata
                    .metadata_mut()
                    .add_optional_dependency(dep.clone(), g);
            }
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
    use crate::{
        fs,
        ops::{test_config, test_venv},
        test_resources_dir_path, Verbosity,
    };
    use tempfile::tempdir;

    #[test]
    fn test_update_project_dependencies() {
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
        let options = UpdateOptions {
            install_options: InstallOptions { values: None },
        };

        update_project_dependencies(None, &config, &options).unwrap();
    }

    #[test]
    fn test_update_project_optional_dependencies() {
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
        let options = UpdateOptions {
            install_options: InstallOptions { values: None },
        };

        update_project_dependencies(None, &config, &options).unwrap();
    }
}
