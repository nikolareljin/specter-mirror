# Kiro development

The first task in `01-core-engine` was executed in a Kiro session and reviewed:
RGB24 grayscale conversion, Sobel L1 edges and image-pipeline tests. Both
functions use caller-owned buffers and preserve output on invalid input.
The frame contract and existing mirror behavior are unchanged.

Verification: core tests, formatting, clippy, WASM and i686 compilation.
Review added mixed-gradient and short-output mutation checks. No hook execution
or physical hardware performance is recorded for this task.

The hook definitions target the installed Kiro IDE 0.11.107 surface:
`.kiro/hooks/*.kiro.hook`, `when.type: userTriggered` and
`then.type: runCommand` with a literal command. The installed runtime's
schema and command executor were inspected. Hooks are manual to avoid
repeated builds. Open the project in that version, inspect the definitions
and approve each command before running. Newer releases may use another
schema; check their documentation before migrating. Configuration inspection
is not a claim that a hook ran or that it consumed no credits.

Record genuine completed work here with: spec/task, Kiro version and surface,
changed files, observed hook commands/results, produced tests, architecture
changes and reviewed commit or PR. Keep raw sessions and account data out of
the source. Do not mark tasks complete without implementation evidence.
