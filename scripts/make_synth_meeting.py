"""say で複数話者の会議を合成し、正解（話者・本文）を JSON に残す。話者分離の動作確認用。"""
import json, subprocess, sys
from pathlib import Path
out = Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
import os
VOICES = dict(enumerate(os.environ.get("VOICES", "Kyoko,Eddy,Reed,Sandy").split(",")))
LINES = [
    (0, "それでは定例会議を始めます。まず先週の進捗から確認しましょう。"),
    (1, "はい、新しい料金プランの試験運用は予定どおり終わりました。"),
    (2, "ユーザーからの問い合わせは先週より二割ほど減っています。"),
    (3, "ただし東京リージョンでは、一時的に応答が遅くなる問題が出ました。"),
    (0, "原因は特定できていますか。"),
    (3, "はい、データベースの接続数の上限が足りていませんでした。"),
    (1, "来週までに設定を見直して、負荷試験をやり直します。"),
    (2, "お客様への連絡は私のほうで進めます。"),
    (0, "ありがとうございます。次に来月のリリース計画について話しましょう。"),
    (1, "機能の追加は三件で、すべて開発が完了しています。"),
    (3, "テストは今週金曜日までに終わらせる予定です。"),
    (0, "では、本日の会議はここまでとします。お疲れさまでした。"),
]
gap = subprocess.run(["ffmpeg","-v","error","-y","-f","lavfi","-i","anullsrc=r=16000:cl=mono","-t","0.5",str(out/"gap.wav")]); 
files, truth, t = [], [], 0.0
for i, (sp, text) in enumerate(LINES):
    a = out/f"u{i:02d}.aiff"; w = out/f"u{i:02d}.wav"
    subprocess.run(["say","-v",VOICES[sp],"-o",str(a),text],check=True)
    subprocess.run(["ffmpeg","-v","error","-y","-i",str(a),"-ac","1","-ar","16000",str(w)],check=True)
    d = float(subprocess.run(["ffprobe","-v","error","-show_entries","format=duration","-of","csv=p=0",str(w)],capture_output=True,text=True).stdout)
    truth.append({"start": round(t,2), "end": round(t+d,2), "speaker": sp, "text": text}); t += d + 0.5
    files += [w, out/"gap.wav"]
lst = out/"list.txt"; lst.write_text("".join(f"file '{f.resolve()}'\n" for f in files))
subprocess.run(["ffmpeg","-v","error","-y","-f","concat","-safe","0","-i",str(lst),"-c","copy",str(out/"meeting.wav")],check=True)
json.dump(truth, open(out/"truth.json","w"), ensure_ascii=False, indent=1)
print("OK", round(t,1), "秒")
