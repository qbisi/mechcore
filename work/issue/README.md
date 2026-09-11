# Issues

An issue is something you found while doing something else, which disagrees
with something you believed, and which you are deliberately not fixing now.

All three clauses do work. *Found while doing something else* separates an issue
from research, which is a question someone chose to pursue. *Disagrees with
something you believed* separates it from a wish, which nothing contradicts.
*Deliberately not fixing now* separates it from a fix, which you should just
make.

An issue is a holding pen, not a backlog. It has no owner, no priority and no
status. The only thing that happens to an issue is that it leaves.

## Where an issue is not

| Destination | Holds | Why an issue is not this |
| --- | --- | --- |
| `plan.md` | intent | nobody has decided to act on an issue yet |
| a spec's `Unresolved` | a design choice nobody has made | an issue is an observation, not a choice |
| `docs/rules/` | a mechanism the build decides, with its evidence | a rule is understood; an issue is not |
| `work/research/` | a question someone set out to answer | an issue was not sought |

The four are where issues go when they leave. Until then an issue sits here
precisely because it fits none of them.

## Admission

Four tests. An observation has to pass all four to be worth a file.

**It was found, not sought.** It surfaced while you were doing something else.
If you went looking, what you have is research, and it belongs in
`work/research/` with the question that drove it.

**It is a discrepancy, not a preference.** Something observed disagrees with
something written, asserted or assumed. "The panel sweep reports 665 of 668"
contradicts a document that claims all of them. "Capture feels slow" contradicts
nothing, and is a plan item or nothing at all.

**Fixing it now would derail what you are doing.** If it is a two-line change in
a file already open, make the change. What earns a file is the finding you chose
to walk past.

**Re-finding it would cost real work.** A number a command prints on demand does
not need a file; cite the command instead. A finding that took a live run, a
diagnostic build or a corpus sweep does, and the file has to carry enough to
avoid paying that cost twice.

## Shape

One file per issue, `work/issue/<slug>.md`, where the slug names the
discrepancy rather than the symptom. Four things are required, one line or one
block each.

```markdown
# Three rounds break the energy-tower skill claim

Found: 2026-09-11, while verifying the reproduction scripts after moving them.

**Observed.** `scripts/battle_invariant_support.py` reports 665 of 668, not a
clean sweep. Rerun with `python3 scripts/battle_invariant_support.py`.

**Contradicts.** `docs/spec/document/battle.md`, which states the debt holds for
every round. The three exceptions are not characterised.

**Walked past because.** It surfaced mid-migration and chasing it would have
left the scripts half-moved.
```

The date is written out because git does not carry one: only this readme is
tracked, so an issue file has no history to read a date from.

`Contradicts` must name the document, the assertion or the commit it disagrees
with. When the belief was never written down, say that in as many words. An
issue whose expectation is unstated cannot be closed, because nothing decides
whether the disagreement went away.

Evidence goes in the file. An issue that points at a scratch directory dies when
the directory does.

## Exits

An issue leaves in exactly six ways. Five are promotions, one is a death.

1. **To `plan.md`**, when someone decides to act. The issue becomes intent, and
   the file goes.
2. **To a spec's `Unresolved`**, when the discrepancy turns out to be a design
   choice nobody made rather than a defect.
3. **To `docs/rules/`**, when the game turns out to work that way and the
   mistaken belief was ours.
4. **To `work/research/`**, when it is worth pursuing as a question in its own
   right. The issue file is replaced by the investigation.
5. **Into an assertion**, when the fix lands and a test now holds it. The
   issue's content moves into the test, which is the only place that cannot go
   stale unnoticed.
6. **Deleted**, when the observation was wrong, or when the thing it disagreed
   with no longer exists.

Nothing else removes an issue. In particular an issue never ages out: one that
has sat here for months is telling you something, and closing it on a date
throws that away.

An issue that leaves by any of the first five paths has to be reachable from
where it landed. Copy the evidence across; do not link to the file you are about
to delete.

## Tracking

This readme is tracked. The issues are not.

That asymmetry is deliberate and it has one consequence worth stating plainly: a
tracked document must never cite an issue, because the reader who follows the
citation finds nothing. The six exits are how a finding becomes citable. An
issue that matters enough to reference from `plan.md`, a spec or a rules
document has, by that fact, already earned its promotion.
