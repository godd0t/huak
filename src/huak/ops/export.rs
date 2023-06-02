use crate::{dependency::Dependency, Config, Error, HuakResult};
use indexmap::IndexMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

pub struct ExportOptions {
    pub include: Option<String>,
    pub exclude: Option<String>,
    pub output_file: String,
}

pub fn export_dependencies_to_file(
    config: &Config,
    options: &ExportOptions,
) -> HuakResult<()> {
    let workspace = config.workspace();
    let metadata = workspace.current_local_metadata()?;

    // Validate the output file directory
    let output_file_path = if options.output_file.starts_with('/') {
        // If it's a full path, use it directly.
        PathBuf::from(&options.output_file)
    } else {
        // If it's not a full path, join it with the workspace root.
        workspace.root().join(&options.output_file)
    };

    if let Some(parent_dir) = output_file_path.parent() {
        if !parent_dir.exists() {
            return Err(Error::OutputFilePathDoesNotExist(
                parent_dir.to_string_lossy().into_owned(),
            ));
        }
    }

    let dependencies = metadata.metadata().dependencies();
    let optional_dependencies = metadata.metadata().optional_dependencies();

    if dependencies.is_none() || optional_dependencies.is_none() {
        return Err(Error::ProjectDependenciesNotFound);
    }

    let dependencies: Vec<Dependency> = dependencies
        .unwrap_or(&[])
        .iter()
        .map(Dependency::from)
        .collect();

    let optional_dependencies = metadata.metadata().optional_dependencies();

    let mut all_dependencies: IndexMap<String, Vec<Dependency>> =
        IndexMap::new();
    for dep in &dependencies {
        all_dependencies
            .entry("required".to_string())
            .or_insert_with(Vec::new)
            .push(dep.clone());
    }
    if let Some(opt_deps) = optional_dependencies {
        for (group, reqs) in opt_deps {
            let deps = reqs.iter().map(Dependency::from).collect();
            all_dependencies.insert(group.clone(), deps);
        }
    }

    let include = options
        .include
        .as_deref()
        .map(|s| s.split(',').map(String::from).collect::<Vec<_>>());
    let exclude = options
        .exclude
        .as_deref()
        .map(|s| s.split(',').map(String::from).collect::<Vec<_>>());

    let include_slice = include.as_deref().unwrap_or(&[]);
    let exclude_slice = exclude.as_deref().unwrap_or(&[]);

    let processed_dependencies =
        process_dependencies(include_slice, exclude_slice, &all_dependencies)?;

    let mut output_file = match File::create(&output_file_path) {
        Ok(file) => file,
        Err(e) => return Err(Error::IOError(e)),
    };

    for dependency in processed_dependencies {
        let line = format!("{}\n", dependency);
        write!(output_file, "{}", line)?;
    }

    Ok(())
}

fn process_dependencies(
    include: &[String],
    exclude: &[String],
    all_dependencies: &IndexMap<String, Vec<Dependency>>,
) -> HuakResult<Vec<Dependency>> {
    // We initialize an empty vector to hold the dependencies that pass the filters.
    let mut processed_dependencies: Vec<Dependency> = Vec::new();

    // We iterate over all the dependencies.
    for (group, deps) in all_dependencies {
        // We check if the group of dependencies is included in the filters.
        // If no groups are specified for inclusion, we include the group as long as it's not specified for exclusion.
        // If some groups are specified for inclusion, we include the group only if it's in the inclusion list and not in the exclusion list.
        if (include.is_empty() && !exclude.contains(group))
            || (!include.is_empty()
                && include.contains(group)
                && !exclude.contains(group))
        {
            // If the group passes the filters, we add all its dependencies to the result vector.
            processed_dependencies.extend_from_slice(deps);
        }
    }

    // We return the result vector.
    Ok(processed_dependencies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fs, ops::test_config, test_resources_dir_path, Verbosity};
    use std::collections::HashSet;
    use tempfile::tempdir;

    #[test]
    fn test_export_dependencies_to_file() {
        let dir = tempdir().unwrap();
        fs::copy_dir(
            test_resources_dir_path().join("mock-project"),
            dir.path().join("mock-project"),
        )
        .unwrap();

        let root = dir.path().join("mock-project");
        let cwd = root.to_path_buf();
        let config = test_config(root.clone(), cwd, Verbosity::Quiet);
        let options = ExportOptions {
            include: None,
            exclude: None,
            output_file: "requirements.txt".to_string(),
        };

        export_dependencies_to_file(&config, &options).unwrap();

        let requirements_txt = std::fs::read_to_string(
            config.workspace().root().join("requirements.txt"),
        )
        .unwrap();
        let requirements: HashSet<String> =
            requirements_txt.lines().map(|s| s.to_string()).collect();

        let metadata = config.workspace().current_local_metadata().unwrap();
        let dependencies = metadata.metadata().dependencies();
        let optional_dependencies = metadata.metadata().optional_dependencies();

        let mut all_dependencies: IndexMap<String, Vec<Dependency>> =
            IndexMap::new();
        for dep in dependencies.unwrap_or(&[]).iter().map(Dependency::from) {
            all_dependencies
                .entry("required".to_string())
                .or_insert_with(Vec::new)
                .push(dep);
        }
        if let Some(opt_deps) = optional_dependencies {
            for (group, reqs) in opt_deps {
                let deps = reqs.iter().map(Dependency::from).collect();
                all_dependencies.insert(group.clone(), deps);
            }
        }

        let processed_dependencies =
            process_dependencies(&[], &[], &all_dependencies).unwrap();

        let processed_requirements: HashSet<String> = processed_dependencies
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert_eq!(requirements, processed_requirements);
    }

    #[test]
    fn test_export_dependencies_to_file_conflicting_groups() {
        let dir = tempdir().unwrap();
        fs::copy_dir(
            test_resources_dir_path().join("mock-project"),
            dir.path().join("mock-project"),
        )
        .unwrap();
        let root = dir.path().join("mock-project");
        let cwd = root.to_path_buf();
        let config = test_config(root, cwd, Verbosity::Quiet);
        let options = ExportOptions {
            include: Some("dev".to_string()),
            exclude: Some("dev".to_string()),
            output_file: "requirements.txt".to_string(),
        };

        let result = export_dependencies_to_file(&config, &options);
        assert!(result.is_err());
        match result {
            Err(Error::DependencyGroupConflict(_)) => (),
            _ => panic!("Expected Error::DependencyGroupConflict"),
        }
    }
}
