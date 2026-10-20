# Browser runtime prototype

The goal is playable, responsive browser games through an embeddable Ring3 API.
The first vertical prototype reuses the complete BottleShip runtime instead of
preserving Ring3's current CPU/Win32 boundary. This is an experimental backend,
not a claim of full game compatibility or a stable released SDK.

## Evidence and assumptions

- The same owned Max Payne v1.01 installation reached its menu and tutorial in
  the locally built pinned reference. Movement and firing worked. Its displayed
  69–76 FPS measured guest Present submissions; this was a short observation.
- The SDK vertical acceptance then opened that same bundle, observed tutorial
  movement and ammo 18→17 at displayed 67–85 FPS, acknowledged actual CPU pause
  and resume, and successfully stopped and removed the frame. Closed-session
  calls rejected. This remains a short local observation, not a compatibility
  guarantee or a game-speed ratio against the old engine.
- A manual save in the normal prologue survived a graceful SDK stop and a new
  session: loading that slot restored the saved doorway position rather than the
  later unsaved entrance position, and movement continued into the next sequence.
- Audio status showed initially zero then positive master input, progressing
  render frames and source cursors, stable context time/frames while paused, and
  renewed progress after resume. This establishes digital signal/lifecycle
  behavior, not audible quality, fresh PCM production or long-play stability.
- The first embedding uses the same origin for the host and runtime frame. The
  host supplies a private, reachable bundle URL; game payloads are never committed.
- BottleShip's CPU, Win32, renderer, audio and filesystem remain together. Its
  relaxed arithmetic policy differs from exact x87. Preserve upstream identity,
  licenses and effective configuration rather than claiming equivalence.
- The user has delegated implementation choices and authorized logical commits
  and push. Existing code and these experiments may be replaced.

## Interface and behavior

The host facade mounts one runtime frame and exposes launch, pause, resume,
status, stop and disposal. The adapter identifies the upstream backend and pin.

The wire namespace is `ring3.runtime.v1`; every request/reply carries its session,
generation and request ID. Validate origin, source and payload before accepting
it. Late or forged messages cannot complete a current operation.

Launch submission is distinct from the first guest presentation. Pause/resume
complete only after the worker acknowledges the actual scheduler transition.
Stop completes after child sessions and accepted engine filesystem/registry
writes are drained, then releases the frame and listeners. This is not a guest
ExitProcess or a flush of private guest CRT buffers. A failed drain must remain
a failure and retain the session for retry or explicit forced disposal. A
previously lost file write cannot be reconstructed by retrying a barrier. No arbitrary
worker evaluation or translation through the old Ring3 CPU/Win32 ABI is used.

Status and lifecycle replies include a cold audio snapshot: context state/time,
volume/mute, reported worklet versions, existing signal counters and at most 32
buffer-control or encoded-source rows per list. Reads do not create a context,
control playback, reset counters or scan PCM. Missing signal storage is null.
Fields are individually atomic, not a coherent transaction; compare only the
same document and registration. Counters wrap at 32 bits. Master peak is a
lifetime maximum before the limiter, and advancing frames may contain silence.
Streaming starvation counts source-block episodes, not browser/device underruns.

## Structure and commands

- `runtime.mjs` / `runtime.d.mts`: dependency-free ESM browser facade and types.
- `backend.lock.json`: immutable upstream source and CPU artifact identities.
- `backend/bridge.patch`: narrow upstream host/worker lifecycle adapter.
- `tools/prepare-backend.mjs`: reproducible isolated backend checkout and build.
- `tests/`: focused protocol/ownership checks and the browser acceptance fixture.

Preparation, with Bun installed:

```sh
node browser-sdk/tools/prepare-backend.mjs --directory .cache/browser-runtime --bun bun
node --test browser-sdk/tests/*.test.mjs
```

Checking out this branch does not switch the existing playground to this
backend. `playground/server.mjs` on port 4173 still loads Ring3's
`ring3_engine.wasm`; disabling its trace changes recording, not the CPU backend.
The experimental SDK runtime must be served separately. After preparation:

```sh
cd .cache/browser-runtime
BS_WGB_ROOTS="/absolute/directory/containing-bundles" bun x --bun vite --host 127.0.0.1 --port 4175 --strictPort
```

Open the acceptance host on port 4175 with the encoded same-origin bundle URL
described below. `BS_WGB_ROOTS` grants the local development route access to that
bundle directory; keep game files outside public assets. After restarting the
development server, create a fresh SDK session rather than resuming a stale tab.

The backend uses its pinned lockfile and builds into a fresh `dist.staging`.
Source pins, emitted CPU hashes and upstream notices are checked before that
candidate replaces `dist`; build or validation failure leaves the current output
intact. Only one generated `dist.previous` is retained. Existing runtime build
metadata identifies disposable generated output; unrecognized fixed paths and
symlinks are refused, while legacy `dist.previous-*` directories are left alone.
A leftover staging directory after process interruption requires inspection
before retry. Prepare and promote only with active runtime sessions stopped:
directory replacement is not a live deployment protocol. The build records
actual tools and emitted JavaScript/Wasm hashes and reuses the pinned CPU binary.
The default reusable checkout is `.cache/browser-runtime`, separate from Cargo's
`target/`; explicit `--directory` paths still work. Selected experiment results
belong under `artifacts/`, outside these replaceable caches.

The distribution includes `ring3-sdk/runtime.mjs` and the acceptance host at
`ring3-sdk/acceptance.html?bundle=<encoded-local-wgb-url>`. The host contains no
game payload. Serving must supply COOP/COEP; private bundles stay outside public
assets and require a same-origin streaming route.

Code uses ESM, small named functions and structured process arguments:

```js
await runtime.pause();
const state = await runtime.status();
```

## Verification and delivery

1. Pin and build the original backend without proprietary payloads.
2. Review the parent/child contract before applying the bridge.
3. Test malformed/stale replies, acknowledgments, timeout and cleanup ownership.
4. In the browser, launch the same private Max Payne bundle through the facade;
   observe menu, tutorial movement and firing.
5. Verify pause/resume and stop from the host; confirm the frame is removed and
   the old session cannot resume. Exercise a real save in a new session, then
   verify audio signal progress and pause/resume/stop through status. Audible
   quality, extended play and broader game compatibility remain separate gates.

Always keep work scoped, record actual results and preserve upstream notices.
Never publish game data, bypass failed lifecycle acknowledgments, silently drop
durable state, or call a launch postMessage a successful gameplay result.

The whole playable engine/SDK objective remains open after this prototype.
