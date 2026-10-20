# Polling driver: Registry and in-place event storage

This is the design for the `jiaojing/compio` fork's `fix/poll-events-reuse`
branch. It replaces the earlier `Option<Events>` and restoration-guard fix.
The upstream baseline is `3d9933787fc3e6c9b0e8ce9241b568cd40c862f6`;
the earlier fork fix is `196faa705f407ea02d970c465399ffaf9fd33ea6`.
This work does not include an upstream pull request or a stable backport.

## Problem

The upstream polling driver temporarily moves `self.events` out with
`std::mem::take` so an event-processing closure can borrow both the buffer and
the whole `Driver`. In polling 3.11.0, `Events::default()` allocates a buffer
for 1024 events. Each call through that helper constructs a temporary buffer
that is discarded when the original buffer is restored.

On macOS arm64, the standalone regression reports 1000 allocations requesting
32,768,000 bytes for 1000 warmed, notified polls on unmodified upstream. This
is allocation churn; it does not measure payload copies, resident memory or
latency. Empty timed polls can return before the helper, so the claim is not
that every call to `poll` allocates.

The borrowing conflict comes from fd helpers taking `&mut Driver` while the
event iterator borrows `Driver.events`. Those helpers only need fd queue
state and access to the poller.

## Decision and ownership

Introduce a private, concrete `Registry` in the existing polling module:

```rust
#[derive(Default)]
struct Registry {
    queues: HashMap<RawFd, FdQueue>,
}
```

| Owner | State and responsibility |
| --- | --- |
| `Driver` | Event buffer, waiting and timeout handling, wake state, blocking task completions, event classification and AIO dispatch |
| `Registry` | Per-fd read/write queues, interest registration, pending-operation resubmission, removal, cancellation and fd deregistration |
| `Notify` | Existing poller ownership and wake coordination, shared with wakers through the existing `Arc` |

Registry methods borrow the `Poller` owned by the same driver's `Notify`.
Keeping that borrow explicit avoids another poller owner or an additional
`Arc`. All construction and call sites remain private to this module; callers
must use the same poller throughout a registry's lifetime. An empty registry
is a valid default, without registering anything in the kernel.

`Driver` does not inspect or mutate the queue map. Its drop implementation
asks the registry to deregister all remaining fds while the poller is still
alive. Deregistration errors during drop continue to be ignored, matching
upstream behavior.

## Event processing

`events` is an ordinary, always-present `Events` field. Polling clears and
fills this existing buffer, then iterates it in place. The fd branch calls:

```rust
self.registry.poll_one(&self.notify.poll, event, fd)?;
```

The immutable event-buffer borrow and mutable registry borrow refer to
different fields. No container is moved out and no replacement is created.
The existing queue helpers move onto `Registry`; their algorithms and the
ordering of their poller operations stay the same.

The event loop remains in `Driver`, including platform-gated AIO handling.
An immediately invoked closure retains the old helper's local return scope:
an error or early return leaves the event loop, then the driver performs the
existing final `set_awake()` before returning that result. The notification
reset, zero-timeout selection, post-wait `set_awake()` and completion draining
also retain their existing order.

On an unwinding panic the buffer remains owned by `Driver`, so restoring it
does not require a guard. This does not introduce a promise that arbitrary
operation panics leave all registration state recoverable, nor does it add
a new panic-time wake-state update.

## Invariants and scope

- Registry operations preserve existing queue ordering, one-shot interest
  renewal, submission rollback, resubmission and cancellation behavior.
- Existing fd-validity and operation-key lifetime requirements remain in
  force. Moving those operations adds no new unsafe mechanism.
- Public APIs, dependencies, toolchain requirements and backend selection
  do not change. Linux io_uring and Windows IOCP implementations are untouched.
- The wrapper stores the existing map directly. It adds no heap allocation,
  lock, dynamic dispatch, shared ownership or event-buffer copy.
- The old `get_queue` assertion remains an existing operation invariant;
  the temporary-storage `expect` calls and guard disappear.
- This fix is independent of UDP, WebSocket and application-specific workloads.

## Alternatives

The earlier option-and-guard implementation fixes allocation churn with a
smaller mechanical patch, but represents temporary absence of a buffer that
logically exists for the driver's whole lifetime.

Associated or free helpers taking queue-map and poller references also permit
disjoint borrowing. They avoid a new owner type but repeat the same dependency
pair across the helper chain. Registry is chosen to keep the queue and interest
management rules together, accepting a larger method-movement diff.

A temporary struct containing borrowed map and poller references would bundle
those parameters. It is unnecessary once the queue state has its own private
owner. A separate trait, runtime abstraction or new shared poller is not needed.

Tokio is a reference for separating event-buffer access from event dispatch,
not a template for this type. Its `mio::Registry` is an OS registration handle;
its readiness tracking and token-based dispatch differ from Compio's fd
operation queues.

## Validation and limits

Keep the standalone `poll_allocations` integration test. It selects the
polling backend, warms it up, and counts allocation requests on the calling
thread during 1000 notified polls. The expected result is zero calls and zero
requested bytes. The original upstream version was separately observed to
fail this regression.

Run driver, runtime and network code tests, including cancellation, concurrent
read/write readiness, buffer pools, operation drop, timeout and wake tests.
Run Clippy with warnings denied and the repository's nightly formatter on the
changed Rust files. Exact commands and results belong in [results.md](results.md).

macOS exercises the kqueue polling backend. It does not establish Linux epoll,
FreeBSD/Solaris AIO or Windows correctness, or any Linux io_uring/AWS latency
gain. Existing stable-backport application benchmark results do not substitute
for validating this master-branch implementation.

The change is a follow-up commit on the existing fork branch. Reverting that
commit restores the option-and-guard implementation and its tests; no public
API migration is required.
