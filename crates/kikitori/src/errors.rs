use kikitori_core::{CliError, ErrorKind};

/// Convert command failures into the stable CLI error taxonomy.
pub fn from_message(message: String) -> CliError {
    let low = message.to_ascii_lowercase();
    let (kind, message, hint) = if low.contains("not found") || low.contains("no such file") {
        (
            ErrorKind::Input,
            "指定したファイルが見つからないか、読み込めません。".to_owned(),
            "ファイルのパスと読み取り権限を確認してください。",
        )
    } else if low.contains("ffmpeg") {
        (
            ErrorKind::Dependency,
            "音声処理に必要な ffmpeg を利用できません。".to_owned(),
            "ffmpeg をインストールし、PATH から実行できることを確認してください。",
        )
    } else if low.contains("permission denied") || low.contains("operation not permitted") {
        (
            ErrorKind::Privilege,
            "必要な操作を実行する権限がありません。".to_owned(),
            "必要な権限を確認してからコマンドを再実行してください。",
        )
    } else if low.contains("out of memory") || low.contains("insufficient memory") {
        (
            ErrorKind::Memory,
            "処理に必要なメモリが不足しています。".to_owned(),
            "他のアプリを終了してから再実行してください。",
        )
    } else if kikitori_core::error::is_storage_full(&message) {
        (
            ErrorKind::Processing,
            "ディスクの空きが足りません。".to_owned(),
            "ディスクを空けてから、コマンドを再実行してください。",
        )
    } else if low.contains("download") {
        (
            ErrorKind::Download,
            "ダウンロードに失敗しました。".to_owned(),
            "ネットワーク接続を確認し、`kikitori setup` を再実行してください。",
        )
    } else {
        (
            ErrorKind::Processing,
            "処理に失敗しました。".to_owned(),
            "入力と実行環境を確認し、コマンドを再実行してください。",
        )
    };
    CliError::new(kind, message, hint)
}
