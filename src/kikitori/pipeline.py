"""VAD → 認識（トークン時刻つき）→ 話者分離 → トークンへ話者を割り当て。"""
import sys
from dataclasses import dataclass

import numpy as np

from . import engine

SR = 16000


@dataclass
class Segment:
    start: float
    end: float
    speaker: int
    text: str


def vad_chunks(vad, samples: np.ndarray, pad=1.0):
    """VAD の区間を実音声の余白つきで切り出す。(余白込みの開始秒, 波形, 本体の開始秒, 本体の終了秒) の列。

    冒頭の余白が短いと認識器が最初の一文を落とすため、前後 1 秒の実音声を含める。
    重複して認識された文字は、本体の範囲外を捨てて取り除く。
    """
    spans = []
    win = 512
    for i in range(0, len(samples) - win + 1, win):
        vad.accept_waveform(samples[i:i + win])
        while not vad.empty():
            seg = vad.front
            spans.append((seg.start, seg.start + len(seg.samples)))
            vad.pop()
    vad.flush()
    while not vad.empty():
        seg = vad.front
        spans.append((seg.start, seg.start + len(seg.samples)))
        vad.pop()
    core = []
    for a, b in spans:
        if core and a - core[-1][1] < SR // 2:
            core[-1][1] = b
        else:
            core.append([a, b])
    if not core and len(samples) > 0.2 * SR and float(np.sqrt((samples ** 2).mean())) > 0.005:
        core = [[0, len(samples)]]  # VAD が拾えない短い発話は全体を 1 区間として認識する
    p = int(pad * SR)
    out = []
    for a, b in core:
        lo, hi = max(0, a - p), min(len(samples), b + p)
        out.append((lo / SR, samples[lo:hi], a / SR, b / SR))
    return out


PAD = 0.5  # 各区間の前後につける余白（実音声＋弱い雑音）の秒数
EXT = 0.6  # VAD の発話区間を前後に広げる秒数（Silero は発話の頭を遅れて検出しがち）
MAXSEG = 28.0  # これを超える区間は VAD の無音で分ける
_RNG = np.random.default_rng(0)


def _noise(sec):
    return (_RNG.standard_normal(int(sec * SR)) * 0.003).astype(np.float32)


def _turns(dz, samples):
    return sorted((r.start, r.end, r.speaker) for r in dz.process(samples).sort_by_start_time())


def _merge(turns, min_gap):
    merged = []
    for a, b, sp in turns:
        for m in reversed(merged[-4:]):
            if m[2] == sp and a - m[1] < min_gap:
                m[1] = max(m[1], b)
                break
        else:
            merged.append([a, b, sp])
    return [tuple(m) for m in merged]


def _embed(emb, samples, a, b, cap=15.0):
    """区間の話者埋め込み。長い区間は中央の cap 秒だけを使う。"""
    mid = (a + b) / 2
    a, b = max(a, mid - cap / 2), min(b, mid + cap / 2)
    st = emb.create_stream()
    st.accept_waveform(SR, samples[int(a * SR):int(b * SR)])
    st.input_finished()
    v = np.asarray(emb.compute(st), dtype=np.float64)
    return v / (np.linalg.norm(v) + 1e-9)


def _cluster(vecs, weights, n):
    """コサイン距離の平均連結で n 個になるまで併合する。ラベルの列を返す。"""
    groups = [[i] for i in range(len(vecs))]
    cent = [vecs[i] * weights[i] for i in range(len(vecs))]
    w = list(weights)
    while len(groups) > n:
        best, pair = -2.0, None
        for i in range(len(groups)):
            for j in range(i + 1, len(groups)):
                ci, cj = cent[i] / (np.linalg.norm(cent[i]) + 1e-9), cent[j] / (np.linalg.norm(cent[j]) + 1e-9)
                sim = float(ci @ cj)
                if sim > best:
                    best, pair = sim, (i, j)
        i, j = pair
        groups[i] += groups[j]; cent[i] = cent[i] + cent[j]; w[i] += w[j]
        del groups[j], cent[j], w[j]
    labels = [0] * len(vecs)
    for g, members in enumerate(groups):
        for i in members:
            labels[i] = g
    return labels


def diarize(samples, models, threads, num_speakers, threshold, min_gap=0.6, log=lambda m: None):
    """話者分離の区間 (開始, 終了, 話者) を返す。

    sherpa-onnx の num_clusters は指定どおりの人数にならないことがあるため、人数が指定されたときは
    少し細かく分けたあと、区間ごとの話者埋め込みを取り直して、指定の人数になるまで併合する。
    """
    if num_speakers <= 0:
        return _merge(_turns(engine.make_diarizer(models, threads, -1, threshold), samples), min_gap)
    turns = _merge(_turns(engine.make_diarizer(models, threads, -1, min(threshold, 0.35)), samples), min_gap)
    if len({t[2] for t in turns}) <= num_speakers and len(turns) >= num_speakers:
        # すでに足りないか同数なら、同数のときはそのまま使う（足りないときは警告）
        n = len({t[2] for t in turns})
        if n < num_speakers:
            print(f"警告: 指定の {num_speakers} 人に分けられず、{n} 人になりました。", file=sys.stderr)
        return turns
    emb = engine.make_embedder(models, threads)
    keep = [t for t in turns if t[1] - t[0] >= 0.4]
    vecs = [_embed(emb, samples, a, b) for a, b, _ in keep]
    labels = _cluster(vecs, [min(b - a, 15.0) for a, b, _ in keep], num_speakers)
    lab = {id(t): l for t, l in zip(keep, labels)}
    # 短くて埋め込みを取らなかった区間は、直前の区間の話者を引き継ぐ
    out, last = [], 0
    for t in turns:
        last = lab.get(id(t), last)
        out.append((t[0], t[1], last))
    log(f"話者 {len(set(labels))} 人（指定 {num_speakers} 人）")
    return _merge(out, min_gap)


def _dominant(turns, a, b):
    """区間 [a, b] と最も長く重なる話者。重なりがなければ、時間的に最も近い区間の話者。"""
    ov = {}
    for x, y, sp in turns:
        o = min(b, y) - max(a, x)
        if o > 0:
            ov[sp] = ov.get(sp, 0.0) + o
    if ov:
        return max(ov, key=ov.get)
    mid = (a + b) / 2
    return min(turns, key=lambda t: min(abs(t[0] - mid), abs(t[1] - mid)))[2] if turns else 0


def plan(spans, turns, min_piece=0.5):
    """VAD の発話区間を土台に、区間内で話者が交代するところだけ切る。(開始, 終了, 話者) の列。"""
    pieces = []
    for a, b in spans:
        cuts = sorted({t for x, y, _ in turns for t in (x, y) if a + min_piece < t < b - min_piece})
        edges = [a] + cuts + [b]
        segs = []
        for x, y in zip(edges, edges[1:]):
            sp = _dominant(turns, x, y)
            if segs and (segs[-1][2] == sp or y - x < min_piece):
                segs[-1] = (segs[-1][0], y, segs[-1][2] if segs[-1][2] == sp or y - x < min_piece else sp)
            else:
                segs.append((x, y, sp))
        pieces += segs
    return pieces


def run(samples, asr, models, threads=4, num_speakers=-1, threshold=0.6, replacer=None, log=lambda m: None):
    """VAD の発話区間を土台に、話者分離の結果で話者を付けて認識する。トークンの時刻は使わない。"""
    turns = diarize(samples, models, threads, num_speakers, threshold, log=log)
    total = len(samples) / SR
    ext = EXT
    raw = [(max(0.0, c0 - ext), min(total, c1 + ext)) for _, _, c0, c1 in vad_chunks(engine.make_vad(models, threads), samples, pad=0.0)]
    spans = []
    for a, b in raw:  # 広げて重なった区間はつなぐ
        if spans and a <= spans[-1][1]:
            spans[-1] = (spans[-1][0], max(spans[-1][1], b))
        else:
            spans.append((a, b))
    # 長すぎる区間は VAD の最大長で分かれている。話者分離が見つけた発話で VAD が拾えなかった部分も足す
    covered = lambda x, y: any(min(y, b) - max(x, a) > 0.2 for a, b in spans)
    spans += [(x, y) for x, y, _ in turns if y - x >= 0.5 and not covered(x, y)]
    spans.sort()
    pieces = plan(spans, turns)
    bounds = sorted({p[0] for p in pieces} | {p[1] for p in pieces})
    waves = []
    for a, b, sp in pieces:
        # 境界の外側に、隣の区間に食い込まない範囲で実音声の余白をつける。足りない分は弱い雑音で補う
        prev = max([t for t in bounds if t < a - 1e-6], default=0.0)
        nxt = min([t for t in bounds if t > b + 1e-6], default=total)
        lp, rp = min(0.4, max(0.0, (a - prev) / 2)), min(0.4, max(0.0, (nxt - b) / 2))
        w = samples[int((a - lp) * SR):int((b + rp) * SR)]
        waves.append(np.concatenate([_noise(PAD - lp), w, _noise(PAD - rp)]))
    segs = []
    for (a, b, sp), text in zip(pieces, asr.transcribe(waves)):
        if replacer and text:
            text = replacer.apply(text)
        if text:
            segs.append(Segment(a, b, sp, text))
    found = len({g.speaker for g in segs})
    if num_speakers > 0 and found < num_speakers:
        print(f"警告: 指定の {num_speakers} 人に対し、発言が見つかった話者は {found} 人でした。", file=sys.stderr)
    return segs
