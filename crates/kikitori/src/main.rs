mod capture;
mod commands;
mod errors;
mod log;
mod receipt;

use clap::{Args, Parser, Subcommand, ValueEnum};
use commands::*;
use kikitori_core::{CliError, ErrorKind};
use std::process::ExitCode as ProcessExitCode;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum LogFormat {
    Human,
    Json,
}

#[derive(Clone, Debug, Args)]
struct GlobalArgs {
    #[arg(
        long,
        global = true,
        value_enum,
        env = "KIKITORI_LOG_FORMAT",
        default_value = "human",
        help = "診断ログの形式"
    )]
    log_format: LogFormat,
    #[arg(long, global = true, default_value_t = yes_from_env(), help = "確認を省略する")]
    yes: bool,
    #[arg(long, global = true, help = "実行内容だけを表示し、変更しない")]
    dry_run: bool,
    #[arg(long, global = true, default_value_t = offline_from_env(), help = "ネットワークから取得しない")]
    offline: bool,
    #[arg(short, long, alias = "verbose", global = true, action = clap::ArgAction::Count, conflicts_with = "quiet", help = "詳細なログを表示する")]
    verbose: u8,
    #[arg(
        long,
        global = true,
        conflicts_with = "verbose",
        help = "情報ログを表示しない"
    )]
    quiet: bool,
}

fn yes_from_env() -> bool {
    std::env::var("KIKITORI_YES").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}
fn offline_from_env() -> bool {
    std::env::var("HF_HUB_OFFLINE").is_ok_and(|v| v == "1")
}

#[derive(Parser, Debug)]
#[command(
    name = "kikitori",
    version,
    about = "話者ラベル付きの日本語会議文字起こし",
    long_about = "話者ラベル付きの日本語会議文字起こし\n\n終了コード:\n  0  正常終了\n  2  引数の誤り\n  3  入力がない、読めない、または形式が不正\n  4  必要な依存ソフトがない\n  5  オフラインでモデルを利用できない\n  6  確認が必要（--yes を指定）\n  7  ダウンロード失敗\n  8  処理失敗\n  9  補正用モデルを利用できない\n 10  管理者権限が必要\n 11  メモリ不足（--force で続行）"
)]
struct Cli {
    #[command(flatten)]
    global: GlobalArgs,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "音声を文字起こしする")]
    Transcribe(Box<TranscribeArgs>),
    #[command(about = "文字起こしを補正する")]
    Refine(Box<RefineArgs>),
    #[command(about = "実行環境とモデルを診断する")]
    Doctor {
        #[arg(long, help = "診断結果を JSON で出力する")]
        json: bool,
    },
    #[command(about = "モデルを一覧表示、取得、削除する")]
    Models {
        #[command(subcommand)]
        action: ModelAction,
    },
    #[command(about = "利用可能な認識エンジンを一覧表示する")]
    Engines {
        #[command(subcommand)]
        action: EngineAction,
    },
    #[command(about = "必要なモデルと辞書を準備する")]
    Setup,
    #[command(about = "kikitori を更新する")]
    Update {
        /// 入れる版（例: 0.3.1）。省くと最新版
        #[arg(long)]
        version: Option<String>,
        /// CUDA 版（cuda）と通常版（cpu）を替える
        #[arg(long, value_parser = ["cuda", "cpu"])]
        variant: Option<String>,
    },
    #[command(about = "kikitori を削除する")]
    Uninstall {
        /// 取得したモデルと辞書を消さずに残す
        #[arg(long)]
        keep_models: bool,
    },
    #[command(about = "エージェント向けスキルを管理する")]
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    #[command(about = "出力形式の JSON Schema を表示する")]
    Schema {
        #[arg(value_parser = ["transcript", "transcript-v1", "bundle", "doctor", "engines"], help = "表示するスキーマ名")]
        name: String,
    },
}

#[derive(Args, Debug)]
struct TranscribeArgs {
    #[arg(help = "文字起こしする音声ファイル")]
    input: Option<String>,
    #[arg(long, hide = true, help = "旧形式: モデルを準備する")]
    setup: bool,
    #[arg(long, hide = true, help = "旧形式: モデルの保存先")]
    models: Option<String>,
    #[arg(long, default_value = "ja", value_parser = ["ja", "ja-en"], help = "認識モデル（ja または ja-en）")]
    model: String,
    #[arg(long, default_value = "auto", help = "計算デバイス")]
    device: String,
    #[arg(long, default_value = "fp32", value_parser = ["int8", "fp16", "fp32"], help = "モデルの数値精度")]
    precision: String,
    #[arg(long, default_value_t = 4, help = "スレッド数")]
    threads: usize,
    #[arg(long = "num-speakers", default_value_t = -1, help = "話者数（-1 は自動判定）")]
    num_speakers: i32,
    #[arg(long, default_value_t = 0.6, help = "話者判定のしきい値")]
    threshold: f64,
    #[arg(long = "dict", help = "ユーザー辞書ファイル")]
    dictionary: Option<String>,
    #[arg(long, default_value = "txt", value_parser = ["txt", "json", "json-v1", "yaml", "srt", "vtt"], help = "出力形式")]
    format: String,
    #[arg(long, help = "音声認識エンジン")]
    asr: Option<String>,
    #[arg(long, help = "話者分離エンジン")]
    diarizer: Option<String>,
    #[arg(long, help = "Python 版互換の区間重複動作を使う")]
    python_compat: bool,
}

#[derive(Args, Debug)]
struct RefineArgs {
    #[arg(help = "補正する音声ファイル")]
    input: Option<String>,
    #[arg(
        long,
        help = "メモリが足りない見込みでも続ける（終了コード 11 で止めない）"
    )]
    force: bool,
    #[arg(
        long,
        help = "補正のモデルを取得する: Hugging Face のモデルと、動いている Ollama にこの PC に合うモデル（Ollama は入れない）"
    )]
    setup: bool,
    #[arg(long, help = "--setup で、Hugging Face のモデルを先に取得しない")]
    no_hf: bool,
    #[arg(long, value_parser = ["apple", "nvidia", "cpu"], help = "機器の判定を上書きする（環境変数 KIKITORI_HARDWARE でも可）")]
    hardware: Option<String>,
    #[arg(
        long,
        default_value = "ja",
        help = "認識のモデル。補正は日本語（ja）だけに対応する"
    )]
    model: String,
    #[arg(
        long,
        help = "用語集（1 行 1 語、表記<TAB>補足）。固有名詞を LLM に教える"
    )]
    glossary: Option<String>,
    #[arg(long, default_value = "off", value_parser = ["off", "bundle", "claude"], help = "外部 LLM で直す。bundle: プロンプトを JSON で出し、呼び出し元の LLM が直す / claude: claude CLI に渡す")]
    external: String,
    #[arg(
        long,
        help = "--external bundle で出した JSON に、LLM の答え（stdin か --reply）を戻す"
    )]
    apply: Option<String>,
    #[arg(long, help = "--apply で戻す答えのファイル（省略すると stdin）")]
    reply: Option<String>,
    #[arg(long = "no-ollama", help = "Ollama を使わない")]
    no_ollama: bool,
    #[arg(
        long = "merge-model",
        alias = "llm",
        env = "KIKITORI_LLM",
        help = "突き合わせに使う Ollama のモデル（既定: この PC に合うものを表から選ぶ）"
    )]
    merge_model: Option<String>,
    #[arg(long, help = "どの経路で補正するかだけを出して終える")]
    plan: bool,
    #[arg(long = "save-evidence", help = "A・B・K を区間ごとに JSON で保存する")]
    save_evidence: Option<String>,
    #[arg(long, help = "--save-evidence で保存した A・B・K を使い、認識を省く")]
    evidence: Option<String>,
    #[arg(long, hide = true, help = "旧形式: モデルの保存先")]
    models: Option<String>,
    #[arg(long, default_value = "auto", value_parser = ["auto", "cpu", "metal", "cuda"], help = "計算デバイス")]
    device: String,
    #[arg(long, default_value = "auto", value_parser = ["auto", "int8", "fp16", "fp32", "bf16"], help = "モデルの数値精度")]
    dtype: String,
    #[arg(long, default_value_t = 4, help = "スレッド数")]
    threads: usize,
    #[arg(long = "num-speakers", default_value_t = -1, help = "話者数（-1 は自動判定）")]
    num_speakers: i32,
    #[arg(long, default_value_t = 0.6, help = "話者判定のしきい値")]
    threshold: f64,
    #[arg(long = "dict", help = "kikitori の辞書（1 行 1 語、表記<TAB>読み）")]
    dict: Option<String>,
    #[arg(long, default_value = "txt", value_parser = ["txt", "json", "json-v1", "yaml", "srt", "vtt"], help = "txt: 1 行 1 発言 / json・yaml: LLM や道具で読む / srt・vtt: 字幕")]
    format: String,
    #[arg(
        long = "engine-b",
        help = "詳しい書き起こしに使う認識エンジンを上書きする"
    )]
    engine_b: Option<String>,
    #[arg(long = "engine-k", help = "読み（カナ）に使う認識エンジンを上書きする")]
    engine_k: Option<String>,
}

#[derive(Subcommand, Debug)]
enum ModelAction {
    #[command(about = "インストール済みモデルを一覧表示する")]
    List,
    #[command(about = "必要なモデルを取得する")]
    Pull {
        #[arg(long, help = "中核（文字起こし）のモデルを取得する（既定）")]
        core: bool,
        #[arg(long, help = "補正に使う B（Qwen3-ASR）と K（kana-whisper）を取得する")]
        refine: bool,
    },
    #[command(about = "kikitori が取得したモデルを削除する")]
    Remove,
    #[command(about = "モデルとキャッシュの保存先を表示する")]
    Path,
}
#[derive(Subcommand, Debug)]
enum EngineAction {
    #[command(about = "利用可能な認識エンジンを一覧表示する")]
    List,
}
#[derive(Subcommand, Debug)]
enum SkillAction {
    #[command(about = "エージェント用スキルをインストールする")]
    Install {
        #[arg(long, value_parser = ["claude", "codex"], default_value = "claude")]
        agent: String,
        #[arg(long)]
        dir: Option<String>,
    },
    #[command(about = "エージェント用スキルを削除する")]
    Uninstall {
        #[arg(long, value_parser = ["claude", "codex"], default_value = "claude")]
        agent: String,
        #[arg(long)]
        dir: Option<String>,
    },
    #[command(about = "エージェント用スキルの保存先を表示する")]
    Path {
        #[arg(long, value_parser = ["claude", "codex"], default_value = "claude")]
        agent: String,
        #[arg(long)]
        dir: Option<String>,
    },
}

fn main() -> ProcessExitCode {
    capture::release_pre_main();
    let mut args: Vec<String> = std::env::args().collect();
    let json_logs = args
        .windows(2)
        .any(|pair| pair[0] == "--log-format" && pair[1] == "json")
        || args.iter().any(|a| a == "--log-format=json")
        || std::env::var("KIKITORI_LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("json"));
    kikitori_core::notice::set_sink(|kind, message| {
        if kind == "warning" {
            log::warning(message);
        } else {
            log::info(message);
        }
    });
    std::panic::set_hook(Box::new(move |panic| {
        let _ = json_logs;
        if kikitori_core::probe::active() {
            return;
        }
        capture::panic_error(&panic.to_string());
        std::process::exit(8);
    }));
    let invoked = args
        .first()
        .and_then(|s| std::path::Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("kikitori")
        .to_owned();
    if invoked == "kikitori-refine" && !args.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        log::configure(json_logs, false, false);
        capture::replay_pre_main();
        log::warning("kikitori-refine は旧コマンド名です。`kikitori refine` を使用してください。");
        args.insert(1, "refine".to_owned());
    }
    let known = [
        "transcribe",
        "refine",
        "doctor",
        "models",
        "engines",
        "setup",
        "update",
        "uninstall",
        "skill",
        "schema",
        "help",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    // 共通の指定（`--yes` など）がサブコマンドより前に来てもよいので、最初の指定でない引数で判断する
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') && !known.contains(&args[i].as_str()) {
        i += if args[i] == "--log-format" { 2 } else { 1 };
    }
    if args
        .get(i)
        .is_some_and(|arg| !known.contains(&arg.as_str()))
    {
        args.insert(1, "transcribe".to_owned());
    }
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let help = error
                .to_string()
                .replace("Usage:", "使い方:")
                .replace("Commands:", "コマンド:")
                .replace("Options:", "オプション:")
                .replace("Arguments:", "引数:")
                .replace(
                    "Print this message or the help of the given subcommand(s)",
                    "このヘルプ、または指定したサブコマンドのヘルプを表示する",
                )
                .replace(
                    "Print help (see a summary with '-h')",
                    "このヘルプを表示する（概要は -h）",
                )
                .replace("Print help", "このヘルプを表示する")
                .replace("Print version", "バージョンを表示する")
                .replace("[env:", "[環境変数:")
                .replace("[default:", "[既定値:")
                .replace("[possible values:", "[指定できる値:");
            print!("{help}");
            return ProcessExitCode::SUCCESS;
        }
        Err(error) => {
            log::configure(json_logs, false, false);
            capture::replay_pre_main();
            let _ = error;
            log::error(
                "usage",
                "引数の指定が正しくありません。",
                "使用できる引数は `kikitori --help` で確認してください。",
            );
            return ProcessExitCode::from(2);
        }
    };
    log::configure_with_yes(
        matches!(cli.global.log_format, LogFormat::Json),
        cli.global.verbose > 0,
        cli.global.quiet,
        cli.global.yes,
    );
    capture::replay_pre_main();
    let is_transcribe = matches!(&cli.command, Command::Transcribe(_));
    let yes = cli.global.yes;
    let run = || match cli.command {
        Command::Transcribe(args) if args.setup => {
            log::warning("--setup は旧形式です。`kikitori setup` を使用してください。");
            setup::run(yes, cli.global.dry_run, cli.global.offline)
        }
        Command::Transcribe(mut args) => {
            if args.models.is_some() {
                log::warning("--models は旧形式です。モデルの置き場は環境変数 KIKITORI_MODELS で指定してください。");
            }
            let legacy = match args.device.as_str() {
                "coreml" | "directml" | "migraphx" | "rocm" => Some("auto"),
                "sherpa" => Some("cpu"),
                _ => None,
            };
            if let Some(mapped) = legacy {
                log::warning(&format!(
                    "--device {} は旧形式です。--device {mapped} を使用します。",
                    args.device
                ));
                args.device = mapped.to_owned();
            }
            if args.input.is_none() {
                Err(CliError::new(
                    ErrorKind::Input,
                    "音声ファイルを指定してください。",
                    "`kikitori transcribe <音声ファイル>` を実行してください。",
                ))
            } else if !std::path::Path::new(args.input.as_deref().unwrap_or_default()).exists() {
                Err(CliError::new(
                    ErrorKind::Input,
                    format!(
                        "入力ファイルが見つかりません: {}",
                        args.input.as_deref().unwrap_or_default()
                    ),
                    "ファイルのパスを確認して、もう一度実行してください。",
                ))
            } else {
                transcribe::run(*args, yes, cli.global.offline)
            }
        }
        Command::Refine(args) => refine::run(&args, yes, cli.global.dry_run, cli.global.offline),
        Command::Doctor { json } => doctor::run(json).map_err(errors::from_message),
        Command::Models { action } => {
            models::run(action, yes, cli.global.dry_run, cli.global.offline)
        }
        Command::Engines { action } => engines::run(action).map_err(errors::from_message),
        Command::Setup => setup::run(yes, cli.global.dry_run, cli.global.offline),
        Command::Update { version, variant } => {
            update::run(version, variant).map_err(errors::from_message)
        }
        Command::Uninstall { keep_models } => uninstall::run(keep_models, yes, cli.global.dry_run),
        Command::Skill { action } => skill::run(action).map_err(errors::from_message),
        Command::Schema { name } => schema::run(&name).map_err(errors::from_message),
    };
    let result = if is_transcribe {
        run()
    } else {
        capture::capture_stderr(run)
    };
    match result {
        Ok(()) => ProcessExitCode::SUCCESS,
        Err(error) => {
            let exit = error.kind.exit_code();
            log::error(exit.name(), &error.message, &error.hint);
            ProcessExitCode::from(exit.code() as u8)
        }
    }
}
