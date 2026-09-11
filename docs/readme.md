# Document conventions

Two kinds of document live here, and which kind a document is decides what it
may contain and who has to change when reality disagrees with it.

| Directory | Answerable to | When it disagrees with reality |
| --- | --- | --- |
| `rules/` | the build | the document is wrong, and re-measuring is the fix |
| `spec/` | its readers | the code is wrong, and changing the code is the fix |

A rules document describes a mechanism the game decides. It is pinned to a
build, it carries the evidence for what it claims, and it goes stale when the
game changes. Most are the prose beside a machine-readable table in `config/`:
the table is what code reads, the document says what the entries mean.

A spec defines a shape something must conform to. `spec/` is filed by the crate
that owns each contract, so a document and the code that must satisfy it are
found the same way.

The rest of this file is the convention for a spec, apart from the language rule
below, which governs both. A rules document has no other convention yet.

## Language

Every primary document is written in English. A translation is optional, lives
beside its primary as `<name>.zh.md`, and is linked from the first line of both
files:

```markdown
[简体中文](officers.zh.md)
[English](officers.md)
```

A translation is a second copy of one document, not a second document. It is
classed with the primary rather than on its own, it carries no content the
primary lacks, and when the two disagree the primary wins.

English prose may quote a name in another language where the name is the
identifier: an index that lists the game's official Simplified Chinese names in
a table is an English document.

## A spec's shared spine

Two sections are required of every spec, whatever it specifies.

**`## Scope` is the first section.** It says what this contract defines, what it
deliberately does not, and how it divides with its siblings. A reader who stops
after Scope should already know whether this is the document they want.

**`## Unresolved` is the last section.** It holds design choices nobody has made
yet, each one stated as the question it is. A spec with nothing open writes
`None.` under the heading rather than dropping it, because an absent section
cannot be told apart from a question nobody asked.

Unresolved is for decisions, never for work. "Whether a battle records how the
match ended" is a decision. "The deployment executor does not exist" is work,
and work belongs in `plan.md`.

Four things are banned from every spec:

- a Status section, or any statement of how much is implemented;
- corpus counts, pass rates and measurements;
- the evidence or provenance that produced a rule, including decompilation
  traces and file hashes;
- an account of how the design was arrived at, or of what an earlier version got
  wrong.

None of that is worthless. It belongs in `rules/`, in `plan.md`, in
`work/research/`, or in the commit that made the change.

## Three kinds of spec

The spine is shared. What sits between Scope and Unresolved depends on what is
being specified.

| Kind | Documents | Required sections |
| --- | --- | --- |
| Document format | [layout](spec/document/layout.md), [state](spec/document/state.md), [turn](spec/document/turn.md), [battle](spec/document/battle.md), [action](spec/document/action.md), [mcfr](spec/mcfr/mcfr.md), [unit-rules](spec/simulation/unit-rules.md) | `Document shape`, `Normal form`, `Excluded fields` |
| Interface contract | [adapter](spec/adapter/adapter.md), [mcscript](spec/mechcore/mcscript.md), [session](spec/mechcore/session.md) | each operation with its arguments, its result and what it refuses; an error taxonomy |
| Algorithm contract | [rvo](spec/simulation/rvo.md), [quadtree](spec/simulation/quadtree.md) | the determinism invariants; the fidelity boundary |

A required section may be delegated to the sibling that owns it, and the spec
that delegates says where. [action.md](spec/document/action.md) carries neither
`Normal form` nor `Excluded fields`, because a sequence of actions is a turn's
collection rather than an action's, and [turn.md](spec/document/turn.md) states
both. What is not allowed is silence: a missing section with no owner named is a
gap.

`Excluded fields` is the counterpart of Scope, and it is what stops a settled
question from being reopened: it names what the format deliberately leaves out
and why, so a reader who expected a field learns it was considered.

`Normal form` states the canonical order of every collection, so that two
documents describing one position are the same document.

An error taxonomy is what makes an interface contract testable. A caller has to
be able to distinguish a refusal it should retry from one it should not.

A fidelity boundary states what the reproduction is faithful to and where it
stops. It is a scope statement, not a progress report, which is the distinction
the next section is about.

## Boundary is not progress

Both say "X is not covered here", and telling them apart is the rule that does
the most work.

A boundary is a property of the contract. It stays true until the contract
changes, and it belongs in Scope. Progress is a property of the code. It is
stale the day it is written, and it belongs in `plan.md`.

The test is to rewrite the sentence in the present tense with no *current*,
*yet*, *still*, or *not implemented*. If it survives, it is a boundary. If it
collapses into nothing, it was progress.

| Written as | Reads as | Belongs in |
| --- | --- | --- |
| "the simulator does not yet load map objects" | nothing survives | `plan.md` |
| "a layout carries no map objects" | a rule a reader can act on | Scope |
| "the adapter cannot capture a full state today" | nothing survives | `plan.md` |
| "a capture covers the layout projection" | the contract's edge | Scope |

The same test catches a section title. `Current adapter compiler` and
`Loading and current kernel boundary` both name a moment rather than a contract.

## Worked examples

[action.md](spec/document/action.md), [battle.md](spec/document/battle.md),
[turn.md](spec/document/turn.md) and [state.md](spec/document/state.md) follow
this convention and are the ones to copy. The remaining specs predate it.

Five documents have a Chinese primary and no English one, so they do not yet
meet the language rule: [map](rules/map.md), [terrain](rules/terrain.md),
[mcfr](spec/mcfr/mcfr.md), [rvo](spec/simulation/rvo.md) and
[quadtree](spec/simulation/quadtree.md). Each becomes an English primary with
its present text kept as the `.zh.md` translation.
