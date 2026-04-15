# Resource Management

Aurora now supports deterministic scoped cleanup and structured scope management with `with`.

## `with` binds a scoped resource

```aurora
class FileHandle:
    name: String

    def read(borrow self) -> String:
        return self.name

    def close(borrow mut self):
        print("closed " + self.name)

def use_resource() -> Result[String, String]:
    with file = FileHandle(name="demo"):
        print(file.read())
        return Result.Ok("done")
```

The bound resource:

- is available inside the block as a local binding
- is mutable inside the block
- always runs `close(borrow mut self)` when the block exits

The cleanup runs on normal fallthrough and on early `return`.

See:

- [examples/resources/with_resource.au](../examples/resources/with_resource.au)

## Bootstrap rule

In the current compiler bootstrap, a `with` resource must be a class that defines:

```aurora
def close(borrow mut self)
```

with no extra parameters and an implied `None` return type.

## `with ... as ...` for task groups

The bootstrap compiler also supports:

```python
with task_group() as group:
    group.spawn(worker, out.clone())
```

`TaskGroup` is the one builtin non-class value that can currently be used with `with`.

See:

- [examples/concurrency/task_group_select.au](../examples/concurrency/task_group_select.au)
- [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au)

## Current Limits

The current bootstrap `with` implementation does not yet support:

- generic resource types other than `TaskGroup`
- arbitrary enter or exit protocols
- borrowed resource bindings
