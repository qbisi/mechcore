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

The language rule below governs both. After it, one section states the
convention for a rules document and the rest of the file states the convention
for a spec.

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

## A rule is something a reader can rely on

A rules document states rules a reader can act on without going back to check.
That is the whole standard, and it decides what may appear there.

A rule is a claim about the build, carried with the scope it holds in. The scope
is not a hedge, it is what makes the rule usable: a reader who knows where a
rule stops can work inside it and knows to stop at the edge. So state what a
rule does not cover beside the rule itself, because a claim just outside a
closed scope is unverified however obvious an extension of it looks.

Three things read like rules and are not.

- **What our own code reproduces.** That a simulator matches a corpus tick for
  tick is a fact about the simulator. The game is not answerable to it.
- **A single run's trace.** Tick numbers lifted from one replay record an
  observation. The rule is whatever that observation demonstrated, and it is the
  rule that belongs here.
- **Confidence labels and review status.** `strongly_supported` and its
  neighbours say how well a question was answered, not what the game does.

All three belong in `work/research/`, whose README holds the evidence gates a
finding passes on its way out. [rules/combat.md](rules/combat.md) is the worked
example: every entry is a scoped claim about one build, with the boundary it
does not cover stated next to it.

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

A third case is neither. Something observed disagrees with this spec, and nobody
has yet decided whether the spec is wrong or the reading was. That is an issue,
and [CONTRIBUTING.md](../.github/CONTRIBUTING.md) says what becomes one and how
it leaves.

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
| Document format | [layout](spec/document/layout.md), [state](spec/document/state.md), [battle](spec/document/battle.md), [action](spec/document/action.md), [mcfr](spec/mcfr/mcfr.md), [unit-rules](spec/simulation/unit-rules.md) | the shape of the document, `Normal form`, `Excluded fields` |
| Interface contract | [adapter](spec/adapter/adapter.md), [cli](spec/mechcore/cli.md), [mcscript](spec/mechcore/mcscript.md), [session](spec/mechcore/session.md) | each operation with its arguments, its result and what it refuses; an error taxonomy |
| Algorithm contract | [rvo](spec/simulation/rvo.md), [quadtree](spec/simulation/quadtree.md) | the determinism invariants; the fidelity boundary |

A required section may be delegated to the sibling that owns it, and the spec
that delegates says where. What is not allowed is silence: a missing section
with no owner named is a gap.

`Excluded fields` is the counterpart of Scope, and it is what stops a settled
question from being reopened: it names what the format deliberately leaves out
and why, so a reader who expected a field learns it was considered.

The shape may be one section called `Document shape`, as in
[layout.md](spec/document/layout.md), or a run of named sections that between
them account for every field, as in [battle.md](spec/document/battle.md) and
[state.md](spec/document/state.md). What matters is that no field is
undescribed, not which heading describes it. Prefer named sections once one
`Document shape` would run long enough that a reader cannot find a field in it.

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

## What a machine can check

`scripts/check-docs.py` enforces the mechanical half of this file. Run it from
the repository root, and make it pass before a document change lands:

```bash
python3 scripts/check-docs.py
```

It checks four things.

- **Every relative link resolves**, the `#anchor` half included. A renamed
  section silently breaks every link into it, which is the failure most likely
  to happen without anyone noticing.
- **Every readme is spelled `README.md`.**
- **Every spec under `docs/spec/` is classified** into one of the three kinds
  above. A new spec fails until it is classified, so none sits unchecked
  because nobody remembered it existed.
- **Every converted spec opens with `Scope` and closes with `Unresolved`**, and
  carries no section titled `Status` or beginning `Current`.

The checker also holds the list of specs that predate this convention, and that
list is the only record of which ones are left. It fails in both directions: a
spec still on the list that has started conforming is an error too, so
converting one means deleting its line.

What no checker can do is tell whether a sentence is true. Nothing catches a
rules document that quietly stopped describing the build, or a spec the code
has drifted away from. Those need a reader, which is what the rest of this file
is written for.

## Worked examples

Every spec follows this convention, so any of them answers a question about
form. For a document format copy [battle.md](spec/document/battle.md) or
[state.md](spec/document/state.md); for an interface contract
[adapter.md](spec/adapter/adapter.md), whose error taxonomy is the fullest; for
an algorithm contract [rvo.md](spec/simulation/rvo.md), whose fidelity boundary
names what it does not cover rather than implying coverage.

The checker holds the list, not this page, and it fails when a new spec is
neither classified nor conforming. So the claim in the paragraph above cannot
quietly stop being true.

Every document here now has an English primary, and nine carry a `.zh.md`
translation beside it. A new one starts in English; a translation is optional
and follows.
