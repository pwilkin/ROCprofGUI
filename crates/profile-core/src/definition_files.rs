use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

pub const MAX_DEFINITION_BYTES: usize = 256 * 1024;
#[derive(Debug, Serialize, Deserialize)]
pub struct DefinitionFile {
    pub name: String,
    pub text: String,
}
fn validate(text: &str) -> Result<(), String> {
    if text.len() > MAX_DEFINITION_BYTES {
        return Err("Definition file exceeds 256 KiB".into());
    }
    if text.contains('\0') {
        return Err("Definition files cannot contain NUL bytes".into());
    }
    Ok(())
}
pub fn read(path: &Path) -> Result<DefinitionFile, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_DEFINITION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| "Definition file must be UTF-8 text")?;
    validate(&text)?;
    Ok(DefinitionFile {
        name: path
            .file_name()
            .ok_or("Missing filename")?
            .to_string_lossy()
            .into_owned(),
        text,
    })
}
pub fn write(path: &Path, text: &str) -> Result<(), String> {
    validate(text)?;
    let parent = path.parent().ok_or("Missing parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
