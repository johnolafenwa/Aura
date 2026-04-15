# Resource Management

When you open a file, a network connection, or a task group, you need to ensure it gets cleaned up even if something goes wrong. Aurora's `with` statement provides deterministic scoped cleanup -- the resource is always closed when the block exits, whether by normal completion or early `return`.

If you are coming from Python, this works like Python's `with` statement and context managers.

## `with` Binds A Scoped Resource

```python
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
    # file.close() is called automatically here, even on early return
```

The bound resource:

- is available inside the block as a mutable local binding
- always runs `close(borrow mut self)` when the block exits
- cleanup runs on normal fallthrough, on early `return`, and after `try` propagation

See [examples/resources/with_resource.au](../examples/resources/with_resource.au).

## The Resource Protocol

In the current compiler, a `with` resource must be a class that defines:

```python
def close(borrow mut self):
```

The method must take `borrow mut self`, no extra parameters, and return `None`. Any class that defines this method can be used with `with`.

## `with ... as ...` For Task Groups

The builtin `task_group()` function returns a `TaskGroup`, which is the one non-class value that supports `with`:

```python
with task_group() as group:
    group.spawn(worker, out.clone())
    group.spawn(worker, out.clone())
# leaving the block joins all spawned child tasks
```

Task groups tie child tasks to a lexical scope. When the `with` block ends, any still-running child tasks are joined. You can also cancel early with `group.cancel()`.

See [examples/concurrency/task_group_select.au](../examples/concurrency/task_group_select.au) and [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## Current Limits

- only class types with a `close(borrow mut self)` method and `TaskGroup` can be used with `with`
- no arbitrary enter or exit protocols
- no borrowed resource bindings
