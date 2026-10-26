# kikitori のエージェント評価の課題

経緯を知らないエージェントに、SKILL.md と CLI だけを渡して解かせる。音声は公開・合成のものだけを使う。
採点は GRADING.md。エージェントには、この表の「課題」の列だけを渡す（採点の列は渡さない）。

| # | 課題（エージェントに渡す文） | 採点 |
|---|---|---|
| 1 | `meeting.wav` は 4 人の会議の録音です。話者ごとに文字に起こし、機械で読める形で `out/meeting.json` に保存してください。 | `kikitori transcribe`（または `kikitori <音声>`）に `--num-speakers 4` と `--format json-v1`（または json）。終了コード 0、`out/meeting.json` が schema に合う |
| 2 | 同じ録音の固有名詞「東京リージョン」「負荷試験」が正しく書かれるように、辞書を作って起こし直し、`out/meeting-dict.json` に保存してください。 | 辞書ファイルを `表記<TAB>読み` で作り `--dict` を付ける。出力にその表記が出る |
| 3 | 起こした本文を、あなた（呼び出し元の LLM）が直してください。利用者は、本文を外部の LLM に渡してよいと言っています。結果を `out/meeting-refined.json` に保存してください。 | `kikitori refine ... --external bundle` → 問いの JSON を読んで答えを書く → `--apply 問い.json --reply 答え`。要約・言い換えをしていない |
| 4 | この PC で kikitori がどの計算機を使い、どのモデルが取得済みかを調べて、日本語で 3 行にまとめて `out/doctor.txt` に書いてください。 | `kikitori doctor --json` を読んでいる。内容が実際の出力と合う |
| 5 | 存在しないファイル `nofile.m4a` を起こそうとしたら何が起きるかを確かめ、終了コードと、次に何をすればよいかを `out/error.txt` に書いてください。 | 終了コード 3 と hint を読んでいる |
| 6 | kikitori をこの PC から削除するときに、何が消え何が残るかを、実際には削除せずに確かめて `out/uninstall.txt` に書いてください。 | `kikitori uninstall --dry-run`。実際に消していない |
