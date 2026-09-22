# Shaping Data

This page shows how to give your data names and pick the right shape for it.
A loose bag of strings and integers becomes a `Job` with an `id`, a `queue`,
and an `attempts` counter. A value that is sometimes a number and sometimes
an error becomes a `Result` with two variants. Shared behavior lives on the
type.

Aura has two data shapes, classes and enums. The page also covers methods,
copy classes, and generics.

## When To Use What

A useful first cut:

- Use a **class** when every field is present at the same time.
- Use an **enum** when exactly one variant is present at a time.
- Use a **method** when the behavior belongs to one type.
- Use a **free function** when the behavior coordinates several types.

## Start With A Class

Take a small job runner. A job has an identifier, a queue name, and an
attempt count:

```aura
class Job:
    id: int32
    queue: str
    attempts: int32 = 0
```

Construct an instance with named fields:

```aura fragment
job = Job(id=42, queue="image")
```

A field can have a default. The caller above does not supply `attempts`, so
it starts at `0`.

Classes are move types by default. A bare class parameter borrows the value.
Write `own` to transfer ownership:

```aura fragment
def consume(job: own Job):
    print(job.id)

job = Job(id=42, queue="image")
consume(job)
# job has been moved into consume; using it again is a compile error.
```

When a helper only needs to read a job, borrow it:

```aura fragment
def describe(job: Job) -> str:
    return job.queue + "#" + job.id.to_string()
```

The caller keeps the value and can use it again. The call site writes
`describe(job)`. Aura reads the borrow from the parameter type.

## Add Methods

A method is a function declared inside a class. Its receiver is the way
`self` appears in the signature, and it says what the method may do.

```aura
class Job:
    id: int32
    queue: str
    attempts: int32 = 0

    def bump(mut self):
        self.attempts += 1

    def label(self) -> str:
        return self.queue + "#" + self.id.to_string()
```

Use it:

```aura fragment
mut job = Job(id=42, queue="image")
job.bump()
print(job.label())
```

| Receiver | What it can do |
| --- | --- |
| `self` | Read fields without taking ownership. This is the default spelling. |
| `mut self` | Mutate fields on a mutable receiver. |
| `own self` | Consume the instance. |
| no receiver | Associated method called on the type, not an instance. |

A borrowed method cannot move an owned field out of `self`. When the field
type supports cloning, clone it to return an owned copy:

```aura
class User:
    name: str

    def name_copy(self) -> str:
        return self.name.clone()
```

Returning `self.name` directly would move the `str` out through a shared
borrow, and the compiler rejects that. The clone makes the intent explicit.

An associated method is called on the type itself. It is useful for
constructors and factories:

```aura
class Counter:
    value: int32 = 0

    def zero() -> Counter:
        return Counter()
```

```aura fragment
counter = Counter.zero()
```

## Copy Classes

Some records are so small that moving them is more ceremony than it is worth.
When every field is a copy type, declare the class as a `copy class`:

```aura
copy class Offset:
    x: int32
    y: int32
```

A copy class duplicates on assignment:

```aura fragment
a = Offset(x=1, y=2)
b = a
print(a.x)
print(b.x)
```

Do not use `copy class` to escape ownership when it feels inconvenient. Use
it when duplication is part of the type's nature: coordinates, simple numeric
measurements, and identifiers made only of copyable fields.

## Model Alternatives With Enums

An enum describes a value that is exactly one of several shapes. A job in
flight, for example, is always queued, running, done, or failed:

```aura
enum JobState:
    Queued
    Running(worker: str)
    Done(duration: Duration)
    Failed(message: str)
```

Construct a variant by naming it:

```aura fragment
state = JobState.Running(worker="worker-a")
```

`match` inspects the variant and must cover every case:

```aura fragment
def render_state(state: JobState) -> str:
    return match state:
        case JobState.Queued:
            "queued"
        case JobState.Running(worker):
            "running on " + worker
        case JobState.Done(_duration):
            "done"
        case JobState.Failed(message):
            "failed: " + message
```

Two details matter here:

- `match state` inspects the enum without taking ownership of it.
- `_duration` starts with an underscore. That is the convention for a pattern
  binding the body does not read.

When each state carries different data, an enum almost always reads better
than a class with many optional fields.

## Combine Classes And Enums

A class can own an enum, and often should. A stable record with a changing
state is one of the cleanest patterns in Aura:

```aura fragment
class TrackedJob:
    job: Job
    state: JobState = JobState.Queued

    def mark_running(mut self, worker: own str):
        self.state = JobState.Running(worker=worker)

    def mark_failed(mut self, message: own str):
        self.state = JobState.Failed(message=message)
```

The fields that never change live on the class. The field that changes is an
enum, so the compiler can help you handle every transition.

## Generic Data

Classes and enums can take type parameters. `Box[T]` holds some `T`.
`Load[T]` is a value that has arrived, is still absent, or has failed:

```aura
class Box[T]:
    value: T

enum Load[T]:
    Ready(value: T)
    Empty
    Failed(message: str)
```

Generic types let you write utility data structures and keep the type of the
stored value. [Generics And Traits](/manual/generics-and-traits) in the Manual
covers the details.

## Design Notes

Three habits keep Aura data types clean:

- **Prefer small classes** with meaningful fields. A class with ten unrelated
  fields is often two classes waiting for names.
- **Prefer enums** for domain states. `JobState.Failed(message=...)` is
  harder to misuse than a `"failed"` string plus an error field that may be
  empty.
- **Prefer methods** for behavior local to one type. A function that reads
  one class's fields usually belongs to that class. A function that
  coordinates several types is usually a free function.

The next chapter, [Collections](/learn/collections), puts these classes and
enums into Aura's standard collections.

Reference: [Classes](/manual/classes), [Enums And Pattern Matching](/manual/enums-and-match).
