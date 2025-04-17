use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PyExtConfig {
    pub debug_dir: Option<PathBuf>,
    pub python_script: PathBuf,

    #[serde(flatten)]
    pub options: HashMap<String, serde_json::Value>,
}
