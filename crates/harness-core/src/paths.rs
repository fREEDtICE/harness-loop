use std::path::{Component, PathBuf};

pub fn normalize_path(path: PathBuf) -> PathBuf {
    let is_absolute = path.is_absolute();
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = normalized.components().next_back().is_some_and(|last| {
                    !matches!(
                        last,
                        Component::RootDir | Component::Prefix(_) | Component::ParentDir
                    )
                });

                if can_pop {
                    normalized.pop();
                } else if !is_absolute {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    if normalized.as_os_str().is_empty() {
        if is_absolute {
            PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
        } else {
            PathBuf::from(".")
        }
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::normalize_path;

    #[test]
    fn normalize_absolute_parent_segments() {
        let path = normalize_path(PathBuf::from("/tmp/project/config/../runs"));
        assert_eq!(path, PathBuf::from("/tmp/project/runs"));
    }

    #[test]
    fn normalize_relative_parent_segments() {
        let path = normalize_path(PathBuf::from("config/../runs"));
        assert_eq!(path, PathBuf::from("runs"));
    }
}
