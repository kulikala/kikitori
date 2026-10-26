# リリースの手順

kikitori の版は `Cargo.toml` の `[workspace.package]` の `version` だけで決まります。
`scripts/build-release.sh`・`build-release.ps1` が資産の名前をこの値から作り、もらうタグもこの値で照合します。
リリースは 2 段です。手元で版を上げたコミットとタグを作ってタグを GitHub に push すると、GitHub Actions が資産をそろえた Draft の Release を作ります。中身を確かめた人が手動で公開します。

手順には `git`・`gh`（GitHub の CLI）・`cargo`・`docker`（`actionlint` 用）を使います。

## 版の付け方

[Semantic Versioning](https://semver.org/lang/ja/) に従います。0.x の間は、利用者の手順や出力が変わる変更で minor を上げ、それ以外は patch を上げます。
リリースとリリースの間の `version` は、次に出す予定の版に `-dev` を付けた値（例: 0.3.0 を出したあとは `0.4.0-dev`。Cargo の版は semver なので `.dev0` は使えない）にしておきます。

## 1. 変更点を書く

変更を入れるたびに、`CHANGELOG.md` の `## [Unreleased]` の下に書きためます。見出しは `### Added`・`### Changed`・`### Fixed`・`### Removed` を使います。
文書の書き方は [WRITING.md](WRITING.md) に従います。

## 2. 手元で検査して版を上げる

`CI`（.github/workflows/ci.yml）と同じ検査を手元でも通し、版を上げて CHANGELOG の `[Unreleased]` を `## [0.3.0] - 今日の日付` の節に移してコミットします。

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace      # モデルが要るテストは無視されます（--ignored で明示的に回せます）
docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest
cargo publish --workspace --dry-run --allow-dirty   # crates.io に出せる形か（送らない）
```

Linux は Docker、Windows は VM で、同じ検査と資産づくりを手元で通します。GitHub Actions は公開の作業（資産を作って Release に載せる）に使い、確かめるための試し打ちには使いません。macOS ではリリースと同じスクリプトで手元の資産も作れます（後述の「手元で資産を作る」）。

`cargo publish --dry-run` を繰り返すと、前の回に検証で作ったものが使い回され、新しく足した関数が見つからないと出ることがあります。そのときは `target/debug/deps/` の `libkikitori_*` と、`~/.cargo/registry/src/` の中の、名前が `-` で始まる置き場（検証用に展開したもの）を消してから打ち直します。

## 3. タグを作って push する

```bash
git commit -m "Release 0.3.0" Cargo.toml CHANGELOG.md
git tag v0.3.0
git push origin main v0.3.0
```

タグの push で `.github/workflows/release.yml` が動きます。タグが `Cargo.toml` の版と一致し、CHANGELOG にその版の節があることを確かめてから、7 つの資産を並行で組みます。

## 4. Draft の Release を確かめて公開する

Actions が終わると Draft の Release に次が揃っています（公開はされません）。

| 資産 | 備考 |
| --- | --- |
| `kikitori-0.3.0-aarch64-apple-darwin.tar.gz` | macOS の通常版。candle に `metal` と `accelerate` を入れる |
| `kikitori-0.3.0-x86_64-unknown-linux-gnu.tar.gz` | Linux の通常版（CPU）。glibc 2.34 で動く（Debian 12 のコンテナで起動確認まで済み） |
| `kikitori-0.3.0-aarch64-unknown-linux-gnu.tar.gz` | 同上の arm64 |
| `kikitori-0.3.0-x86_64-pc-windows-msvc.zip` | Windows の通常版（CPU）。CRT は静的 |
| `kikitori-0.3.0-aarch64-pc-windows-msvc.zip` | 同上の arm64。VS 2026（v18）と clang の部品が要る（「Windows arm64 に必要な Visual Studio 2026」） |
| `kikitori-0.3.0-x86_64-unknown-linux-gnu-cuda.tar.gz` | Linux x86_64 の CUDA 版。cudarc が libcuda を実行時に読むので、GPU の無い機械でも起動する |
| `kikitori-0.3.0-x86_64-pc-windows-msvc-cuda.zip` | Windows x64 の CUDA 版。同上 |
| `SHA256SUMS` | 上のすべての SHA-256 |
| `install.sh`・`install.ps1` | 導入スクリプト（このタグの資産を選ぶ。最新版の解決は `/releases/latest` の転送先に従う） |

各資産は binary・LICENSE・README.md・THIRD-PARTY-NOTICES を含み、`scripts/build-release.sh`・`build-release.ps1` が解凍した直後に `--version` と `doctor --json` を解凍先の実行ファイルで通してから詰めます。THIRD-PARTY-NOTICES は依存関係のライセンス（cargo metadata）に、静的にリンクされる sherpa-onnx（Apache-2.0）とその中の ONNX Runtime（MIT）の注記を添えたものです。

不足や壊れが無ければ公開します。

```bash
gh release edit v0.3.0 --draft=false
```

公開したあと、`Cargo.toml` の `version` を次に出す予定の版の `-dev` に上げ、CHANGELOG に新しい `[Unreleased]` の節を足して push します。

## 手元で資産を作る

リリースと同じビルドのスクリプトは手元の環境でもそのまま動かせます（スモークテスト込み）。

```bash
./scripts/build-release.sh aarch64-apple-darwin
```

`dist/` に `kikitori-<版>-<target>[.tar.gz|.zip]` と `SHA256SUMS` ができます（Windows の資産は Windows 上で `build-release.ps1` を使う）。
エンジンの実装が一部しか入っていない段階でビルドの経路と詰め方を確かめるときは、`KIKITORI_ALLOW_MISSING_FEATURES=1` で、指定した feature が `kikitori` クレートにまだ無くても除外して回します（正式なリリースでは渡さない）。

sherpa-onnx-sys は、ビルドのたびに対応するアーカイブを sherpa-onnx の GitHub Release から取得します。`SHERPA_ONNX_ARCHIVE_DIR` を渡すと取得せず、そのフォルダにアーカイブが無ければビルドが止まるので、手元でも workflow でも渡しません。

Rust は 1.94.0 を使います。workflow もこの版に固定してあり、手元の検査と同じ clippy で確かめます。

## CUDA 版を手元で作る

Linux x86_64 の CUDA 版は、`nvidia/cuda:12.6.3-devel-ubuntu22.04` のコンテナで作ります（GPU は要りません）。build-essential・pkg-config・cmake・git・jq・curl と Rust を入れてから、次を打ちます。compute capability は 8.0、CUDA のランタイムは静的にリンクされます。

```bash
./scripts/build-release.sh x86_64-unknown-linux-gnu --cuda
```

Windows x64 の CUDA 版は、nvcc・CCCL・cudart を NVIDIA の部品ごとの配布物から取り、Visual Studio の MSVC 14.44（VS 2022 の v143。CUDA 12.6 の nvcc はこれより新しい MSVC の標準ライブラリを通さない）で作ります。VS 2026 のときは、Build Tools に「MSVC v143 x64/x86 build tools（14.44）」を足します。arm64 の Windows では、x64 の nvcc がエミュレーションで動きます。

```powershell
$env:CUDA_PATH = ./scripts/fetch-cuda-windows.ps1
./scripts/build-release.ps1 -Target x86_64-pc-windows-msvc -Cuda
```

どちらも、GPU の無い PC で `kikitori doctor` が「cuda: このビルドに入っているが、この PC では使えない」と出せば、起動と CPU への切り替えまで確かめられたことになります。

## Windows arm64 に必要な Visual Studio 2026

sherpa-onnx の win-arm64 の静的アーカイブは、MSVC 14.44（VS 2022）に無い STL の記号（`__std_find_trivial_1` など）を参照するため、リンクに VS 2026（v18）の Build Tools が必要です。sherpa-onnx-sys のビルドスクリプトが `ring` を引き込むので、VS の clang の部品（Microsoft.VisualStudio.Component.VC.Llvm.Clang）も要ります。
`release.yml` は `vswhere` で確認し、無ければ `aka.ms/vs/18/release/vs_BuildTools.exe` から静かにインストールします。手元でも同じ部品構成を入れてください。

## 公開前に導入スクリプトを試す

資産が手元にあるうちに、`install.sh`・`install.ps1` をこのタグの資産に対して試せます。実行環境は `HOME`・`XDG_CONFIG_HOME`・`XDG_CACHE_HOME`・`HF_HOME`・`KIKITORI_MODELS` を一時ディレクトリに向け、資産と `SHA256SUMS` を集めたフォルダを `KIKITORI_BASE_URL` に渡します。

```bash
mkdir ~/tmp/kikitori-try && cp dist/* ~/tmp/kikitori-try/
cp install.sh ~/tmp/kikitori-try/
KIKITORI_BASE_URL="$HOME/tmp/kikitori-try" \
KIKITORI_INSTALL_DIR="$HOME/tmp/kikitori-try/bin" \
KIKITORI_NO_MODIFY_PATH=1 \
HOME="$HOME/tmp/kikitori-try" \
sh install.sh
```

`KIKITORI_VERSION` を渡さなければ、フォルダの中で最も新しい資産を「latest」として選びます。
導入の最後で `kikitori setup` が中核のモデルを取得するので、モデルを取らせたくない検証では `HF_HUB_OFFLINE=1` を渡します（その場合 setup はオフラインのふるまいになるため、モデルの取得まで見る試験には使えません）。Windows では同じ変数を出し、`install.ps1` を PowerShell で実行します。

## 取り下げる

Release に問題があったときは、次の順で取り下げます。

```bash
gh release delete v0.3.0 --yes            # Release と載せたファイルを消す
git push origin :refs/tags/v0.3.0         # GitHub のタグを消す
git tag -d v0.3.0                         # 手元のタグを消す
```

出してはいけないものを公開してしまったときは、`gh repo edit --visibility private --accept-visibility-change-consequences` でリポジトリを非公開に戻します。
取り下げる前に誰かが複製（clone・ミラー・検索エンジンのキャッシュ）していることがあるので、取り下げても消えるとは限りません。
