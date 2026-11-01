# An I/O project: building a log analyzer

Every service you run writes a log, and sooner or later someone asks a question
the log can answer: how many errors came in overnight, which service produced
them, whether the noise is coming from one place or from everywhere. In this
chapter we'll build `logscan`, a small command-line tool that reads a log file
and prints a report answering those questions. It is the kind of program people
write in an afternoon with a shell pipeline of `grep`, `awk`, and `sort`, and
then rewrite when the pipeline stops fitting on one line.

The project brings together most of what the earlier chapters taught. We'll
read command-line arguments with `sys.args()` and a file with
`fs.read_to_string`, and meet the ownership rules again when we pull a string
out of a list. We'll turn each line of text into a typed `Record` with an
`enum` for its level, which means deciding what to do with lines that don't
fit. We'll count records in dictionaries and print an aligned report. Then,
once the program works, we'll do what you would do with real code: split it
into modules, write tests for each part, and replace the raw error values with
messages a person can act on.

Here is the log we'll analyze. Save it as `app.log` next to `Aura.toml` when
you create the project in the next section:

*Filename: app.log*

```text
2026-10-31T09:14:02 INFO api listening on port 8080
2026-10-31T09:14:05 INFO api GET /users 200
2026-10-31T09:14:06 WARN db slow query took 1840ms
2026-10-31T09:14:09 ERROR api GET /orders 500
2026-10-31T09:14:09 ERROR db connection reset by peer
2026-10-31T09:14:11 INFO worker picked up job 41
connection pool exhausted
2026-10-31T09:14:15 DEBUG worker heartbeat

2026-10-31T09:14:20 INFO api GET /users 200
2026-10-31T09:14:21 NOTICE api config reloaded
2026-10-31T09:14:30 ERROR worker job 41 failed after 3 attempts
```

Most lines have the same shape: a timestamp, a level, the name of the service,
and a free-form message. A few don't. Line 7 lost its prefix, line 9 is blank,
and line 11 uses a level our tool won't know about. Real logs always have lines
like these, and a good part of this chapter is about handling them on purpose
instead of by accident.

By the end, running the finished tool looks like this:

```console
$ aura run src/main.au -- app.log --min-level warn
app.log: 4 records, 2 skipped

by level
  WARN        1
  ERROR       3

by service
  api         1
  db          2
  worker      1

errors by service
  api         1
  db          1
  worker      1

skipped line 7: expected at least 4 fields, found 3
skipped line 11: unknown level `NOTICE`
```

Let's start with an empty package and the arguments.
