use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

pub const HIGHLIGHT_LIMIT: usize = 5 * 1024 * 1024;
pub const FILE_LIMIT: u64 = 32 * 1024 * 1024;

/// Read losslessly. Unsupported encodings must never become replacement characters on save.
pub fn read(path: &Path) -> io::Result<String> {
    let bytes = read_bytes(path)?;
    if bytes.contains(&0) {
        return Err(io::Error::other("无法预览：二进制文件"));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("无法预览：目前仅支持 UTF-8 文本"))
}

pub fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return Err(io::Error::other("无法预览：不是普通文件，或文件超过 32 MB"));
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(io::Error::other("无法预览：文件超过 32 MB"));
    }
    Ok(bytes)
}

/// Save beside the original, then atomically replace it. A failed write keeps the original intact.
pub fn save(path: &Path, text: &str, expected: &str) -> io::Result<()> {
    if read(path)? != expected {
        return Err(io::Error::other(
            "文件已在磁盘变更，未覆盖。请保留当前修改并重新打开项目",
        ));
    }
    let permissions = fs::metadata(path)?.permissions();
    if permissions.readonly() {
        return Err(io::Error::other("文件为只读，未保存"));
    }
    let mut temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| io::Error::other("无效文件路径"))?,
    )?;
    temp.as_file().set_permissions(permissions)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    // Check again after writing the temporary file to narrow the external-editor race.
    if read(path)? != expected {
        return Err(io::Error::other("文件已在磁盘变更，未覆盖"));
    }
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn language(path: &Path) -> &'static str {
    match path.extension().and_then(|x| x.to_str()).unwrap_or("") {
        "rs" => "rust",
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "json" => "json",
        "toml" => "toml",
        "md" | "mdx" => "markdown",
        "yml" | "yaml" => "yaml",
        "html" | "htm" => "html",
        "css" => "css",
        _ => "plain_text",
    }
}
