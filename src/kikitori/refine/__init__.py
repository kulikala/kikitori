"""kikitori-refine: kikitori の本文を、別の認識結果と LLM で補正する（kikitori 本体とは別のコマンド）。

本文（A、ReazonSpeech）と同じ区間を、Qwen3-ASR（B、漢字かな交じり）と kana-whisper（K、カタカナ）でも認識し、
使える環境に応じて次のどれかで直す。どれを選ぶかは route.choose が決める。
- external: 外部 LLM に A・B・K と用語集を渡して直させる（呼び出し元の LLM が直すか、claude CLI に渡す）
- merge:    Ollama の大きなモデルに、区間ごとに A・B・K を突き合わせさせる
- glossary: 長さの規則で A か B を採り、tev1 に用語集の語へ置き換えるかを選ばせる
- rule:     長さの規則だけ（LLM を使わない）
- plain:    B・K を作れないときは、kikitori の本文のまま
"""
