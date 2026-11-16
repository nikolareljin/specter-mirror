# Kiro development

This revision supplies the frame contract, synthetic rendering proof, Kiro
steering and seven implementation specs. No completed Kiro implementation
session or hook execution is recorded yet.

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
