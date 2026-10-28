use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use ureq::ResponseExt;

macro_rules! f {
    ($path:literal,$size:literal,$sha:literal,$git:literal) => {
        FileEntry {
            path: $path,
            size: Some($size),
            sha256: Some($sha),
            git_sha1: Some($git),
        }
    };
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: &'static str,
    pub size: Option<u64>,
    pub sha256: Option<&'static str>,
    pub git_sha1: Option<&'static str>,
}
#[derive(Clone, Debug)]
pub struct Model {
    pub name: &'static str,
    pub role: &'static str,
    pub source: &'static str,
    pub license: &'static str,
    pub files: &'static [FileEntry],
}

const QWEN_FILES: &[FileEntry] = &[
    f!(
        "added_tokens.json",
        1566,
        "de40784677cbd1843cabe5fbee078c7e042cd0b62155f0810af5a13842e5722a",
        "0392e6cfed1ceb0679ecad2450fbacc40dfca138"
    ),
    f!(
        "config.json",
        5820,
        "310e42c3f82d422e8e26e582ad53a595b747fa066bb97e33f1dc5478d63849b1",
        "cc2e204350cdf772716b3203cfdb20e1b7bf8e36"
    ),
    f!(
        "generation_config.json",
        143,
        "832bbd42924612ea75208679eb8306d28f47c5f05d742f4dcd77ae2654bf89e0",
        "c7eb24bc528bdf201b7d66e6cf8f77a8b22150b2"
    ),
    f!(
        "merges.txt",
        1671853,
        "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5",
        "31349551d90c7606f325fe0f11bbb8bd5fa0d7c7"
    ),
    f!(
        "model.safetensors",
        4076191640,
        "4e378e29a060f4a8464a59976a6b84661dc2f88f23a3f43634651de3fc84ccb4",
        "a6d878724f429cdf6f98562ca5470f907bf2edb0"
    ),
    f!(
        "preprocessor_config.json",
        357,
        "0443a40d411d3caad769e7b59971fa6eae75926ed4b7e337e60da909eb38c1f7",
        "75e4c8b236a4eb03aff21b82e719030f3b0f8721"
    ),
    f!(
        "special_tokens_map.json",
        1008,
        "7b376c510ccf9d88bb9bbee41dfc5052122e16e0dec1124a8d8983c59259a9f3",
        "fca9909b535ff91b68474e5ee7258e1574e60d1e"
    ),
    f!(
        "tokenizer.json",
        11429377,
        "bd2a97b55c8f7f9c328c73ee9b9178771037e9f566dfca8e238a063d41cbac92",
        "9e0e26a30e9d33140e6d0a7b94a8e368be879f9f"
    ),
    f!(
        "tokenizer_config.json",
        12517,
        "7f335cbd3b6f1036808e23613c1c1919f8d5570c11cb55420e5094e036f45669",
        "8a1c2ab6e5e14db2c87839d654915c0d77d41fdb"
    ),
    f!(
        "vocab.json",
        2776833,
        "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910",
        "4783fe10ac3adce15ac8f358ef5462739852c569"
    ),
];
const KANA_FILES: &[FileEntry] = &[
    f!(
        "added_tokens.json",
        34648,
        "3c51f66c4c21f9e126970078f11ae77a78c74aee8df606ee9daba86e467108e0",
        "1b33526d33aaa60d79f78ae8651dae50b730185a"
    ),
    f!(
        "config.json",
        1199,
        "849430a620146c3340a7e2fb58df71edae38541cc4cf205ddad8fa7dcf61c168",
        "5fc6c371145c6d3c6bf8b8f52ad59e8d6af21df1"
    ),
    f!(
        "generation_config.json",
        3750,
        "43c37afdfc8467cc9b9d9578a0763108af92f5d3455c9d5406236b741077b3f9",
        "10b6194fb1e735bd4f6cb5cff523f01fab159007"
    ),
    f!(
        "merges.txt",
        493869,
        "2df2990a395e35e8dfbc7511e08c12d56018d8d04691e0133e5d63b21e154dc6",
        "6038932a2a1f09a66991b1c2adae0d14066fa29e"
    ),
    f!(
        "model.safetensors",
        3235581408,
        "1f1c5037afee72e7d6d632a8de5fa623b6c749b2a5c8bdd8ca8edb6ee3c76857",
        "90605612433449cd177d199337512c74e1fdd21c"
    ),
    f!(
        "normalizer.json",
        52666,
        "bf1c507dc8724ca9cf9903640dacfb69dae2f00edee4f21ceba106a7392f26dd",
        "dd6ae819ad738ac1a546e9f9282ef325c33b9ea0"
    ),
    f!(
        "preprocessor_config.json",
        357,
        "654cf18d3e163b948ceaf9766da56ce0b52de265d58673cf61c9376f126bd499",
        "3f567c1115178e9fd2e1ef818dca934e22ca0c85"
    ),
    f!(
        "special_tokens_map.json",
        2186,
        "baea4ea09372eb4fca86b4e4346139fd73cb807d5087e9de0948e971739c3e74",
        "312bc106291bb51bf2cc1648df070bef963a0639"
    ),
    f!(
        "tokenizer.json",
        3931381,
        "615928f5a25409279b47b47d87a4ca2aaee3bd09f65e1a3df6c9d23c718cfdb0",
        "3e0df5a329d804fb94bec16abc3867a462d55c08"
    ),
    f!(
        "tokenizer_config.json",
        282873,
        "3c75940dfce3a294fca7041a5faff011677f1b68fa85e47511bb8cf6dccaded6",
        "5bff1ae6346582c59a1999bb0e1784b6642540b4"
    ),
    f!(
        "vocab.json",
        835528,
        "6788c80b082e9b0d1393147d3a3e62ba19285ac0c82ace8e5ef00f37ead58971",
        "b45bc0e97a5511c917c112c42231cdbf455e200d"
    ),
];

pub const MODELS: &[Model] = &[
    Model {
        name: "qwen3-asr-ja",
        role: "refine-b",
        source: "hf:neosophie/Qwen3-ASR-1.7B-JA@987bda160f2dabfa6757550bcff7cdda2ba0648c",
        license: "Apache-2.0",
        files: QWEN_FILES,
    },
    Model {
        name: "kana-whisper",
        role: "refine-k",
        source: "hf:sbintuitions/kana-whisper@88ecb3d79c5846cb4fcf76f4107b84c8fa2acd82",
        license: "MIT",
        files: KANA_FILES,
    },
];

#[derive(Clone, Debug)]
pub struct CoreAsset {
    pub name: &'static str,
    pub role: &'static str,
    pub source: &'static str,
    pub license: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub archive: bool,
    /// 置き場（モデルの置き場からの相対パス）。Python 版と同じ形にして、Python 版で取得済みのものを使う
    pub path: &'static str,
    /// 取得済みかを見るファイル（`path` の中）。空なら `path` 自体がファイル
    pub check: &'static str,
}
pub const CORE_ASSETS: &[CoreAsset] = &[
    CoreAsset { name:"reazonspeech-ja", role:"asr", source:"https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01.tar.bz2", license:"Apache-2.0", size:713097333, sha256:"e0981d0d5d7b446d41010831b59091ebf57d2aa7b79980f67ca37af460b5842d", archive:true, path:"sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01", check:"tokens.txt" },
    CoreAsset { name:"reazonspeech-ja-en", role:"asr", source:"https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17.tar.bz2", license:"Apache-2.0", size:437969761, sha256:"dc03758608c0280e2cbcaac4597467ffcf846ae0b06436f1706738a11da86f5d", archive:true, path:"sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17", check:"tokens.txt" },
    CoreAsset { name:"segmentation", role:"diarizer", source:"https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2", license:"MIT", size:6958444, sha256:"24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488", archive:true, path:"sherpa-onnx-pyannote-segmentation-3-0", check:"model.onnx" },
    CoreAsset { name:"embedding", role:"diarizer", source:"https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx", license:"Apache-2.0", size:28281164, sha256:"aa3cfc16963a10586a9393f5035d6d6b57e98d358b347f80c2a30bf4f00ceba2", archive:false, path:"3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx", check:"" },
    CoreAsset { name:"vad", role:"vad", source:"https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx", license:"MIT", size:643854, sha256:"9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6", archive:false, path:"silero_vad.onnx", check:"" },
];

impl CoreAsset {
    /// モデルの置き場 `root` の中の、この取得物の場所
    pub fn location(&self, root: &Path) -> PathBuf {
        root.join(self.path)
    }

    /// `root` に取得済みか
    pub fn is_present(&self, root: &Path) -> bool {
        let dest = self.location(root);
        if self.check.is_empty() {
            dest.is_file()
        } else {
            dest.join(self.check).is_file()
        }
    }
}

#[derive(Debug)]
pub enum FetchError {
    Offline(String),
    Http(String),
    Io(String),
    Integrity(String),
    Archive(String),
}
impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Offline(s)
                | Self::Http(s)
                | Self::Io(s)
                | Self::Integrity(s)
                | Self::Archive(s) => s,
            }
        )
    }
}
impl std::error::Error for FetchError {}

macro_rules! tls_builder {
    ($builder:expr) => {{
        let mut builder = $builder;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            builder = builder.tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            );
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            builder = builder.tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::Rustls)
                    .build(),
            );
        }
        builder
    }};
}

/// 大きいファイルの取得用。全体の期限は設けず、接続・名前解決・応答ヘッダーの
/// 待ち時間と、1 回の読み取りの停滞（進捗が止まった時間）だけを区切る。
pub fn http_agent_stall(timeout: Duration) -> ureq::Agent {
    let builder = tls_builder!(ureq::config::Config::builder()
        .timeout_resolve(Some(timeout))
        .timeout_connect(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_per_read(Some(timeout))
        .timeout_global(None)
        .timeout_recv_body(None));
    ureq::Agent::new_with_config(builder.build())
}

/// Construct the HTTP agent shared by downloads and runtime diagnostics.
pub fn http_agent(timeout: Duration) -> ureq::Agent {
    let builder = tls_builder!(ureq::config::Config::builder().timeout_global(Some(timeout)));
    ureq::Agent::new_with_config(builder.build())
}

/// GitHub の `releases/latest` の転送先（`/tag/vX.Y.Z`）から版を読む。
/// `api.github.com` を使わないので、回数制限の影響を受けない。
pub fn resolve_latest_tag(url: &str) -> Result<String, FetchError> {
    let agent = http_agent(Duration::from_secs(30));
    let response = agent
        .get(url)
        .call()
        .map_err(|e| FetchError::Http(e.to_string()))?;
    let final_url = response.get_uri().to_string();
    let tag = final_url.rsplit_once("/tag/v").map_or("", |(_, tag)| tag);
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(FetchError::Http(format!(
            "latest release redirect has no version tag: {final_url}"
        )));
    }
    Ok(tag.to_owned())
}

#[derive(Clone, Debug)]
pub struct FetchItem {
    pub name: String,
    pub source: String,
    pub size: u64,
    pub present: bool,
}

/// Files the `setup` command would fetch for its default Japanese pipeline.
pub fn setup_plan() -> Vec<FetchItem> {
    let root = models_root();
    let mut items = Vec::new();
    for asset in CORE_ASSETS.iter().filter(|a| {
        a.name == "reazonspeech-ja"
            || a.name == "segmentation"
            || a.name == "embedding"
            || a.name == "vad"
    }) {
        let present = asset.is_present(&root);
        items.push(FetchItem {
            name: asset.name.into(),
            source: asset.source.into(),
            size: asset.size,
            present,
        });
    }
    let dict = dictionary_root().join("unidic-lite-1.0.8");
    items.push(FetchItem {
        name: "unidic-lite-1.0.8".into(),
        source: DICT_URL.into(),
        size: DICT_SIZE,
        present: dict.join("sys.dic").is_file(),
    });
    items
}

/// Files the `models pull` command would fetch for the default Japanese model.
pub fn models_pull_plan() -> Vec<FetchItem> {
    setup_plan()
        .into_iter()
        .filter(|i| i.name != "unidic-lite-1.0.8")
        .collect()
}

fn model_repo_commit(m: &Model) -> (&str, &str) {
    m.source
        .strip_prefix("hf:")
        .and_then(|s| s.split_once('@'))
        .unwrap_or((m.source, ""))
}

fn model_present(m: &Model, root: &Path) -> bool {
    let (repo, commit) = model_repo_commit(m);
    m.files
        .iter()
        .all(|f| cached_hf(repo, commit, f.path, root).is_some_and(|p| p.is_file()))
}

/// Files the `models pull --refine` command would fetch: B (Qwen3-ASR) and K (kana-whisper).
pub fn refine_pull_plan() -> Vec<FetchItem> {
    let root = cache_root();
    let mut items = Vec::new();
    for m in MODELS {
        let (repo, commit) = model_repo_commit(m);
        for f in m.files {
            let present = cached_hf(repo, commit, f.path, &root).is_some_and(|p| p.is_file());
            items.push(FetchItem {
                name: format!("{}/{}", m.name, f.path),
                source: format!("hf:{repo}@{commit}/{}", f.path),
                size: f.size.unwrap_or(0),
                present,
            });
        }
    }
    items
}

/// Download every pinned refine file that is not cached yet, reusing blobs from
/// existing Hugging Face caches (the Python version's included).
pub fn fetch_refine_with_progress(
    offline_flag: bool,
    mut progress: impl FnMut(&str, u64, Option<u64>),
) -> Result<(), FetchError> {
    for m in MODELS {
        let (repo, commit) = model_repo_commit(m);
        for f in m.files {
            if cached_hf(repo, commit, f.path, &cache_root()).is_some_and(|p| p.is_file()) {
                continue;
            }
            let label = format!("{}/{}", m.name, f.path);
            fetch_hf(
                repo,
                commit,
                f.path,
                f.sha256,
                f.git_sha1,
                f.size,
                offline_flag,
                |done, total| progress(&label, done, total),
            )?;
        }
    }
    Ok(())
}

/// Whether every pinned refine file is cached.
pub fn refine_models_present() -> bool {
    let root = cache_root();
    MODELS.iter().all(|m| model_present(m, &root))
}
fn offline(flag: bool) -> bool {
    flag || std::env::var("HF_HUB_OFFLINE").is_ok_and(|v| v == "1")
}
fn ioe(e: std::io::Error) -> FetchError {
    FetchError::Io(e.to_string())
}

pub use kikitori_core::paths::home_dir;

pub fn models_root() -> PathBuf {
    kikitori_core::paths::models_root()
}
pub fn dictionary_root() -> PathBuf {
    kikitori_core::paths::dictionary_root()
}
pub fn cache_root() -> PathBuf {
    kikitori_core::paths::hf_cache_root()
}

fn link_or_copy(src: &Path, dst: &Path) -> Result<PathBuf, FetchError> {
    if let Some(p) = dst.parent() {
        fs::create_dir_all(p).map_err(ioe)?
    }
    if fs::symlink_metadata(dst).is_ok() {
        fs::remove_file(dst).map_err(ioe)?;
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(src, dst).is_ok() {
            return Ok(dst.to_path_buf());
        }
        if fs::hard_link(src, dst).is_ok() {
            return Ok(dst.to_path_buf());
        }
        // シンボリックリンクの権限が無い Windows でも、照合済みのキャッシュをその場で読む
        Ok(src.to_path_buf())
    }
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(src, dst).is_ok() {
            return Ok(dst.to_path_buf());
        }
        fs::copy(src, dst).map_err(ioe)?;
        Ok(dst.to_path_buf())
    }
}
/// ファイルの SHA-256（小文字の 16 進）。
pub fn sha256_file(path: &Path) -> Result<String, FetchError> {
    let mut f = fs::File::open(path).map_err(ioe)?;
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b).map_err(ioe)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
fn git_sha1_file(path: &Path) -> Result<String, FetchError> {
    use sha1::Sha1;
    let mut f = fs::File::open(path).map_err(ioe)?;
    let size = f.metadata().map_err(ioe)?.len();
    let mut h = Sha1::new();
    h.update(format!("blob {size}\0").as_bytes());
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b).map_err(ioe)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
fn verify(
    path: &Path,
    sha: Option<&str>,
    git: Option<&str>,
    size: Option<u64>,
) -> Result<bool, FetchError> {
    if size.is_some_and(|n| fs::metadata(path).map(|m| m.len() != n).unwrap_or(true)) {
        return Ok(false);
    }
    if let Some(s) = sha {
        if sha256_file(path)? != s {
            return Ok(false);
        }
    }
    if let Some(s) = git {
        if git_sha1_file(path)? != s {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn cached_hf(repo: &str, commit: &str, file: &str, root: &Path) -> Option<PathBuf> {
    let p = root
        .join(format!(
            "models--{}--{}",
            repo.split_once('/').map(|x| x.0).unwrap_or(""),
            repo.split_once('/').map(|x| x.1).unwrap_or(repo)
        ))
        .join("snapshots")
        .join(commit)
        .join(file);
    p.is_file().then_some(p)
}

#[allow(clippy::too_many_arguments)]
pub fn fetch_hf(
    repo: &str,
    commit: &str,
    file: &str,
    sha: Option<&str>,
    git: Option<&str>,
    size: Option<u64>,
    offline_flag: bool,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, FetchError> {
    let root = cache_root();
    let dir = root.join(format!("models--{}", repo.replace('/', "--")));
    let snapshot = dir.join("snapshots").join(commit).join(file);
    if verify(&snapshot, sha, git, size).unwrap_or(false) {
        return Ok(snapshot);
    }
    // Reuse content in older snapshots when its digest is correct.
    if let Ok(commits) = fs::read_dir(dir.join("snapshots")) {
        for old in commits.flatten() {
            let c = old.path().join(file);
            if c.is_file() && verify(&c, sha, None, size).unwrap_or(false) {
                return link_or_copy(&c, &snapshot);
            }
        }
    }
    // HF blob names are LFS SHA-256 or git blob SHA-1. Verify both the name and content.
    if let Ok(blobs) = fs::read_dir(dir.join("blobs")) {
        for e in blobs.flatten() {
            let p = e.path();
            let n = p.file_name().and_then(|x| x.to_str()).unwrap_or("");
            let named = sha.is_some_and(|s| s == n) || git.is_some_and(|s| s == n);
            if p.is_file() && named && verify(&p, sha, git, size).unwrap_or(false) {
                return link_or_copy(&p, &snapshot);
            }
        }
    }
    if offline(offline_flag) {
        return Err(FetchError::Offline(format!(
            "{repo}/{file} is not cached; rerun without --offline to download it"
        )));
    }
    let url = format!("https://huggingface.co/{repo}/resolve/{commit}/{file}");
    let tmp = unique_temp(&snapshot);
    download(&url, &tmp, size, sha, offline_flag, progress)?;
    let blob_name = sha
        .or(git)
        .ok_or_else(|| FetchError::Integrity("HF files require a pinned blob digest".into()))?;
    let blob = dir.join("blobs").join(blob_name);
    fs::create_dir_all(blob.parent().unwrap()).map_err(ioe)?;
    replace_file(&tmp, &blob)?;
    let actual_snapshot = link_or_copy(&blob, &snapshot)?;
    fs::create_dir_all(dir.join("refs")).map_err(ioe)?;
    let reference = dir.join("refs/main");
    if !reference.exists() {
        let mut f = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(reference)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(actual_snapshot),
            Err(e) => return Err(ioe(e)),
        };
        f.write_all(commit.as_bytes()).map_err(ioe)?
    }
    // Store only this downloaded snapshot path, so removal cannot erase other cache entries.
    record_receipt("hf-repo", &snapshot.display().to_string())?;
    Ok(actual_snapshot)
}

static TEMP_ID: AtomicU64 = AtomicU64::new(0);
fn unique_temp(dest: &Path) -> PathBuf {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    dest.with_file_name(format!(".{name}.{}.{}.part", std::process::id(), id))
}
fn replace_file(from: &Path, to: &Path) -> Result<(), FetchError> {
    if to.exists() {
        fs::remove_file(to).map_err(ioe)?
    }
    fs::rename(from, to).map_err(ioe)
}

/// 本文の転送が止まったとみなす時間。全体の期限は設けない（大きいモデルを落とせるように）。
const DOWNLOAD_STALL: Duration = Duration::from_secs(60);
const DOWNLOAD_ATTEMPTS: u32 = 4;

pub fn download(
    url: &str,
    dest: &Path,
    size: Option<u64>,
    sha: Option<&str>,
    offline_flag: bool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), FetchError> {
    if offline(offline_flag) {
        return Err(FetchError::Offline(format!(
            "{url} is not cached; rerun without --offline to download it"
        )));
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(ioe)?
    }
    // Each call owns a unique partial file. Keeping it across attempts allows byte-range resume.
    let part = unique_temp(dest);
    let mut offset = 0u64;
    // このファイルでこれまでに到達した最大バイト数。Range を無視するサーバーに
    // 200 が返ると offset は 0 に戻るので、進んだかどうかはこの値で判断する
    let mut high_water = 0u64;
    // 進んだ回は数え直す。進まない試行だけを最大 4 回に制限する
    let mut attempts_without_progress = 0u32;
    loop {
        attempts_without_progress += 1;
        let agent = http_agent_stall(DOWNLOAD_STALL);
        let mut req = agent.get(url);
        if offset > 0 {
            req = req.header("Range", &format!("bytes={offset}-"))
        }
        let response = match req.call() {
            Ok(r) => r,
            Err(e) => {
                if attempts_without_progress >= DOWNLOAD_ATTEMPTS {
                    let _ = fs::remove_file(&part);
                    return Err(FetchError::Http(e.to_string()));
                }
                continue;
            }
        };
        // A redirect may drop the Range header. If a resumed request returns 200, restart at zero.
        let append = offset > 0 && response.status() == 206;
        if !append {
            offset = 0
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part)
            .map_err(ioe)?;
        let mut reader = response.into_body().into_reader();
        let mut buf = [0u8; 65536];
        let mut done = offset;
        let mut interrupted = false;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    f.write_all(&buf[..n]).map_err(ioe)?;
                    done += n as u64;
                    progress(done, size)
                }
                Err(_) => {
                    interrupted = true;
                    break;
                }
            }
        }
        f.flush().map_err(ioe)?;
        let progressed = done > high_water;
        if progressed {
            high_water = done;
        }
        if interrupted {
            offset = done;
            if progressed {
                attempts_without_progress = 0;
            }
            if attempts_without_progress >= DOWNLOAD_ATTEMPTS {
                let _ = fs::remove_file(&part);
                return Err(FetchError::Http(
                    "connection interrupted after retries".into(),
                ));
            }
            continue;
        }
        if size.is_some_and(|n| n != done) {
            offset = done;
            if progressed {
                attempts_without_progress = 0;
            }
            if attempts_without_progress >= DOWNLOAD_ATTEMPTS {
                let _ = fs::remove_file(&part);
                return Err(FetchError::Integrity(format!(
                    "short download: expected {}, got {done}",
                    size.unwrap()
                )));
            }
            continue;
        }
        if !verify(&part, sha, None, size)? {
            let _ = fs::remove_file(&part);
            if attempts_without_progress < DOWNLOAD_ATTEMPTS {
                offset = 0;
                continue;
            }
            return Err(FetchError::Integrity(format!(
                "checksum mismatch for {url}"
            )));
        }
        replace_file(&part, dest)?;
        return Ok(());
    }
}

pub fn receipt_path() -> PathBuf {
    kikitori_core::paths::config_dir().join("receipt.txt")
}
pub fn record_receipt(kind: &str, location: &str) -> Result<(), FetchError> {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK
        .lock()
        .map_err(|_| FetchError::Io("receipt lock poisoned".into()))?;
    let path = receipt_path();
    fs::create_dir_all(path.parent().unwrap()).map_err(ioe)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(ioe)?;
    f.write_all(format!("{kind}\t{location}\n").as_bytes())
        .map_err(ioe)
}

pub fn fetch_core(root: &Path, model: &str, offline_flag: bool) -> Result<(), FetchError> {
    fetch_core_with_progress(root, model, offline_flag, |_, _, _| {})
}

pub fn fetch_core_with_progress(
    root: &Path,
    model: &str,
    offline_flag: bool,
    mut progress: impl FnMut(&str, u64, Option<u64>),
) -> Result<(), FetchError> {
    fs::create_dir_all(root).map_err(ioe)?;
    let asr = if model == "ja-en" {
        "reazonspeech-ja-en"
    } else {
        "reazonspeech-ja"
    };
    for a in CORE_ASSETS.iter().filter(|a| {
        a.name == asr || a.name == "segmentation" || a.name == "embedding" || a.name == "vad"
    }) {
        let dest = a.location(root);
        if a.is_present(root) {
            continue;
        }
        let tmp = unique_temp(&dest);
        download(
            a.source,
            &tmp,
            Some(a.size),
            Some(a.sha256),
            offline_flag,
            |done, total| progress(a.name, done, total),
        )?;
        if a.archive {
            let decoder = bzip2::read::BzDecoder::new(fs::File::open(&tmp).map_err(ioe)?);
            tar::Archive::new(decoder)
                .unpack(root)
                .map_err(|e| FetchError::Archive(e.to_string()))?;
            fs::remove_file(&tmp).map_err(ioe)?
        } else {
            replace_file(&tmp, &dest)?
        }
        record_receipt("models-dir", &dest.display().to_string())?;
    }
    Ok(())
}

pub const DICT_URL:&str="https://files.pythonhosted.org/packages/55/2b/8cf7514cb57d028abcef625afa847d60ff1ffbf0049c36b78faa7c35046f/unidic-lite-1.0.8.tar.gz";
pub const DICT_SIZE: u64 = 47356746;
pub const DICT_SHA256: &str = "db9d4572d9fdd4d00a97949d4b0741ec480ee05a7e7e2e32f547500dae27b245";
/// unidic-lite の sdist（tar.gz）から dicdir を取り出し、中身を変えずに `target` に置く
pub fn extract_dictionary(archive: &Path, target: &Path) -> Result<(), FetchError> {
    let staging = unique_temp(target);
    let placed = (|| {
        fs::create_dir_all(&staging).map_err(ioe)?;
        let decoder = flate2::read::GzDecoder::new(fs::File::open(archive).map_err(ioe)?);
        tar::Archive::new(decoder)
            .unpack(&staging)
            .map_err(|e| FetchError::Archive(e.to_string()))?;
        // sdist の中は unidic-lite-1.0.8/unidic_lite/dicdir
        let extracted = staging.join("unidic-lite-1.0.8/unidic_lite/dicdir");
        if !extracted.join("sys.dic").is_file() {
            return Err(FetchError::Archive(
                "unidic-lite の配布物に dicdir/sys.dic がありません".into(),
            ));
        }
        fs::rename(extracted, target).map_err(ioe)
    })();
    // 成否にかかわらず、展開の作業場所を片付ける
    let _ = fs::remove_dir_all(&staging);
    placed
}

pub fn fetch_dictionary(root: &Path, offline_flag: bool) -> Result<(), FetchError> {
    fetch_dictionary_with_progress(root, offline_flag, |_, _| {})
}

pub fn fetch_dictionary_with_progress(
    root: &Path,
    offline_flag: bool,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<(), FetchError> {
    let target = root.join("unidic-lite-1.0.8");
    if target.join("sys.dic").is_file() {
        return Ok(());
    }
    fs::create_dir_all(root).map_err(ioe)?;
    let tmp = unique_temp(&target);
    download(
        DICT_URL,
        &tmp,
        Some(DICT_SIZE),
        Some(DICT_SHA256),
        offline_flag,
        progress,
    )?;
    let placed = extract_dictionary(&tmp, &target);
    let _ = fs::remove_file(&tmp);
    placed?;
    record_receipt("dict", &target.display().to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests;
