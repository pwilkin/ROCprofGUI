use std::path::PathBuf;

pub const DIR_NAME: &str = ".rocprofgui";
pub const OVERRIDE_VAR: &str = "ROCPROFGUI_HOME";

pub fn root() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os(OVERRIDE_VAR) {
        if explicit.is_empty() {
            return Err(format!("{OVERRIDE_VAR} is set but empty"));
        }
        return Ok(PathBuf::from(explicit));
    }
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .ok_or_else(|| format!("HOME is not set; set {OVERRIDE_VAR} to choose a data directory"))?;
    Ok(PathBuf::from(home).join(DIR_NAME))
}

pub fn subdir(relative: &str) -> Result<PathBuf, String> {
    let path = root()?.join(relative);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn override_wins_over_home_and_empty_values_are_rejected() {
        temp_env(&[(super::OVERRIDE_VAR, Some("/tmp/explicit")), ("HOME", Some("/home/ignored"))], || {
            assert_eq!(super::root().unwrap(), std::path::Path::new("/tmp/explicit"));
        });
        temp_env(&[(super::OVERRIDE_VAR, Some("")), ("HOME", Some("/home/u"))], || {
            assert!(super::root().is_err());
        });
        temp_env(&[(super::OVERRIDE_VAR, None), ("HOME", Some("/home/u"))], || {
            assert_eq!(super::root().unwrap(), std::path::Path::new("/home/u/.rocprofgui"));
        });
        temp_env(&[(super::OVERRIDE_VAR, None), ("HOME", None)], || {
            assert!(super::root().is_err());
        });
    }

    fn temp_env(vars: &[(&str, Option<&str>)], body: impl FnOnce()) {
        let saved: Vec<_> = vars.iter().map(|(k, _)| (*k, std::env::var_os(k))).collect();
        for (k, v) in vars {
            match v {
                Some(value) => std::env::set_var(k, value),
                None => std::env::remove_var(k),
            }
        }
        body();
        for (k, v) in saved {
            match v {
                Some(value) => std::env::set_var(k, value),
                None => std::env::remove_var(k),
            }
        }
    }
}
