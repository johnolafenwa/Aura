# Shaping Data

Most programs get easier to read once the data has names. A loose bag of strings and integers becomes a `Job` with an `id`, a `queue`, and an `attempts` counter. A value that was "sometimes a number and sometimes an error" becomes a `Result` with two variants. The behaviour that used to drift between helper functions moves onto the type.

This chapter introduces Aurora's two data shapes — **classes** and **enums** — together with **methods**, **copy classes**, and **generics**. It is deliberately not a feature checklist. The through-line is how to decide which shape fits your domain.

## When To Use What

A useful first cut:

- Use a **class** when every field is present at the same time.
- Use an **enum** when exactly one variant is present at a time.
- Use a **method** when behaviour belongs to the type.
- Use a **free function** when behaviour coordinates several types.

The rest of the chapter fills those decisions in.

## Start With A Class

Imagine a small job runner. A job has an identifier, a queue name, and an attempt count:

```python
class Job:
    id: int32
    queue: String
    attempts: int32 = 0
```

Construct an instance with named fields:

```python
job = Job(id=42, queue="image")
```

Fields can have defaults. The caller above did not supply `attempts`, so it starts at `0`.

By default, classes are **move types**. A bare class parameter borrows; write
`own` to transfer ownership:

```python
def consume(job: own Job):
    print(job.id)

job = Job(id=42, queue="image")
consume(job)
# job has been moved into consume; using it again is a compile error.
```

When a helper only needs to look at a job, borrow it:

```python
def describe(job: borrow Job) -> String:
    return job.queue + "#" + job.id.to_string()
```

The caller keeps the value and can use it again. The call site writes `describe(job)`; Aurora reads the borrow form from the parameter type.

## Add Methods

Methods are functions declared inside a class. The **receiver** — how `self` is named in the signature — says what the method is allowed to do.

```python
class Job:
    id: int32
    queue: String
    attempts: int32 = 0

    def bump(borrow mut self):
        self.attempts += 1

    def label(self) -> String:
        return self.queue + "#" + self.id.to_string()
```

Use it:

```python
mut job = Job(id=42, queue="image")
job.bump()
print(job.label())
```

Receiver forms:

| Receiver | What it can do |
| --- | --- |
| `self` | Read fields without taking ownership; this is the default spelling. |
| `borrow self` | Explicit synonym for shared `self`. |
| `borrow mut self` | Mutate fields on a mutable receiver. |
| `own self` | Consume the instance. |
| no receiver | Associated method called on the type, not an instance. |

A borrowed method cannot move an owned field out of `self`. Clone when you need to return an owned copy:

```python
class User:
    name: String

    def name_copy(self) -> String:
        return self.name.clone()
```

Returning `self.name` directly would move the `String` through a shared borrow, which the compiler rejects. The clone makes the intention explicit and the reader does not have to guess.

An associated method is called on the type itself — useful for constructors and factories:

```python
class Counter:
    value: int32 = 0

    def zero() -> Counter:
        return Counter()
```

```python
counter = Counter.zero()
```

## Copy Classes

Some records are so small that treating them as move values is more ceremony than it is worth. When every field is itself a copy type, declare the class `copy class`:

```python
copy class Offset:
    x: int32
    y: int32
```

Copy classes duplicate on assignment:

```python
a = Offset(x=1, y=2)
b = a
print(a.x)
print(b.x)
```

This is not a way to opt out of ownership when it feels inconvenient. Reach for `copy class` when duplication is part of the type's nature — coordinates, simple numeric measurements, identifiers made entirely of copyable fields.

## Model Alternatives With Enums

An enum describes a value that is exactly one of several shapes. A job in flight, for instance, is always in one of four states: queued, running, done, or failed.

```python
enum JobState:
    Queued
    Running(worker: String)
    Done(duration: Duration)
    Failed(message: String)
```

Construct a variant by naming it:

```python
state = JobState.Running(worker="worker-a")
```

`match` then inspects the variant exhaustively:

```python
def render_state(state: borrow JobState) -> String:
    return match borrow state:
        case JobState.Queued:
            "queued"
        case JobState.Running(worker):
            "running on " + worker
        case JobState.Done(_duration):
            "done"
        case JobState.Failed(message):
            "failed: " + message
```

Two details are worth noticing. `match borrow state` inspects the enum without taking ownership, which is important because `state` is itself a `borrow JobState`. And the `_duration` name uses the leading underscore convention for a pattern binding that the body does not read.

When each state carries different data, an enum almost always reads better than a class with many optional fields.

## Combine Classes And Enums

A class can own an enum, and often should. This shape — a stable record with a changing state — is one of the cleanest patterns in Aurora.

```python
class TrackedJob:
    job: Job
    state: JobState = JobState.Queued

    def mark_running(borrow mut self, worker: own String):
        self.state = JobState.Running(worker=worker)

    def mark_failed(borrow mut self, message: own String):
        self.state = JobState.Failed(message=message)
```

The fields that never change live on the class. The field that does change is an enum, so the compiler can help make sure every transition is handled.

## Generic Data

Classes and enums can be parameterised by type. `Box[T]` holds some `T`; `Load[T]` represents a value that has either arrived, is still absent, or has failed:

```python
class Box[T]:
    value: T

enum Load[T]:
    Ready(value: T)
    Empty
    Failed(message: String)
```

Generic types let you write utility data structures without giving up the type of the stored value. [Generics And Traits](/manual/generics-and-traits) in the Manual covers the details.

## Design Notes

Three habits keep Aurora data types clean:

- **Prefer small classes with meaningful fields.** A class with ten unrelated fields is often two classes waiting for names.
- **Prefer enums for domain states.** `JobState.Failed(message=...)` is harder to misuse than a `"failed"` string plus a maybe-empty error field.
- **Prefer methods for type-local behaviour.** A function that reads one class's fields usually belongs to that class. A function that coordinates several types is usually a free function.

The next chapter takes the same ideas into Aurora's standard collections — where the classes and enums we just built start to form programs.

Reference: [Classes](/manual/classes), [Enums And Pattern Matching](/manual/enums-and-match).
