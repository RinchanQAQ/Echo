# Source-verified API reference — rodio 0.22.2 & lofty 0.25.4

Every claim below carries the URL it came from. Anything I could not verify is flagged
**UNVERIFIED** rather than guessed. Fetched 2026 (docs.rs serves these versions).

MSRV: rodio 0.22.2 declares `rust-version = "1.87"`
(https://raw.githubusercontent.com/RustAudio/rodio/v0.22.2/Cargo.toml) — Rust 1.99 is fine.

---

# PART A — rodio 0.22.2

## A0. Headline: `OutputStream`, `OutputStreamHandle` and `Sink` no longer exist

Both of the pages you listed 404:

- https://docs.rs/rodio/0.22.2/rodio/struct.OutputStreamBuilder.html → HTTP 404,
  *"Version 0.22.2 of `rodio` exists, but this page inside it could not be found."*
- https://docs.rs/rodio/0.22.2/rodio/struct.Sink.html → HTTP 404, same message.

The crate root (https://docs.rs/rodio/0.22.2/rodio/) lists the real public items:
`Player`, `SpatialPlayer`, `DeviceSinkBuilder`, `MixerDeviceSink`, `DeviceSinkError`,
`PlayError`, `mixer::Mixer`, `decoder::Decoder`. There is **no** `OutputStream`,
`OutputStreamHandle` or `Sink` anywhere in the item list.

CHANGELOG.md, v0.22 section (https://raw.githubusercontent.com/RustAudio/rodio/v0.22.2/CHANGELOG.md):

> - Breaking: _Sink_ terms are replaced with _Player_ and _Stream_ terms replaced with _Sink_. This is a simple rename, functionality is identical.
>   - `OutputStream` is now `MixerDeviceSink` (in anticipation of future `QueueDeviceSink`)
>   - `OutputStreamBuilder` is now `DeviceSinkBuilder`
>   - `open_stream_or_fallback` is now `open_sink_or_fallback`
>   - `open_default_stream` is now `open_default_sink`
>   - `open_stream` is now `open_mixer` (in anticipation of future `open_queue`)
>   - `Sink` is now `Player`
>   - `SpatialSink` is now `SpatialPlayer`
>   - `StreamError` is now `OsSinkError`

**Two CHANGELOG claims do not match the shipped 0.22.2 API** (verified by absence on the
docs.rs pages): there is no `open_mixer` — the method is still `DeviceSinkBuilder::open_stream` —
and there is no `OsSinkError`; the error enum is `DeviceSinkError`.

Also from the 0.21 section of the same changelog: `OutputStreamHandle` was already removed in
0.21 (*"Breaking: `OutputStreamHandle` removed, use `OutputStream` and `OutputStream::mixer()` instead."*),
and `Sink::try_new` became `connect_new`.

## A1. Current top-level playback API + code you can write

Entry point = `DeviceSinkBuilder` (opens the OS sink) + `Player` (the control handle).

Exact signatures (https://docs.rs/rodio/0.22.2/rodio/stream/struct.DeviceSinkBuilder.html):

```rust
pub fn open_default_sink() -> Result<MixerDeviceSink, DeviceSinkError>
pub fn from_default_device() -> Result<DeviceSinkBuilder, DeviceSinkError>
pub fn from_device(device: Device) -> Result<DeviceSinkBuilder, DeviceSinkError>
pub fn open_stream(self) -> Result<MixerDeviceSink, DeviceSinkError>
pub fn open_sink_or_fallback(&self) -> Result<MixerDeviceSink, DeviceSinkError> where E: Clone
pub fn with_buffer_size(self, buffer_size: BufferSize) -> DeviceSinkBuilder<E>
```

`MixerDeviceSink` (https://docs.rs/rodio/0.22.2/rodio/stream/struct.MixerDeviceSink.html):

```rust
pub fn mixer(&self) -> &Mixer
pub fn config(&self) -> &DeviceSinkConfig
pub fn log_on_drop(&mut self, enabled: bool)
```
> "When dropped playback will end, and the associated OS-Sink will be disposed"

`Player` (https://docs.rs/rodio/0.22.2/rodio/struct.Player.html):

```rust
pub fn connect_new(mixer: &Mixer) -> Player
pub fn new() -> (Player, SourcesQueueOutput)
```
> "Dropping the `Player` stops all its sounds. You can use `detach` if you want the sounds to continue playing."

`Mixer` (https://docs.rs/rodio/0.22.2/rodio/mixer/struct.Mixer.html):

```rust
pub fn add<T>(&self, source: T) where T: Source + Send + 'static
```

### Exact code: open default device, get a controllable handle, keep it alive across UI frames

```rust
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

struct Audio {
    // MUST outlive everything: dropping this stops audio and disposes the OS sink.
    sink: MixerDeviceSink,
    player: Player,          // pause / resume / volume / seek / queue
}

impl Audio {
    fn new() -> Result<Self, rodio::stream::DeviceSinkError> {
        let sink = DeviceSinkBuilder::open_default_sink()?;  // MixerDeviceSink
        let player = Player::connect_new(sink.mixer());      // Player
        Ok(Self { sink, player })
    }

    fn play_file(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let file = std::fs::File::open(path)?;
        let source = Decoder::try_from(file)?;   // Decoder<BufReader<File>>
        self.player.append(source);
        Ok(())
    }
}
```

Store `Audio` in the egui app struct (see A6 — `Player`, `MixerDeviceSink` and `Mixer` are all
`Send + Sync`).

There is also a one-shot convenience fn, used in `examples/basic.rs`
(https://raw.githubusercontent.com/RustAudio/rodio/v0.22.2/examples/basic.rs):

```rust
let stream_handle = rodio::DeviceSinkBuilder::open_default_sink()?;
let mixer = stream_handle.mixer();
let file = std::fs::File::open("assets/beep.wav")?;
let player = rodio::play(mixer, BufReader::new(file))?;  // -> Result<Player, PlayError>
player.set_volume(0.2);
```

The stream module says `play` is *"A convenience function. Plays a sound once. Returns a `Player`
that can be used to control the sound."* I did **not** fetch the verbatim `fn play` signature page,
so treat the argument/return shape above as taken from the working example rather than a quoted signature.

## A2. Exact signatures — all on `Player` unless stated

From https://docs.rs/rodio/0.22.2/rodio/struct.Player.html:

```rust
// append a decoded file
pub fn append<S>(&self, source: S)
where S: Source + Send + 'static, f32: FromSample<S::Item>

// volume
pub fn volume(&self) -> Float
pub fn set_volume(&self, value: Float)          // 1.0 == normal

// pause / play
pub fn play(&self)                              // "Resumes playback of a paused player. No effect if not paused."
pub fn pause(&self)                             // "No effect if already paused."
pub fn is_paused(&self) -> bool

// stop / clear / queue
pub fn stop(&self)                              // "Stops the sink by emptying the queue."
pub fn clear(&self)                             // "Removes all currently loaded `Source`s from the `Player`, and pauses it."
pub fn skip_one(&self)
pub fn empty(&self) -> bool                     // "Returns true if this sink has no more sounds to play."
pub fn len(&self) -> usize                      // number of queued sounds
pub fn detach(self)

// SEEKING
pub fn try_seek(&self, pos: Duration) -> Result<(), SeekError>
pub fn get_pos(&self) -> Duration

// speed
pub fn speed(&self) -> f32
pub fn set_speed(&self, value: f32)
pub fn sleep_until_end(&self)
```

Answers to your specific questions:

- **Seeking**: it is `try_seek`, **not** `seek`, and the argument is `std::time::Duration`.
  There is no `Player::seek`. `Player::try_seek` takes `&self` (so it works through a shared
  reference — good for UI code).
- `Player` has **no** method named `seek`, `unpause`, `try_pause`, `volume_up`, or `set_pos`.
- The `Source` trait also has a seek method, with a *different* receiver
  (https://docs.rs/rodio/0.22.2/rodio/source/trait.Source.html):

```rust
fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError>   // provided method on Source
```

So: `Player::try_seek(&self, Duration)` **vs** `Source::try_seek(&mut self, Duration)`.
`Player::try_seek` docs, verbatim:
> "Attempts to seek to a given position in the current source. This blocks between 0 and ~5 milliseconds.
> As long as the duration of the source is known, seek is guaranteed to saturate at the end of the source."
> Errors: "This function will return `SeekError::NotSupported` if one of the underlying sources does not support seeking."

Working seek example (https://raw.githubusercontent.com/RustAudio/rodio/v0.22.2/examples/seek_mp3.rs):

```rust
let stream_handle = rodio::DeviceSinkBuilder::open_default_sink()?;
let player = rodio::Player::connect_new(stream_handle.mixer());
let file = std::fs::File::open("assets/music.mp3")?;
player.append(rodio::Decoder::try_from(file)?);
player.try_seek(Duration::from_secs(4))?;
player.sleep_until_end();
```

`Float` is `f32` by default; the crate docs say (`https://docs.rs/rodio/0.22.2/rodio/`, type alias `Float`):
> "Floating point type used for internal calculations. Can be configured to be either `f32` (default) or `f64` using the `64bit` feature flag."

## A3. Decoding a local file

Module docs (https://docs.rs/rodio/0.22.2/rodio/decoder/index.html) — "the simplest way":

```rust
use std::fs::File;
use rodio::Decoder;

let file = File::open("audio.mp3").unwrap();
let decoder = Decoder::try_from(file).unwrap();  // Automatically sets byte_len from metadata
```

And, verbatim, "for more control over decoder settings, use the builder pattern":

```rust
use std::fs::File;
use rodio::Decoder;

let file = File::open("audio.mp3").unwrap();
let len = file.metadata().unwrap().len();

let decoder = Decoder::builder()
    .with_data(file)
    .with_byte_len(len)      // Enable seeking and duration calculation
    .with_seekable(true)     // Enable seeking operations
    .with_hint("mp3")        // Optional format hint
    .with_gapless(true)      // Enable gapless playback
    .build()
    .unwrap();
```

`Decoder` struct page (https://docs.rs/rodio/0.22.2/rodio/decoder/struct.Decoder.html):

```rust
pub struct Decoder<R: Read + Seek>(/* private fields */);

// impl<R: Read + Seek + Send + Sync + 'static> Decoder<R>
pub fn builder() -> DecoderBuilder<R>
pub fn new(data: R) -> Result<Self, DecoderError>
pub fn new_looped(data: R) -> Result<LoopedDecoder<R>, DecoderError>
pub fn new_wav(data: R)  -> Result<Self, DecoderError>   // features `hound` or `symphonia-wav`
pub fn new_flac(data: R) -> Result<Self, DecoderError>   // features `claxon` or `symphonia-flac`
pub fn new_vorbis(data: R) -> Result<Self, DecoderError> // features `lewton` or `symphonia-vorbis`
pub fn new_mp3(data: R)  -> Result<Self, DecoderError>   // features `minimp3` or `symphonia-mp3`
pub fn new_aac(data: R)  -> Result<Self, DecoderError>   // feature `symphonia-aac`
pub fn new_mp4(data: R)  -> Result<Self, DecoderError>   // feature `symphonia-isomp4`
```

`TryFrom` impls present: `TryFrom<File>` (yields `Decoder<BufReader<File>>`), `TryFrom<BufReader<R>>`,
`TryFrom<Cursor<T>>`. The 0.21 changelog explains why this matters:
> "Breaking: `symphonia::SeekError` has a new variant `RandomAccessNotSupported`. This error usually means that you are trying to seek backward without `is_seekable` or `byte_len` set: use `Decoder::try_from` or `DecoderBuilder` for that."

**Practical upshot for your player: use `Decoder::try_from(File::open(path)?)`** — it wraps in
`BufReader` and sets `byte_len` for you, which is what makes seeking work.

Sample type gotcha: the docs.rs build uses `all-features = true`, so
`impl Iterator for Decoder<R>` renders as `type Item = f64` on that page. With **default**
features `Float = f32`, so `Decoder::Item` is `f32`. Both satisfy `Player::append`'s
`f32: FromSample<S::Item>` bound. Do not hardcode `f64` sample types.

## A4. Features — and no C compiler needed

Source of truth: https://raw.githubusercontent.com/RustAudio/rodio/v0.22.2/Cargo.toml and
https://docs.rs/crate/rodio/0.22.2/features (that page reports **40 feature flags, 19 enabled by default**).

Verbatim from `Cargo.toml`:

```toml
default = [
    "playback",
    "recording",
    "flac",
    "mp3",
    "mp4",
    "vorbis",
    "wav",
    "dither",
]
flac = ["symphonia-flac"]
mp3 = ["symphonia-mp3"]
mp4 = ["symphonia-isomp4", "symphonia-aac"]
vorbis = ["symphonia-ogg", "symphonia-vorbis"]
wav = ["symphonia-wav", "symphonia-pcm"]
symphonia-all = ["symphonia/all-formats", "symphonia/all-codecs"]
```

| Format | Feature(s) to enable | Default? | Backend |
|---|---|---|---|
| MP3 | `mp3` → `symphonia-mp3` → `symphonia/mp3` | **YES** | Symphonia. Alt: `minimp3` (off) |
| FLAC | `flac` → `symphonia-flac` → `symphonia/flac` | **YES** | Symphonia. Alt: `claxon` (off) |
| WAV | `wav` → `symphonia-wav` **+** `symphonia-pcm` | **YES** | Symphonia. Alt: `hound` (off) |
| AAC (raw ADTS, .aac) | `symphonia-aac` (pulled in by `mp4`) | **YES** | Symphonia |
| AAC / M4A in MP4 | `mp4` → `symphonia-isomp4` + `symphonia-aac` | **YES** | Symphonia |
| OGG / Vorbis | `vorbis` → `symphonia-ogg` + `symphonia-vorbis` | **YES** | Symphonia. Alt: `lewton` (off) |
| **Opus** | — | **NOT SUPPORTED** | — |

Additional non-default flags: `symphonia-aiff`, `symphonia-caf`, `symphonia-mkv`,
`symphonia-alac`, `symphonia-adpcm`, `symphonia-mp1`, `symphonia-mp2`, `symphonia-mpa`,
`symphonia-simd`, `symphonia-all`, `claxon`, `hound`, `minimp3`, `lewton`, `wav_output`,
`tracing`, `experimental`, `noise`, `rand`, `rand_distr`, `64bit`, `wasm-bindgen`,
`crossbeam-channel`, `recording`, `playback`, `dither`.

### Opus: not supported by rodio 0.22.2

I verified this **by absence**, across three independent places — there is no positive
"unsupported" statement to quote:

1. Neither https://docs.rs/crate/rodio/0.22.2/features (all 40 flags) nor `Cargo.toml` has any
   `opus` / `symphonia-opus` feature.
2. `Decoder` has no `new_opus` constructor (full associated-function list on the `Decoder` page).
3. `https://api.github.com/repos/RustAudio/rodio/contents/examples?ref=v0.22.2` lists every
   example; there is `music_flac.rs`, `music_m4a.rs`, `music_mp3.rs`, `music_ogg.rs`,
   `music_wav.rs` — and **no opus example**.

**UNVERIFIED**: I did not fetch Symphonia 0.5.5's feature list, so "Symphonia has no Opus decoder"
is inference from rodio's flag list, not a direct quote. If Opus is a hard requirement for your
player, plan on a separate decoder crate feeding rodio a `Source`.

### C compiler / system libraries on Windows

For a **default-features** `rodio = "0.22.2"` build on Windows: **no C compiler is required.**

Evidence: the only C-backed dependency in the manifest is `minimp3_fixed = { version = "0.5.4", optional = true }`,
which is reachable only through the non-default `minimp3` feature. Everything in the default set —
`symphonia` 0.5.5 (pure Rust), `cpal` 0.17, `dasp_sample`, `thiserror`, `num-rational`, `rand`/`rand_distr` —
is Rust. The README (https://raw.githubusercontent.com/RustAudio/rodio/v0.22.2/README.md) lists ALSA
dev files as a **Linux-only** requirement ("Dependencies (Linux only)").

**UNVERIFIED (narrowly)**: I did not fetch `cpal` 0.17's own build script/manifest, so "cpal needs no
C toolchain on Windows" is inference from the dependency graph, not a quoted cpal statement. It is
consistent with the README's Linux-only requirement note.

## A5. Detecting that a track ended

The recommended, officially-exampled approach is appending a `rodio::source::EmptyCallback` after
the track. Full example at
https://raw.githubusercontent.com/RustAudio/rodio/v0.22.2/examples/callback_on_end.rs:

```rust
let stream_handle = rodio::DeviceSinkBuilder::open_default_sink()?;
let player = rodio::Player::connect_new(stream_handle.mixer());

let file = std::fs::File::open("assets/music.wav")?;
player.append(rodio::Decoder::try_from(file)?);

// lets increment a number after `music.wav` has played. We are going to use atomics
// however you could also use a `Mutex` or send a message through a `std::sync::mpsc`.
let playlist_pos = Arc::new(AtomicU32::new(0));

// The closure needs to own everything it uses. We move a clone of
// playlist_pos into the closure. That way we can still access playlist_pos
// after appending the EmptyCallback.
let playlist_pos_clone = playlist_pos.clone();
player.append(rodio::source::EmptyCallback::new(Box::new(move || {
    println!("empty callback is now running");
    playlist_pos_clone.fetch_add(1, Ordering::Relaxed);
})));
```

Adapted for a UI that must advance to the next track (the callback runs on the audio thread, so it
should only set a flag):

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use rodio::source::EmptyCallback;

// Appending a guard sentinel after each track. The callback fires when the
// track ahead of it finishes.
fn enqueue(&self, path: &Path, index: usize, done: Arc<AtomicUsize>) {
    let file = std::fs::File::open(path).unwrap();
    self.player.append(rodio::Decoder::try_from(file).unwrap());
    self.player.append(EmptyCallback::new(Box::new(move || {
        done.store(index + 1, Ordering::Relaxed);   // signal "advance" only
    })));
}

// once per UI frame:
let next = self.done.swap(usize::MAX, Ordering::Relaxed);
if next != usize::MAX { self.start_track(next); }
```

Other primitives, and why they are weaker:

- `Player::empty() -> bool` — true when the queue is drained. Ambiguous as an "advance" signal:
  it is also true *before* you append the first source, and false while more tracks are queued.
  Fine as a secondary "is anything playing" check.
- `Player::get_pos() -> Duration` — "Returns the position of the sound that's being played. This
  takes into account any speedup or delay applied." Use it for a progress bar, **not** for
  end-detection (the last frame of a track is a moving target, and it resets on seek).
- `Player::sleep_until_end(&self)` — blocks the calling thread; never call it from the UI thread.
- `player.len() -> usize` — number of sounds still queued.

## A6. Thread safety / storing in an egui app struct

Auto-trait tables, quoted from the docs.rs pages:

| Type | Send | Sync | Other |
|---|---|---|---|
| `Player` | ✅ `impl Send for Player` | ✅ `impl Sync for Player` | `RefUnwindSafe`, `UnwindSafe`, `!Freeze`; `impl Drop` |
| `MixerDeviceSink` | ✅ `impl Send` | ✅ `impl Sync` | **`!RefUnwindSafe`, `!UnwindSafe`**; `impl Drop` |
| `Mixer` | ✅ `impl Send` | ✅ `impl Sync` | `Clone` |
| `DeviceSinkBuilder<E>` | ✅ `impl<E> Send` | ✅ (`where E: Sync`) | — |
| `Decoder<R>` | ✅ `impl Send` | ✅ `impl Sync` | `!RefUnwindSafe`, `!UnwindSafe` |

**Answer: yes — store `MixerDeviceSink` + `Player` directly in your egui `App` struct.** All three
types are `Send + Sync`, and all the control methods (`set_volume`, `pause`, `play`, `try_seek`,
`append`, `empty`) take `&self`, so you can call them from `update()` while holding `&self`.

Two real caveats:

1. **Drop stops audio.** `MixerDeviceSink` — *"When dropped playback will end, and the associated
   OS-Sink will be disposed"*. `Player` — *"Dropping the `Player` stops all its sounds."*
   So neither may be a temporary. Also note `MixerDeviceSink`'s drop **prints to stderr** unless you
   call `handle.log_on_drop(false)`; the docs say "Not recommended during development".
2. **`!UnwindSafe` on `MixerDeviceSink`** only matters if you wrap UI code in `std::panic::catch_unwind`;
   use `AssertUnwindSafe` there if so.

`Player::detach(self)` exists if you want sounds to outlive the handle, and `Player::new()` returns
`(Player, SourcesQueueOutput)` if you want to build your own pipeline instead of connecting to a mixer.

---

# PART B — lofty 0.25.4

## B1. Reading tags from a path

`lofty::read_from_path` **does exist**. Signature
(https://docs.rs/lofty/0.25.4/lofty/probe/fn.read_from_path.html):

```rust
pub fn read_from_path<P>(path: P) -> Result<TaggedFile, FileParseError>
where
    P: AsRef<Path>,
```
> "Read a `TaggedFile` from a path. NOTE: This will determine the `FileType` from the extension"

It is re-exported at the crate root (https://docs.rs/lofty/0.25.4/lofty/, Re-exports):
`pub use crate::probe::read_from;` and `pub use crate::probe::read_from_path;` — so both
`lofty::read_from_path` and `lofty::probe::read_from_path` work.

Crate-root examples, verbatim:

```rust
use lofty::probe::Probe;
use lofty::read_from_path;

// This will guess the format from the extension
// ("mp3" in this case), but we can guess from the content if we want to.
let path = "test.mp3";
let tagged_file = read_from_path(path)?;

// Let's guess the format from the content just in case.
// This is not necessary in this case!
let tagged_file2 = Probe::open(path)?.guess_file_type()?.read()?;
```

`Probe` (https://docs.rs/lofty/0.25.4/lofty/probe/struct.Probe.html):

```rust
pub struct Probe<R: Read> { /* private fields */ }
pub const fn new(reader: R) -> Self
pub fn with_file_type(reader: R, file_type: FileType) -> Self
pub fn file_type(&self) -> Option<FileType>
pub fn set_file_type(self, file_type: FileType) -> Self
pub fn options(self, options: ParseOptions) -> Self
pub fn into_inner(self) -> R

// impl Probe<BufReader<File>>
pub fn open<P>(path: P) -> Result<Self, FileParseError> where P: AsRef<Path>

// impl<R: Read + Seek> Probe<R>
pub fn guess_file_type(self) -> Result<Self>          // io::Result<Self>
pub fn read(self) -> Result<TaggedFile, FileParseError>
```
> `Probe::read` "Panics: If an unregistered `FileType` (`FileType::Custom`) is encountered."

Return types: `TaggedFile` is `lofty::file::TaggedFile`; the error is `lofty::error::FileParseError`.

**Trait-import gotcha (important).** `TaggedFile`'s only *inherent* method is
`change_file_type(&mut self, FileType)`. `properties()`, `first_tag()`, `primary_tag()`, `tags()`,
`tag()`, `contains_tag_type()` all come from **traits** and are invisible unless imported:

```rust
use lofty::file::{AudioFile, TaggedFileExt};   // REQUIRED for .properties() and .first_tag()
use lofty::tag::Accessor;                      // REQUIRED for .title()/.artist()/...
```
(`lofty::prelude` re-exports commonly used items if you prefer one import.)

## B2. Reading title / artist / album / track / genre / year / duration

`lofty::tag::Accessor` — full trait, verbatim
(https://docs.rs/lofty/0.25.4/lofty/tag/trait.Accessor.html). **All 30 methods are *provided*
methods; none are required:**

```rust
pub trait Accessor {
    fn artist(&self) -> Option<Cow<'_, str>> { ... }
    fn set_artist(&mut self, _value: String) { ... }
    fn remove_artist(&mut self) { ... }
    fn title(&self) -> Option<Cow<'_, str>> { ... }
    fn set_title(&mut self, _value: String) { ... }
    fn remove_title(&mut self) { ... }
    fn album(&self) -> Option<Cow<'_, str>> { ... }
    fn set_album(&mut self, _value: String) { ... }
    fn remove_album(&mut self) { ... }
    fn genre(&self) -> Option<Cow<'_, str>> { ... }
    fn set_genre(&mut self, _value: String) { ... }
    fn remove_genre(&mut self) { ... }
    fn track(&self) -> Option<u32> { ... }
    fn set_track(&mut self, _value: u32) { ... }
    fn remove_track(&mut self) { ... }
    fn track_total(&self) -> Option<u32> { ... }
    fn set_track_total(&mut self, _value: u32) { ... }
    fn remove_track_total(&mut self) { ... }
    fn disk(&self) -> Option<u32> { ... }
    fn set_disk(&mut self, _value: u32) { ... }
    fn remove_disk(&mut self) { ... }
    fn disk_total(&self) -> Option<u32> { ... }
    fn set_disk_total(&mut self, _value: u32) { ... }
    fn remove_disk_total(&mut self) { ... }
    fn date(&self) -> Option<Timestamp> { ... }
    fn set_date(&mut self, _value: Timestamp) { ... }
    fn remove_date(&mut self) { ... }
    fn comment(&self) -> Option<Cow<'_, str>> { ... }
    fn set_comment(&mut self, _value: String) { ... }
    fn remove_comment(&mut self) { ... }
}
```

Corrections to the assumptions in your question:

- The string getters are `Option<Cow<'_, str>>` — **not** `Option<&str>`. Confirmed for
  `title`, `artist`, `album`, `genre`, `comment`.
- `track`, `track_total`, `disk`, `disk_total` return `Option<u32>` (**not** strings).
- There is **no `year()` method** and no `album_artist()` on `Accessor`. The date accessor is
  `date() -> Option<Timestamp>`. The trait docs say: *"`Accessor` intentionally exposes only a small
  set of common tag metadata. Additional standard fields remain available using `ItemKey` through the
  `Tag`'s item APIs."*
- Setters take `String` by value and **overwrite** all existing values (*"setter methods **overwrite**
  existing values, rather than append"*).

`Timestamp` (https://docs.rs/lofty/0.25.4/lofty/tag/items/timestamp/struct.Timestamp.html) has
**public fields**, not accessor methods:

```rust
pub struct Timestamp {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
    pub hour: Option<u8>,
    pub minute: Option<u8>,
    pub second: Option<u8>,
}
```
So the year is `tag.date().map(|d| d.year)  // Option<u16>`.
`Timestamp` also implements `FromStr`, `Display`, `Ord` (`"2024-06-15T14:30:00".parse().unwrap()`).
Path: `lofty::tag::items::timestamp::Timestamp`, re-exported as `lofty::tag::items::Timestamp`.

**Duration.** `TaggedFile` implements `AudioFile` with `type Properties = FileProperties`
(https://docs.rs/lofty/0.25.4/lofty/file/struct.TaggedFile.html):

```rust
fn properties(&self) -> &Self::Properties      // -> &FileProperties, from trait AudioFile
```

`FileProperties` (https://docs.rs/lofty/0.25.4/lofty/properties/struct.FileProperties.html),
`#[non_exhaustive]`:

```rust
pub fn duration(&self) -> Duration                  // <-- total duration, infallible
pub fn overall_bitrate(&self) -> Option<u32>        // kbps
pub fn audio_bitrate(&self) -> Option<u32>          // kbps
pub fn sample_rate(&self) -> Option<u32>            // Hz
pub fn bit_depth(&self) -> Option<u8>
pub fn channels(&self) -> Option<u8>
pub fn channel_mask(&self) -> Option<ChannelMask>
```

So: `let dur: std::time::Duration = tagged_file.properties().duration();`
Note `duration()` returns a bare `Duration` (not `Option`), and it is **zeroed out** if you disabled
property reading — `Probe::read` docs: *"If `read_properties` is false, the properties will be zeroed out."*

## B3. Cover art / embedded picture

**Correction to your premise:** in 0.25.4 the generic `Tag` does *not* expose pictures through
`TagItem`/`ItemValue`. Pictures live in a dedicated list on `Tag`.

`Tag` (https://docs.rs/lofty/0.25.4/lofty/tag/struct.Tag.html), exact signatures:

```rust
pub const fn new(tag_type: TagType) -> Self
pub fn pictures(&self) -> &[Picture]                                  // "Returns the stored Pictures as a slice"
pub fn picture_count(&self) -> u32
pub fn get_picture_type(&self, picture_type: PictureType) -> Option<&Picture>
pub fn push_picture(&mut self, picture: Picture)
pub fn set_picture(&mut self, index: usize, picture: Picture)
pub fn remove_picture(&mut self, index: usize) -> Picture             // panics if index out of bounds
pub fn remove_picture_type(&mut self, picture_type: PictureType)
pub fn get(&self, item_key: ItemKey) -> Option<&TagItem>
pub fn get_string(&self, item_key: ItemKey) -> Option<&str>
pub fn get_binary(&self, item_key: ItemKey, convert: bool) -> Option<&[u8]>
pub fn insert(&mut self, item: TagItem) -> bool
pub fn insert_text(&mut self, item_key: ItemKey, text: String) -> bool
pub fn take(&mut self, key: ItemKey) -> impl Iterator<Item = TagItem> + use<'_>
pub fn remove_key(&mut self, key: ItemKey)
```
`get_picture_type` is documented as *"a convenience method for retrieving a picture by type without
manually searching through the slice returned by `Tag::pictures`."*

`Picture` (https://docs.rs/lofty/0.25.4/lofty/picture/struct.Picture.html):

```rust
pub fn data(&self) -> &[u8]                          // "Returns the Picture data as borrowed bytes."
pub fn into_data(self) -> Vec<u8>
pub fn mime_type(&self) -> Option<&MimeType>         // "determined from the data, and is immutable"
pub fn pic_type(&self) -> PictureType
pub fn set_pic_type(&mut self, pic_type: PictureType)
pub fn description(&self) -> Option<&str>
pub fn set_description(&mut self, description: Option<String>)
```
`Picture` is `Clone + Debug + Eq + Hash + Send + Sync`. It is built with
`Picture::unchecked(Vec<u8>).pic_type(..).mime_type(..).description(..).build()`.

`PictureType` (https://docs.rs/lofty/0.25.4/lofty/picture/enum.PictureType.html) is
`#[non_exhaustive]` with 22 variants; the front cover is **`PictureType::CoverFront`**
(documented as "Front cover"). Others include `CoverBack`, `Icon`, `OtherIcon`, `Leaflet`, `Media`,
`LeadArtist`, `Artist`, `Conductor`, `Band`, `Composer`, `Lyricist`, `RecordingLocation`,
`DuringRecording`, `DuringPerformance`, `ScreenCapture`, `BrightFish`, `Illustration`, `BandLogo`,
`PublisherLogo`, `Other`, and `Undefined(u8)`.

**About the typed `TagItem`/`ItemKey` layer you asked about** — verified
(https://docs.rs/lofty/0.25.4/lofty/tag/enum.ItemValue.html):

```rust
pub enum ItemValue {
    Text(String),
    Locator(String),
    Binary(Vec<u8>),
}
```
There is **no `Picture` variant**. `ItemValue::Binary(Vec<u8>)` is raw bytes and does not carry a
MIME type or a `PictureType`. Therefore: **for cover art use `Tag::pictures()` /
`Tag::get_picture_type()`; do not route it through `ItemValue`.** (Format-specific types such as
`Id3v2Tag` do hold `AttachedPictureFrame`s, but the generic `Tag` normalises them into `Vec<Picture>`.)

### Complete, compiling-shape example for reading a file

```rust
use std::borrow::Cow;
use std::time::Duration;

use lofty::error::FileParseError;
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::read_from_path;
use lofty::tag::{Accessor, Tag};

fn read_tags(path: &str) -> Result<(), FileParseError> {
    let tagged_file: TaggedFile = read_from_path(path)?;

    // primary_tag()/first_tag() come from TaggedFileExt
    let tag: Option<&Tag> = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());

    if let Some(tag) = tag {
        let title:  Option<Cow<'_, str>> = tag.title();
        let artist: Option<Cow<'_, str>> = tag.artist();
        let album:  Option<Cow<'_, str>> = tag.album();
        let genre:  Option<Cow<'_, str>> = tag.genre();

        let track:       Option<u32> = tag.track();
        let track_total: Option<u32> = tag.track_total();

        // There is no year(); date() -> Option<Timestamp>, and Timestamp.year is a public field.
        let year: Option<u16> = tag.date().map(|d| d.year);

        // Front cover
        let cover: Option<&Picture> = tag.get_picture_type(PictureType::CoverFront);
        if let Some(pic) = cover {
            let bytes: &[u8] = pic.data();
            let mime: Option<&MimeType> = pic.mime_type();
            let _ = (bytes.len(), mime);
            // e.g. egui: image from bytes; mime distinguishes Jpeg vs Png
        }

        let _ = (title, artist, album, genre, track, track_total, year);
    }

    // properties() comes from AudioFile
    let duration: Duration = tagged_file.properties().duration();
    let _ = duration;
    Ok(())
}
```

**UNVERIFIED**: how to turn `&MimeType` into a `&str` (I confirmed `MimeType` is an enum at
`lofty::picture::MimeType` and that its variants include `Jpeg` and `Png` — used as
`MimeType::Jpeg` / `MimeType::Png` in the docs' own examples — but I did not fetch the `MimeType`
page, so I cannot quote a `Display`/`as_str` method). Match on the variants, or `format!("{mime:?}")`,
until you check that page.

## B4. Features and build requirements

Only **three** feature flags exist — there are **no per-format gates** in 0.25.4
(https://docs.rs/crate/lofty/0.25.4/features and
https://docs.rs/crate/lofty/0.25.4/source/Cargo.toml.orig):

```toml
default                   = ["id3v2_compression_support"]
id3v2_compression_support = ["dep:flate2"]
serde                     = ["dep:serde"]
```
> "This version has **2** feature flags, **1** of them enabled by **default**." (docs.rs counts the
> two non-default/optional ones this way; the manifest above is authoritative.)

So: **all supported formats work out of the box with default features.** `serde` is opt-in and only
adds `Serialize`/`Deserialize` impls (e.g. on `PictureType`).

**C compiler: not required.** Runtime deps in the manifest are `data-encoding`, `byteorder`,
`lofty_attr`, `log`, `ogg_pager`, `paste`, and — only via the default `id3v2_compression_support` —
`flate2` for ID3 compressed frames. No `-sys` crate and no build script appears in the manifest.

**UNVERIFIED (narrowly)**: lofty does not disable flate2's default features, so flate2's own default
backend applies. I did not fetch flate2's manifest in this session to prove that backend is the
pure-Rust `miniz_oxide` rather than a C zlib. If a no-C-toolchain build ever fails, pin flate2's
backend explicitly with `default-features = false, features = ["rust_backend"]`.

## B5. Supported formats

Verbatim table from the crate root (https://docs.rs/lofty/0.25.4/lofty/):

| File Format | Metadata Format(s) |
| --- | --- |
| AAC (ADTS) | `ID3v2`, `ID3v1` |
| Ape | `APE`, `ID3v2`\*, `ID3v1` |
| AIFF | `ID3v2`, `Text Chunks` |
| FLAC | `Vorbis Comments`, `ID3v2`\* |
| MP3 | `ID3v2`, `ID3v1`, `APE` |
| MP4 | `iTunes-style ilst` |
| MPC | `APE`, `ID3v2`\*, `ID3v1`\* |
| Opus | `Vorbis Comments` |
| Ogg Vorbis | `Vorbis Comments` |
| Speex | `Vorbis Comments` |
| WAV | `ID3v2`, `RIFF INFO` |
| WavPack | `APE`, `ID3v1` |

\* "The tag will be **read only**, due to lack of official support."

Corroborated by the `FileProperties` `From` impls (AAC, Aiff, Ape, Flac, Mp4, Mpc, Mpeg, Opus,
Speex, Vorbis, WavPack, Wav) and the `TaggedFile` `From` impls (Aac, Aiff, Ape, Flac, Mp4, Mpc,
Mpeg, Opus, Speex, Vorbis, Wav, WavPack).

**Note the asymmetry with Part A: lofty reads Opus metadata fine, but rodio 0.22.2 cannot decode
Opus audio.**

---

# Summary of things I could NOT verify

1. Verbatim `fn rodio::play` signature (shape taken from a working example, not quoted).
2. That cpal 0.17 needs no C toolchain on Windows (inference from the dependency graph + the
   README's Linux-only ALSA note; cpal's own manifest not fetched).
3. That Symphonia 0.5.5 has no Opus decoder (inference from absence in rodio's flags).
   Opus's *absence from rodio* is verified three ways.
4. That flate2's default backend is pure Rust (lofty's manifest doesn't override it; flate2's
   manifest not fetched).
5. Any `MimeType` → `&str` conversion API on `lofty::picture::MimeType`.
6. `rodio::DeviceSinkConfig` getters beyond `config()` (page not fetched).
