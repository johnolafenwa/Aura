"""Concurrency: a worker pool, a three-stage pipeline, and a fan-out of tasks.

The pool runs N small jobs through four workers fed by a bounded queue. The
pipeline moves N text records through parse and transform stages, each a task
joined to the next by a bounded queue. The fan-out starts N / 50 tasks and
joins every result. The work per message is small, so this measures task
switching and queue hand-off more than raw computation. Python runs it on one
asyncio event loop; Aura and Rust (tokio) use their multi-threaded schedulers.
"""
import asyncio
import sys

WORKERS = 4
CAPACITY = 64
STOP = -1


def mix(value):
    x = value
    for _ in range(16):
        x = (x * 31 + 7) % 1000003
    return x


async def produce(jobs, n):
    for i in range(n):
        await jobs.put(i)
    for _ in range(WORKERS):
        await jobs.put(STOP)


async def worker(jobs, results):
    while True:
        job = await jobs.get()
        if job == STOP:
            return
        await results.put(mix(job))


async def source(out, n):
    for i in range(n):
        await out.put("order-" + str(i) + ":" + str(i * 7919 % 10007))
    await out.put(None)


async def parse(lines, out):
    while True:
        line = await lines.get()
        if line is None:
            await out.put(STOP)
            return
        fields = line.split(":")
        await out.put(int(fields[1]) + len(fields[0]))


async def transform(values, out):
    while True:
        value = await values.get()
        if value == STOP:
            await out.put(STOP)
            return
        await out.put(mix(value))


async def compute(seed):
    return mix(seed * 13)


async def run(n):
    jobs = asyncio.Queue(CAPACITY)
    results = asyncio.Queue(CAPACITY)
    tasks = [asyncio.create_task(produce(jobs, n))]
    for _ in range(WORKERS):
        tasks.append(asyncio.create_task(worker(jobs, results)))
    pool_sum = 0
    pool_max = 0
    for _ in range(n):
        value = await results.get()
        pool_sum += value
        pool_max = max(pool_max, value)
    await asyncio.gather(*tasks)

    lines = asyncio.Queue(CAPACITY)
    values = asyncio.Queue(CAPACITY)
    mixed = asyncio.Queue(CAPACITY)
    stages = [
        asyncio.create_task(source(lines, n)),
        asyncio.create_task(parse(lines, values)),
        asyncio.create_task(transform(values, mixed)),
    ]
    items = 0
    pipeline_sum = 0
    while True:
        value = await mixed.get()
        if value == STOP:
            break
        items += 1
        pipeline_sum += value
    await asyncio.gather(*stages)

    fan_out = n // 50
    handles = [asyncio.create_task(compute(i)) for i in range(fan_out)]
    task_sum = 0
    for handle in handles:
        task_sum += await handle
    print(
        f"task_pool jobs={n} pool_sum={pool_sum} pool_max={pool_max} items={items} "
        f"pipeline_sum={pipeline_sum} tasks={fan_out} task_sum={task_sum}"
    )


if __name__ == "__main__":
    asyncio.run(run(int(sys.argv[1])))
