# Issues

An issue is something you found while doing something else, which disagrees
with something you believed, and which you are deliberately not fixing now.

All three clauses do work. *Found while doing something else* separates an issue
from research, which is a question someone chose to pursue. *Disagrees with
something you believed* separates it from a wish, which nothing contradicts.
*Deliberately not fixing now* separates it from a fix, which you should just
make.

An issue is a holding pen, not a backlog. It has no owner, no priority and no
status beyond open. Do not assign it, do not label it by urgency, and do not
sort it against other issues. The only thing that happens to an issue is that
it leaves.

## Where an issue is not

| Destination | Holds | Why an issue is not this |
| --- | --- | --- |
| `plan.md` | intent | nobody has decided to act on an issue yet |
| a spec's `Unresolved` | a design choice nobody has made | an issue is an observation, not a choice |
| `docs/rules/` | a mechanism the build decides, with its evidence | a rule is understood; an issue is not |
| `work/research/` | a question someone set out to answer | an issue was not sought |

The four are where issues go when they leave. Until then an issue stays open
precisely because it fits none of them.

## Admission

Four tests. An observation has to pass all four to be worth an issue.

**It was found, not sought.** It surfaced while you were doing something else.
If you went looking, what you have is research, and it belongs in
`work/research/` with the question that drove it.

**It is a discrepancy, not a preference.** Something observed disagrees with
something written, asserted or assumed. "The panel sweep reports 665 of 668"
contradicts a document that claims all of them. "Capture feels slow" contradicts
nothing, and is a plan item or nothing at all.

**Fixing it now would derail what you are doing.** If it is a two-line change in
a file already open, make the change. What earns an issue is the finding you
chose to walk past.

**Re-finding it would cost real work.** A number a command prints on demand does
not need an issue; cite the command instead. A finding that took a live run, a
diagnostic build or a corpus sweep does, and the issue has to carry enough to
avoid paying that cost twice.

## Shape

The Finding template is the only way to open one, and it states the form. The
title names the discrepancy rather than the symptom, as a sentence: "Three
rounds break the energy-tower skill claim", not "sweep is wrong". Four blocks
are required, one line or one block each.

`Found` says what you were doing when it surfaced. GitHub stamps the filing
date, so write a date only when you found it on a different day than you filed
it.

`Observed` carries the measurement and the command that reproduces it.

`Contradicts` must name the document, the assertion or the commit it disagrees
with. When the belief was never written down, say that in as many words. An
issue whose expectation is unstated cannot be closed, because nothing decides
whether the disagreement went away.

`Walked past because` says what chasing it would have derailed.

Evidence goes in the issue body. An issue that points at a scratch directory
dies when the directory does, and a reader on the web has no checkout at all.
Paste the numbers, the command and the output.

## Exits

An issue leaves in exactly six ways. Five are promotions, one is a death.

1. **To `plan.md`**, when someone decides to act. The issue becomes intent.
2. **To a spec's `Unresolved`**, when the discrepancy turns out to be a design
   choice nobody made rather than a defect.
3. **To `docs/rules/`**, when the game turns out to work that way and the
   mistaken belief was ours.
4. **To `work/research/`**, when it is worth pursuing as a question in its own
   right. The issue is replaced by the investigation.
5. **Into an assertion**, when the fix lands and a test now holds it. The
   issue's content moves into the test, which is the only place that cannot go
   stale unnoticed.
6. **Closed as not planned**, when the observation was wrong, or when the thing
   it disagreed with no longer exists.

The first five close as completed, and the closing comment says which exit was
taken and where the content landed. The sixth closes as not planned. Nothing
else closes an issue. In particular an issue never ages out: one that has sat
open for months is telling you something, and closing it on a date throws that
away.

An issue that leaves by any of the first five paths has to be reachable from
where it landed. Copy the evidence across; do not link back to the issue you are
about to close.

## Citing

Issues now outlive any checkout, so a link to one resolves. That does not make
an issue citable from a tracked document.

`docs/rules/` and `docs/spec/` state what is understood. An issue is by
definition not understood: nobody has decided whether the document is wrong or
the reading was. A rules document or a spec that cites an issue is telling its
reader to go and find out, which is the one thing a document may not do. The six
exits are how a finding becomes citable: an issue that matters enough to
reference from `plan.md`, a spec or a rules document has, by that fact, already
earned its promotion.

A commit message may name an issue, and a commit that lands exit 5 closes it
with a `Closes #N` trailer after the body. A commit records a moment; it does
not promise the moment is still open.
