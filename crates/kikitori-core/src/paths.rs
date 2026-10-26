//! kikitori が使う置き場。Python 版と同じ場所にして、Python 版で取得済みのものをそのまま使う。

use std::path::PathBuf;

fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// 利用者のホーム。Python の `Path.home()` と同じく、HOME が無ければ USERPROFILE（Windows）
pub fn home_dir() -> PathBuf {
    var("HOME")
        .or_else(|| var("USERPROFILE"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// モデルの置き場: KIKITORI_MODELS、無ければ ($XDG_CACHE_HOME か ~/.cache)/kikitori/models（Windows も同じ）
pub fn models_root() -> PathBuf {
    var("KIKITORI_MODELS").unwrap_or_else(|| {
        var("XDG_CACHE_HOME")
            .unwrap_or_else(|| home_dir().join(".cache"))
            .join("kikitori")
            .join("models")
    })
}

/// 辞書の置き場: モデルの置き場の隣の dict
pub fn dictionary_root() -> PathBuf {
    let models = models_root();
    models
        .parent()
        .map(|p| p.join("dict"))
        .unwrap_or_else(|| PathBuf::from("dict"))
}

/// Hugging Face のキャッシュ: HF_HUB_CACHE、HF_HOME/hub、~/.cache/huggingface/hub
pub fn hf_cache_root() -> PathBuf {
    var("HF_HUB_CACHE")
        .or_else(|| var("HF_HOME").map(|p| p.join("hub")))
        .unwrap_or_else(|| home_dir().join(".cache").join("huggingface").join("hub"))
}

/// 設定の置き場: $XDG_CONFIG_HOME/kikitori、Windows は %APPDATA%\kikitori、ほかは ~/.config/kikitori
pub fn config_dir() -> PathBuf {
    var("XDG_CONFIG_HOME")
        .or_else(|| if cfg!(windows) { var("APPDATA") } else { None })
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("kikitori")
}
