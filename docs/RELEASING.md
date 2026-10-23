# リリースの手順

kikitori の版は `src/kikitori/__init__.py` の `__version__` の 1 か所で決まります。`pyproject.toml` は、ビルドの道具 hatchling の設定でそこを読みます。
リリースは 2 段に分かれます。手元で、版を上げたコミットとタグを作り、配る wheel（インストール用のパッケージ）と sdist（ソースの tar.gz）を試しに作って確かめる段（`scripts/release.py`）と、タグを GitHub に push して、GitHub Actions が Release を作る段です。
手順には `uv`・`git`・`gh`（GitHub の CLI）を使います。

## 版の付け方

[Semantic Versioning](https://semver.org/lang/ja/) に従います。0.x の間は、利用者の手順や出力が変わる変更で minor を上げ、それ以外は patch を上げます。
リリースとリリースの間の `__version__` は、次に出す予定の版に `.dev0` を付けた値（例: 0.2.0 を出したあとは `0.3.0.dev0`）にしておきます。

## 1. 変更点を書く

変更を入れるたびに、`CHANGELOG.md` の `## [Unreleased]` の下に書きためます。見出しは `### Added`・`### Changed`・`### Fixed`・`### Removed` を使います。

## 2. 手元でリリースのコミットとタグを作る

`main` で、作業ツリーをきれいにしてから打ちます。

```bash
uv run python scripts/release.py 0.3.0
```

`release.py` は次を順に行います。どこかで失敗したら、書き換えた `__init__.py` と `CHANGELOG.md` を戻して止まります。

1. 検査: 作業ツリーがきれい、`main` にいる、`main` の最初のコミット（親の無いコミット）のメッセージが `kikitori: public history root` である、タグがまだ無い、`[Unreleased]` に中身がある。最初のコミットの検査は、公開してはいけない古い履歴の上で誤ってリリースしないためのものです。公開リポジトリの最初のコミットを、このメッセージで作ってあります。
2. `__version__` を新しい版にし、`CHANGELOG.md` の `[Unreleased]` の中身を `## [0.3.0] - 今日の日付` の節に移す。
3. テスト（`pytest`）。
4. 一時のフォルダに wheel と sdist を作る。
5. 一時の場所に wheel を `uv tool install` し、`kikitori --version` と `kikitori-refine --version` が新しい版を返すことを確かめる。
6. 公開前の検査（この文書の「公開前の検査」の節）を、履歴と wheel・sdist に走らせる。
7. コミット `Release 0.3.0` とタグ `v0.3.0` を作る。push はしない。

## 3. 公開する

```bash
git push origin main v0.3.0
```

タグの push で `.github/workflows/release.yml` が動き、次を行います。

1. テスト。
2. タグと `__version__` が一致することを確かめる。
3. wheel と sdist を作る。
4. `CHANGELOG.md` から、その版の節を取り出して Release の本文にする（節が無ければ失敗する）。
5. `install.sh`・`install-windows.ps1` の `__WHEEL_URL__` を、その版の wheel の URL に置き換え、`install.ps1` の `__INSTALLER_URL__` を、その版の `install-windows.ps1` の URL に置き換える。
6. GitHub Release を作り、wheel・sdist・install.sh・install.ps1・install-windows.ps1 を載せる。

`releases/latest/download/install.sh` は、最新の Release の install.sh を指します。手元のリポジトリの導入スクリプトは置き換える前の値のままで、そのままでは使えません。試すときは、`KIKITORI_WHEEL` に wheel のパスを渡します。Windows では、さらに `KIKITORI_INSTALLER` に `install-windows.ps1` のパスを渡し、`install.ps1` を実行します。

公開したあと、`__version__` を次に出す予定の版の `.dev0` に上げ、`main` にコミットして push します。

## 公開前の検査

`scripts/check_public.py` は、公開するものに出してはいけない語（顧客名・取引先名・手元のパスの断片など）が無いかを調べます。
語の一覧は、それ自体が出してはいけない語なので、リポジトリの外に置きます。

- 一覧の場所: `~/.config/kikitori/public-denylist.txt`（環境変数 `KIKITORI_DENYLIST` で変更可）。1 行 1 語、`#` で始まる行は注記。一覧が無いと検査は失敗するので、リリースする人が自分の手元に作る。入れる語は、扱った録音の顧客名・取引先名・人名、手元のフォルダのパスの断片（ホームのフォルダ名など）。
- 調べるもの: `main` の全コミットのファイルの中身・パス・コミットメッセージ・author と committer、タグのメッセージ、作業ツリーの git が管理するファイル、`--dist` で渡した wheel と sdist の中身。

```bash
uv run python scripts/check_public.py              # 履歴と作業ツリー
uv run python scripts/check_public.py --dist dist  # wheel と sdist も
```

1 件でも見つかったら公開しません。見つかった語を直します。まだ push していないコミットに入っているなら、`git rebase` でそのコミットを直してから検査し直します。既に公開した履歴に入っているなら、下の「取り下げる」に従います。

## 取り下げる

Release に問題があったときは、次の順で取り下げます。

```bash
gh release delete v0.3.0 --yes            # Release と載せたファイルを消す
git push origin :refs/tags/v0.3.0         # GitHub のタグを消す
git tag -d v0.3.0                         # 手元のタグを消す
```

出してはいけないものを公開してしまったときは、リポジトリを非公開に戻します（`gh repo edit kulikala/kikitori --visibility private --accept-visibility-change-consequences`）。
公開した内容は、取り下げる前に誰かが複製（clone やミラー、検索エンジンのキャッシュ）していることがあり、取り下げで消えるとは限りません。

## モデルの表を更新する

補正に使う Ollama のモデルは、`src/kikitori/refine/catalog.py` の `CATALOG`（機器ごとの候補）と `SIZES`（大きさ）で決まります。
`kikitori-refine --setup`（取得するモデル）と、補正のときに直し方とモデルを選ぶ処理（`src/kikitori/refine/route.py`）は、どちらもこの表を読みます。

モデルを替えるときは、次を合わせて直します。

1. `CATALOG` と `SIZES`。大きさは ollama.com/library の tags ページの値。
2. Ollama に求める版が上がるなら `MIN_OLLAMA`（`ollama show <モデル>` の `requires`）。
3. `tests/test_refine.py` の `test_pick_by_hardware` の期待値。
4. README の「計算機とモデルの選び方」の表。新しいモデルの精度を正解つきの録音で測っていなければ、README の「測っていません」の箇条にそのモデルを書く。
5. `CHANGELOG.md` の `[Unreleased]`。
