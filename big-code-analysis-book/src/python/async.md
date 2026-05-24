# Async patterns

`bca.analyze` is CPU-bound: the work is a tree-sitter parse plus
the metric passes, both of which release the GIL on the Rust side
via PyO3's `Python::detach`. The canonical async pattern is
therefore `asyncio.to_thread`:

```python
{{#include ../../../big-code-analysis-py/examples/async_patterns.py:20:42}}
```

## Why `to_thread`, not native `async`

`bca.analyze` is a synchronous Python function backed by
synchronous Rust code — there is no `await` boundary inside it.
Wrapping it in `asyncio.to_thread`:

1. Schedules the call on the default thread pool.
2. Lets other coroutines progress while the parse + metric pass
   runs.
3. Returns the result back to the calling coroutine when done.

Because the Rust side releases the GIL across the heavy work,
several `to_thread(bca.analyze, ...)` calls genuinely run in
parallel — this is not co-operative I/O multiplexing, it is real
multi-core utilisation gated on the thread pool's size.

## Custom executors

For a tighter cap on the worker count, hand `to_thread` a
purpose-built executor:

```python
import asyncio
from concurrent.futures import ThreadPoolExecutor

import big_code_analysis as bca

async def analyze_many(paths: list[str]) -> list[object]:
    loop = asyncio.get_running_loop()
    with ThreadPoolExecutor(max_workers=8) as pool:
        return await asyncio.gather(
            *(loop.run_in_executor(pool, bca.analyze, p) for p in paths)
        )
```

Eight workers on an 8-core machine is the comfortable upper bound
for purely CPU-bound work; raising it further oversubscribes the
machine and trades throughput for context-switch overhead.

## Streaming results

`asyncio.as_completed` lets you start consuming results as soon
as the first analysis finishes — useful when the per-file work
varies wildly in cost (a 5 KB file vs a 500 KB generated bundle):

```python
import asyncio
import big_code_analysis as bca

async def first_failure(paths: list[str]) -> str | None:
    """Return the path of the first file with cyclomatic > 50."""
    tasks = [asyncio.create_task(asyncio.to_thread(bca.analyze, p)) for p in paths]
    try:
        for coro in asyncio.as_completed(tasks):
            result = await coro
            if result is None:
                continue
            if result["metrics"]["cyclomatic"]["sum"] > 50:
                return result["name"]
    finally:
        for t in tasks:
            t.cancel()
    return None
```

The `finally`-block cancellation matters: `as_completed` does not
auto-cancel pending tasks when the caller returns early, so a
leaked task can keep running on the thread pool well after the
async function returns.

## Anti-pattern: calling `bca.analyze` directly in a coroutine

```python
# Don't do this.
async def bad(path: str) -> dict | None:
    return bca.analyze(path)  # blocks the event loop on every call
```

`async def` does not make the body asynchronous. Without
`to_thread` or an explicit executor, every coroutine that calls
`bca.analyze` stalls the event loop for the full duration of the
parse — other tasks waiting on I/O, timers, or queues all freeze
until the parse returns. The `to_thread` wrapper is one line and
makes the difference between a responsive server and a
single-threaded one.

## When `analyze_batch` is the better fit

If you are processing a static, finite list of paths and do not
need streaming results, [`bca.analyze_batch`](batch.md) is
simpler than `gather(*to_thread(...))`: it runs sequentially on
the calling thread but never raises on per-file errors. Wrap the
whole `analyze_batch` call in `asyncio.to_thread` to keep the
event loop responsive:

```python
import asyncio
import big_code_analysis as bca

async def batch(paths: list[str]) -> list[object]:
    return await asyncio.to_thread(bca.analyze_batch, paths)
```

This trades the per-file parallelism of `gather` for the
simpler error model of `analyze_batch`. Pick `gather` when you
want both parallelism and typed `OSError` dispatch; pick
`to_thread(analyze_batch, paths)` when you want one async call
and the never-raise contract.
