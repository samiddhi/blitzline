"""Example ASR worker for Blitzline's HTTP backend: whisper-timestamped, then MMS-FA word refinement.

Run:  python asr_worker.py [port]      (needs the GPU, whisper-timestamped and torchaudio; default port 8765)
Blitzline config: asr.backend="http", asr.endpoint="http://127.0.0.1:8765/transcribe",
asr.worker_command=["/path/to/python", "/path/to/asr_worker.py"]
"""
import base64, json, os, re, sys, tempfile, threading, time, unicodedata
from http.server import BaseHTTPRequestHandler, HTTPServer

import torch, torchaudio as ta, whisper_timestamped as wt

DEV = "cuda"
SR = 16000
PAD = 1.0  # seconds of audio around each segment for alignment
MIN_CONF = 0.5  # below this MMS-FA score keep whisper's time for that word
# ponytail: tiny 3->2 letter map; extend as needed, unknown codes fall back to auto-detect
LANG = {"slv": "sl", "eng": "en", "deu": "de", "fra": "fr", "spa": "es", "ita": "it", "rus": "ru", "hrv": "hr", "srp": "sr"}

# ponytail: the GPU is sometimes briefly unavailable right after launch (hybrid graphics); wait up to 30 s
for _ in range(30):
    if torch.cuda.is_available():
        break
    time.sleep(1)
asr = wt.load_model("large-v3-turbo", device=DEV)
bundle = ta.pipelines.MMS_FA
fa = bundle.get_model(with_star=False).to(DEV).eval()
DICT = bundle.get_dict(star=None)


def rom(w):
    # ponytail: Latin scripts only; other scripts romanize to "" and keep whisper's times (uroman if needed)
    return re.sub("[^a-z]", "", unicodedata.normalize("NFD", w.lower().replace("đ", "d")))


def refine(audio, seg):
    ws = [w for w in seg["words"] if rom(w["text"])]
    if not ws:
        return
    a0 = max(0.0, seg["start"] - PAD)
    wav = audio[int(a0 * SR):int((seg["end"] + PAD) * SR)][None].to(DEV)
    with torch.inference_mode():
        em, _ = fa(wav)
    em = torch.log_softmax(em, -1).cpu()
    toks = [DICT[c] for w in ws for c in rom(w["text"])]
    ali, sc = ta.functional.forced_align(em, torch.tensor([toks], dtype=torch.int32))
    spans = ta.functional.merge_tokens(ali[0], sc[0].exp())
    fr = wav.shape[1] / em.shape[1] / SR
    k = 0
    for w in ws:
        n = len(rom(w["text"]))
        sp = spans[k:k + n]
        k += n
        if sum(s.score for s in sp) / n >= MIN_CONF:
            s, e = a0 + sp[0].start * fr, a0 + sp[-1].end * fr
            if e > s:
                w["start"], w["end"] = s, e


def transcribe(path, language):
    audio = wt.load_audio(path)
    out = wt.transcribe(asr, audio, language=language, verbose=None)
    audio = torch.from_numpy(audio)
    dur = len(audio) / SR
    segments = []
    for seg in out["segments"]:
        if not seg["text"].strip():
            continue
        try:
            refine(audio, seg)
        except Exception as e:  # keep whisper's times if alignment fails for this segment
            print("MMS-FA failed for segment", seg["start"], e, file=sys.stderr, flush=True)
        # Blitzline needs times inside the audio, monotonic starts and end > start
        words, p = [], 0.0
        for w in seg["words"]:
            s = min(max(w["start"], p), dur)
            e = min(w["end"], dur)
            if e > s:
                words.append({"word": w["text"], "start": s, "end": e})
                p = s
        start = words[0]["start"] if words else min(seg["start"], dur)
        end = words[-1]["end"] if words else min(seg["end"], dur)
        if end > start:
            segments.append({"start": start, "end": end, "text": seg["text"].strip(), "words": words})
    return {"segments": segments}


class H(BaseHTTPRequestHandler):
    def do_POST(self):
        try:
            req = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            lang = (req.get("language") or "")[:3].lower()
            lang = LANG.get(lang, lang if len(lang) == 2 else None)
            with tempfile.NamedTemporaryFile(suffix=".wav") as f:
                f.write(base64.b64decode(req["audio"]))
                f.flush()
                body, code = json.dumps(transcribe(f.name, lang)).encode(), 200
        except Exception as e:
            body, code = json.dumps({"error": str(e)}).encode(), 500
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def exit_with_parent(pid):
    # set by Blitzline when it launches the worker; a hand-started worker has no parent to follow
    while True:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            os._exit(0)
        except PermissionError:
            pass
        time.sleep(2)


if __name__ == "__main__":
    if os.environ.get("BLITZLINE_PARENT_PID"):
        threading.Thread(target=exit_with_parent, args=(int(os.environ["BLITZLINE_PARENT_PID"]),), daemon=True).start()
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    print(f"ASR worker on http://127.0.0.1:{port}/transcribe", flush=True)
    HTTPServer(("127.0.0.1", port), H).serve_forever()
