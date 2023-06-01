use crate::{Config, HuakResult, Error, dependency::Dependency};
use std::fs::File;
use std::io::Write;
use std::collections::HashSet;
use std::path::PathBuf;
use indexmap::IndexMap;
use pep508_rs::Requirement;

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
            return Err(Error::OutputFilePathDoesNotExist(parent_dir.to_string_lossy().into_owned()));
        }
    }

    let dependencies = metadata.metadata().dependencies();
    let optional_dependencies = metadata.metadata().optional_dependencies();

    if dependencies.is_none() || optional_dependencies.is_none() {
        return Err(Error::ProjectDependenciesNotFound);
    }

    let dependencies: Vec<Dependency> = dependencies.unwrap_or(&[])
        .iter()
        .map(Dependency::from)
        .collect();

    let include = options.include.as_deref().map(|s| s.split(',').map(String::from).collect::<Vec<_>>());
    let exclude = options.exclude.as_deref().map(|s| s.split(',').map(String::from).collect::<Vec<_>>());

    let include_slice = include.as_deref().unwrap_or(&[]);
    let exclude_slice = exclude.as_deref().unwrap_or(&[]);

    let mut all_dependencies = if include_slice.contains(&"required".to_string()) && !exclude_slice.contains(&"required".to_string()) {
        dependencies.clone()
    } else {
        vec![]
    };

    let processed_dependencies = process_dependencies(include_slice, exclude_slice, &dependencies, &optional_dependencies)?;
    all_dependencies.extend(processed_dependencies);

    let mut output_file = match File::create(&output_file_path) {
        Ok(file) => file,
        Err(e) => return Err(Error::IOError(e)),
    };

    for dependency in all_dependencies {
        let line = format!("{}\n", dependency);
        write!(output_file, "{}", line).unwrap();
    }

    Ok(())
}


fn process_dependencies(include: &[String], exclude: &[String], dependencies: &Vec<Dependency>, optional_dependencies: &Option<&IndexMap<String, Vec<Requirement>>>) -> HuakResult<Vec<Dependency>> {
    let mut include_set: HashSet<_> = include.iter().cloned().collect();
    let exclude_set: HashSet<_> = exclude.iter().cloned().collect();

    // Add "required" to the include set by default
    include_set.insert("required".to_string());

    // Create a combined IndexMap of all dependencies
    let mut all_dependencies: IndexMap<String, Vec<Requirement>> = IndexMap::new();
    for dep in dependencies {
        all_dependencies.insert(dep.name().parse().unwrap(), vec![dep.requirement().clone()]);
    }
    if let Some(opt_deps) = optional_dependencies {
        all_dependencies.extend(opt_deps.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    // Check if all groups in include and exclude exist
    let all_groups: HashSet<_> = all_dependencies.keys().cloned().collect();
    for group in include_set.union(&exclude_set) {
        // if group is 'required', skip
        if group == "required" {
            continue;
        } else if !all_groups.contains(group) {
            return Err(Error::DependencyGroupNotFound(group.to_string()));
        }
    }

    // Check if a group is both included and excluded
    let conflict_groups: Vec<String> = include_set.intersection(&exclude_set).cloned().collect();
    if !conflict_groups.is_empty() {
        return Err(Error::DependencyGroupConflict(conflict_groups.join(", ")));
    }

    let processed_dependencies: Vec<Dependency> = all_dependencies
        .iter()
        .filter_map(|(group, deps)| {
            if include.is_empty() && exclude.contains(group) {
                None
            } else if (exclude.is_empty() && include.contains(group)) || (include.contains(group) && !exclude.contains(group)) {
                Some(deps.iter().map(Dependency::from).collect::<Vec<_>>())
            } else if include.is_empty() && exclude.is_empty() {
                // Add all dependencies if neither include nor exclude is specified
                Some(deps.iter().map(Dependency::from).collect::<Vec<_>>())
            } else {
                None
            }
        })

        .flatten()
        .collect();

    Ok(processed_dependencies)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fs, ops::test_config, test_resources_dir_path, Verbosity};
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

        let requirements_txt =
            std::fs::read_to_string(config.workspace().root().join("requirements.txt")).unwrap();
        let requirements: HashSet<String> = requirements_txt.lines().map(|s| s.to_string()).collect();

        let metadata = config.workspace().current_local_metadata().unwrap();
        let dependencies = metadata.metadata().dependencies();
        let optional_dependencies = metadata.metadata().optional_dependencies();
        let dependencies: Vec<Dependency> = dependencies.unwrap_or(&[])
            .iter()
            .map(|dep| Dependency::from(dep))
            .collect();
        let processed_dependencies = process_dependencies(&[], &[], &dependencies, &optional_dependencies).unwrap();

        let mut metadata_dependencies: HashSet<String> = HashSet::new();
        for dep in processed_dependencies {
            metadata_dependencies.insert(dep.to_string());
        }

        assert_eq!(requirements, metadata_dependencies);
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
