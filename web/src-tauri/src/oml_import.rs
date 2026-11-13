use serde_json::Value;
use std::{collections::HashSet, path::PathBuf, process::Command};

pub fn create(
    hub: &mut hub_core::Hub,
    mut args: Value,
    files: &Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let files = files
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("OMLファイルを選択してください")?;
    let executable = std::env::current_exe()?.with_file_name("oml.exe");
    if !executable.is_file() {
        return Err("OML Utilitiesのoml.exeをこのアプリと同じフォルダーに置いてください".into());
    }
    let temporary = tempfile::tempdir()?;
    let sources = temporary.path().join("sources");
    let specs = temporary.path().join("specs");
    std::fs::create_dir(&sources)?;
    let mut names = HashSet::new();
    for value in files {
        let file = PathBuf::from(value.as_str().ok_or("OMLファイルの指定が不正です")?);
        if !file.is_absolute()
            || !file.is_file()
            || !file
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("oml"))
        {
            return Err("有効なOMLファイルを選択してください".into());
        }
        let name = file.file_name().ok_or("ファイル名がありません")?;
        if !names.insert(name.to_string_lossy().to_lowercase()) {
            return Err("同じ名前のOMLファイルが複数あります".into());
        }
        std::fs::copy(&file, sources.join(name))?;
    }
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = command
        .arg("document")
        .arg(&sources)
        .arg(&specs)
        .args(["--format", "md", "--single-md"])
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "OML仕様書の生成に失敗しました（{}）\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    args["root"] = serde_json::json!(specs);
    args["oml"] = serde_json::json!(true);
    hub_service::call(hub, "admin/create", &args).map_err(Into::into)
}
