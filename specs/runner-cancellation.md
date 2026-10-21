# Cooperative command-runner cancellation

This is an additive Rust library contract. Public `assimilate project/source`
remains stateless. No CLI flags, background worker, scheduler, configuration,
provider selection or persistent cancellation journal is added.

## API

`mmr::dream::CommandCancellation` is `Clone + Debug + Default` with private
state. Default is uncancelled. `request_cancel(&self)` makes an idempotent,
one-way request shared by every clone; `is_cancelled(&self)` reports that
request. There is no reset. Requesting cancellation is not acknowledgement that
an operation has stopped.

`CommandDreamRunner::run_with_cancellation(&DreamRequest,
&CommandCancellation) -> anyhow::Result<DreamRunnerOutput>` adds cancellation to
the same request/output/deadline budgets. `CommandRunnerError::Cancelled` is an
additive typed failure category. The existing `DreamRunner::run` signature and
uncancelled behavior remain unchanged.

## Lifecycle

- Observe an already-cancelled handle before allocation/spawn and check again
  before spawning. A pre-cancelled invocation never creates its child.
- A watch future wakes the existing scoped runtime across OS threads and Tokio
  runtimes. It races the owned pipe/deadline future, without detached tasks or
  blocking reader threads.
- When observed, cancellation drops pending pipe operations and executes the
  same bounded process-group/direct-child cleanup used for timeout/errors.
- Join the scoped worker before returning. Report `Cancelled` only after
  cleanup succeeds; ownership/group/direct-reap errors retain precedence.
- Recheck cancellation before returning validated output. Cancellation can race
  successful completion or an already-completed failure; callers must not infer
  a durable publication fence from this handle. A future worker must check its
  persistent cancellation/fencing state in its commit transaction.

The token is in-process and not durable. SIGKILL or a process crash bypasses Rust
cleanup; this API does not guarantee orphan termination after owner death. Unix
cleanup still requires waitid/WNOWAIT, exclusive child reaping and ordinary
same-group descendants. Escaped process groups and privileged descendants are
outside containment. Non-Unix descendant cleanup remains unverified and is not
guaranteed. A synchronous call still blocks its calling thread; async callers
should use an appropriate blocking boundary and await its completion.

## Verification

Synthetic local tests must demonstrate cancellation before spawn (marker
negative control), blocked stdin, stdout/stderr beyond pipe capacity, inherited
pipe descriptors and ordinary descendants, direct-child reaping, repeated
requests/token reuse and token isolation, cross-thread and nested-runtime
wakeups, and cleanup-error precedence. Existing uncancelled behavior and the
full repository verification loop must pass. No live histories, provider
credentials or network endpoints are required.
