# Synthetic MP3 probe fixture

`tone.mp3` is an original 0.1-second, 440 Hz mono sine wave, with no recordings,
third-party audio, or private data. Generated on macOS on 2026-11-15:

```python
import math, struct, wave
with wave.open('/private/tmp/probe-tone.wav', 'wb') as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(44100)
    w.writeframes(b''.join(struct.pack('<h', round(3000 * math.sin(
        2 * math.pi * 440 * i / 44100))) for i in range(4410)))
```

```sh
lame --silent --noreplaygain -b 64 /private/tmp/probe-tone.wav tone.mp3
mpg123 -q -w /private/tmp/probe-tone-decoded.wav tone.mp3
ffmpeg -v error -i tone.mp3 -f null -
```

Generated with LAME 4.0; independently checked with mpg123 1.33.7 and FFmpeg.
SHA-256: `3b1804b9335437838f4057ef3d266b014af71b922f14f16949ddeee79f5e728a`.
Both independent decoders succeeded for the original file and a variant with a
131,072-byte ID3v2.4 padding body prepended (header `49 44 33 04 00 00 00 08 00 00`).
The large-tag variant decoded to 4,410 samples at 44,100 Hz, mono. These are fixture-generation tools only;
tests and production do not execute them. Tests construct large ID3 padding at
runtime, retaining the real MPEG frames. JPEG fixtures are generated with the
existing `image` test codec and decoded again after APP/COM metadata insertion.
