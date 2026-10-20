# Registry implementation validation

Validated on 2026-10-20 on macOS arm64 (Darwin 25.3.0), with Rust 1.98.1.
Formatting used the installed nightly toolchain, Rust 1.101.0-nightly
(`b6eeb93c9`, 2026-10-18). The [design](design.md) describes the ownership
boundary and the preserved return and wake behavior.

## Code tests

```sh
cargo +1.98.1 test --locked \
  -p compio-driver -p compio-runtime -p compio-net \
  --lib --tests \
  --features compio-driver/polling,compio-runtime/time
```

Passed: 92 tests, zero failures.

| Package | Passed |
| --- | ---: |
| compio-driver | 21 |
| compio-runtime | 15 |
| compio-net | 56 |

Coverage includes operation cancellation and drop, managed buffers, timeout,
notification, cross-thread wake, TCP/UDP/Unix I/O, concurrent read/write
readiness and connection errors.

The existing `notified_polls_reuse_event_storage` regression passed. Its
assertion requires **zero allocation calls and zero requested bytes** during
1000 notified polls after eight warmup polls, with the polling backend
explicitly selected. It counts requests on the calling thread, not all
threads in the process. The test remains unchanged from the preceding fork
commit.

The three preceding unit tests specific to taking and restoring an optional
buffer were removed with that mechanism. The real-driver allocation regression
and existing runtime panic tests remain.

This command excludes doctests. A UnixStream ancillary-data example had
previously failed to compile identically on unmodified upstream; this change
does not edit that example, and doctests were not rerun for this validation.

## Lint and formatting

```sh
cargo +1.98.1 clippy --locked \
  -p compio-driver -p compio-runtime -p compio-net \
  --all-targets \
  --features compio-driver/polling,compio-runtime/time \
  -- -D warnings

rustfmt +nightly --edition 2024 --config-path rustfmt.toml \
  --config skip_children=true --check \
  compio-driver/src/sys/driver/poll/mod.rs \
  compio-driver/tests/poll_allocations.rs

git diff --check
```

All passed. Formatting checks cover the changed driver file and retained
allocation test without reformatting unrelated upstream files.

## Source review and limits

Compared the fd queue helper bodies against upstream
`3d9933787fc3e6c9b0e8ce9241b568cd40c862f6`. The changes to `try_get_queue`,
`get_queue`, `submit`, `submit_front`, `renew`, `remove_one`, `cancel_one` and
`poll_one` are owner/access-path changes and explicit poller parameters.
The production file retains the same 15 unsafe blocks; no new unsafe
mechanism or public API is introduced. Driver still owns event classification,
AIO dispatch and wake coordination. Registry now also encapsulates the
existing deregistration loop used during driver drop.

This validation exercises macOS kqueue. Linux epoll and platform-gated
FreeBSD/Solaris AIO paths were not compiled or executed here. No Linux
io_uring, AWS, application benchmark or latency improvement is claimed.

This is the master-based fork branch. The stable 0.12.5 backport and the
application's existing stable dependency pin are separate and were not
migrated by this change. No upstream issue or pull request is part of this
delivery.
