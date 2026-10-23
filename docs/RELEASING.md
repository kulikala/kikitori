# リリースの手順

kikitori の版は `src/kikitori/__init__.py` の `__version__` だけで決まります。
リリースは 2 段です。手元で `scripts/release.py` を打って版を上げたコミットとタグを作り、タグを GitHub に push すると、GitHub Actions が Release を作ります。
手順には `uv`・`git`・`gh`（GitHub の CLI）を使います。

## 版の付け方

[Semantic Versioning](https://semver.org/lang/ja/) に従います。0.x の間は、利用者の手順や出力が変わる変更で minor を上げ、それ以外は patch を上げます。
リリースとリリースの間の `__version__` は、次に出す予定の版に `.dev0` を付けた値（例: 0.2.0 を出したあとは `0.3.0.dev0`）にしておきます。

## 1. 変更点を書く

変更を入れるたびに、`CHANGELOG.md` の `## [Unreleased]` の下に書きためます。見出しは `### Added`・`### Changed`・`### Fixed`・`### Removed` を使います。
文書の書き方は [WRITING.md](WRITING.md) に従います。

## 2. 手元でリリースのコミットとタグを作る

`main` で、作業ツリーをきれいにしてから打ちます。

```bash
uv run python scripts/release.py 0.3.0
```

`release.py` は、版を書き換えて `[Unreleased]` を `## [0.3.0] - 今日の日付` の節に移し、テスト・wheel と sdist のビルド・試しの導入・公開前の検査を通してから、コミット `Release 0.3.0` とタグ `v0.3.0` を作ります。
どこかで失敗したら、書き換えたファイルを戻して止まります。push はしません。
`main` が公開用の履歴（最初のコミットが `kikitori: public history root`）でなければ、始める前に止まります。

## 3. 公開する

```bash
git push origin main v0.3.0
```

タグの push で `.github/workflows/release.yml` が動きます。テストとタグの版の照合を通したあと、`CHANGELOG.md` のその版の節を本文にして GitHub Release を作り、wheel・sdist・install.sh・install.ps1・install-windows.ps1 を載せます。導入スクリプトには、このときその版の URL が書き込まれます。

リポジトリの中の導入スクリプトは URL が書き込まれる前のままなので、そのままでは動きません。手元で試すときは、`KIKITORI_WHEEL` に wheel のパスを渡します。Windows では、さらに `KIKITORI_INSTALLER` に `install-windows.ps1` のパスを渡して、`install.ps1` を実行します。

公開したあと、`__version__` を次に出す予定の版の `.dev0` に上げ、`main` にコミットして push します。

## 公開前の検査

`scripts/check_public.py` は、公開するものに出してはいけない語（扱った録音の顧客名・取引先名・人名、手元のフォルダのパスの断片など）が無いかを調べます。`release.py` も途中でこの検査を走らせます。

語の一覧は、それ自体が出してはいけない語なので、リポジトリの外に置きます。既定の場所は `~/.config/kikitori/public-denylist.txt`（環境変数 `KIKITORI_DENYLIST` で変更可）で、1 行 1 語、`#` で始まる行は注記です。一覧が無いと検査は失敗するので、リリースする人が自分の手元に作ります。

```bash
uv run python scripts/check_public.py              # main の履歴と作業ツリー
uv run python scripts/check_public.py --dist dist  # wheel と sdist も
```

1 件でも見つかったら公開しません。まだ push していないコミットなら、`git rebase` で直してから検査し直します。既に公開した履歴に入っているなら、下の「取り下げる」に従います。

## 取り下げる

Release に問題があったときは、次の順で取り下げます。

```bash
gh release delete v0.3.0 --yes            # Release と載せたファイルを消す
git push origin :refs/tags/v0.3.0         # GitHub のタグを消す
git tag -d v0.3.0                         # 手元のタグを消す
```

出してはいけないものを公開してしまったときは、`gh repo edit --visibility private --accept-visibility-change-consequences` でリポジトリを非公開に戻します。
取り下げる前に誰かが複製（clone・ミラー・検索エンジンのキャッシュ）していることがあるので、取り下げても消えるとは限りません。

## モデルの表を更新する

補正に使う Ollama のモデルは `src/kikitori/refine/catalog.py` の `CATALOG`（機器ごとの候補）と `SIZES`（大きさ）で決まります。`kikitori-refine --setup` と補正の直し方の選択は、どちらもこの表を読みます。

モデルを替えるときは、次を合わせて直します。

1. `CATALOG` と `SIZES`。大きさは ollama.com/library の tags ページの値。
2. Ollama に求める版が上がるなら `MIN_OLLAMA`（`ollama show <モデル>` の `requires`）。
3. `tests/test_refine.py` の `test_pick_by_hardware` の期待値。
4. README の「計算機とモデルの選び方」の表。新しいモデルの精度を正解つきの録音で測っていなければ、README の「測っていません」の箇条にそのモデルを書く。
5. `CHANGELOG.md` の `[Unreleased]`。
