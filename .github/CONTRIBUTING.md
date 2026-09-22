# Issues

Two kinds of issue exist here, and each has its own template.

A **finding** is something you found while doing something else, which
disagrees with something you believed, and which you are deliberately not
fixing now. A **research question** is the opposite: a question the plan chose
to pursue, cut to one number or one decision, published so that several can be
answered at once by agents that never hold the game. The sections down to
[Citing](#citing) are about a finding, and *issue* in them means a finding.
[Research](#research) is about the question.

## A finding

A finding is something you found while doing something else, which disagrees
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

### Where an issue is not

| Destination | Holds | Why an issue is not this |
| --- | --- | --- |
| `plan.md` | intent | nobody has decided to act on an issue yet |
| a spec's `Unresolved` | a design choice nobody has made | an issue is an observation, not a choice |
| `docs/rules/` | a mechanism the build decides, with its evidence | a rule is understood; an issue is not |
| `work/research/` | a question someone set out to answer | an issue was not sought |

The four are where issues go when they leave. Until then an issue stays open
precisely because it fits none of them.

### Admission

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

### Shape

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

### Exits

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

### Citing

Issues now outlive any checkout, so a link to one resolves. That does not make
an issue citable from a tracked document.

`docs/rules/` and `docs/spec/` state what is understood. An issue is by
definition not understood: nobody has decided whether the document is wrong or
the reading was. A rules document or a spec that cites an issue is telling its
reader to go and find out, which is the one thing a document may not do. The six
exits are how a finding becomes citable: an issue that matters enough to
reference from `plan.md`, a spec or a rules document has, by that fact, already
earned its promotion.

A commit message may name an issue, and the pull request that lands exit 5
closes it with a `Closes #N` line at the end of its body, before the
`Co-Authored-By` trailer. A commit records a moment; it does not promise the
moment is still open.

## Research

A research question is sought, which is exactly what a finding is not. It has
an owner once it is claimed, it has a size, and it has a definition of done
written before anyone starts. `plan.md` describes the loop a mechanism is
studied by; this section is how that loop is split between the one session
that holds the game and the agents that answer questions without it.

### Why the game is the boundary

One machine runs one game, and one process drives it: the Adapter serves one
client, refuses a second at the same level, and a higher level takes the game
by tearing down the capture in flight. So exactly one session records, and
that session is the one that keeps `plan.md`: the **keeper**. Everything a
question needs from the game is done before the question is published, and
everything it turns out to need later is asked for, never taken.

A **claimant** therefore never runs a script that declares `game:`.
`mechcore run <script> --check` says whether one does, without touching
anything. The Adapter's level check is what happens when this rule is broken,
and it is a loss, not a safeguard: the keeper's next claim ends the claimant's
capture unpublished.

### Where the evidence lives

Three places hold what a question is answered from, and none of them is this
repository's history.

| Evidence | Where | Who reaches it |
| --- | --- | --- |
| the build's decompilation, one directory per build, and its symbol index as a release | [`qbisi/mechcore-decomp`](https://github.com/qbisi/mechcore-decomp), private, put under `work/decomp/<build>/` by `scripts/decomp.py sync`, which reuses what the machine already holds | a cloud session through GitHub: attached to the session or through the GitHub connector in Claude Code, through a read-only token in `MECHCORE_DECOMP_TOKEN` on that repository alone in Codex |
| the native replays and the battle documents converted from them | [`qbisi/mechcore-replay`](https://github.com/qbisi/mechcore-replay), public, fetched by `scripts/replay.py sync` at the commit `replay/REPLAY_REV` pins | anyone |
| the recordings and sidecars a question is answered against | a release of this repository named `oracle/issue-<n>`, one per question, published by `scripts/oracle.py` | anyone, while the question is open |

A recording is an unstable product: it is captured again when the build
changes, added to when a claimant asks for more, and gone when its question
closes. What the repository keeps of it is what reproduces it, the layout and
the script under `tests/<topic>/`, and what it established, the hashes the
topic's `regressions.mcscript` pins. The release is deleted when the issue
closes, tag included, and no tracked document cites one; an issue and a
commit message may.

### What the keeper does before opening one

1. **Cuts the question to one number or one decision**, and writes what each
   hypothesis predicts for it, before recording. A question two hypotheses
   answer alike is not cut yet.
2. **Reads the build**, and names in the issue what a claimant will read: the
   class, the method, the address, the file under `mechcore-decomp`. The
   reading's result is written as plainly as what it cannot answer, so the
   claimant starts where the keeper stopped rather than from the beginning.
3. **Commits the fixtures** under `tests/<topic>/` on master: the layouts, each
   with the reason it exists, and the script that records them with its
   predictions as `expect` lines. The script is the experiment; the issue
   points at it.
4. **Opens the issue** with the Research template, labelled `research`. The
   `Touches` block names the modules the answer may change; two open questions
   never share one, which is what makes them answerable in parallel.
5. **Records, and publishes the oracle.** The files the script wrote under
   `/tmp/mechcore/<topic>/<script>/` go to the release `oracle/issue-<n>` with
   `scripts/oracle.py publish <n> <path>...`, and the issue's `Oracle` block
   lists each file with its tick count and physics hash.

A question that needs a new instrumentation profile or a change to the Adapter
is the keeper's own and is not published.

### Claiming

A claimant claims by opening a **draft** pull request from a branch named
`research/<n>-<slug>`, based on master, whose body says which agent is
working it and where, and ends with `Closes #<n>` before its `Co-Authored-By`
trailer. The body is the commit master will hold, so it is written as one. The draft is the claim: an issue
with a draft already open is taken, and a second claimant yields to the first.
Assignment is not used, because every agent here acts through one account.

The claim is also where the working record lives. A claimant's clone dies with
its session, so the discarded hypotheses, the first divergences and the ticks
that decided go into the pull request's thread and the issue's, which outlive
any checkout. That is the research directory of a claimed question.

### Answering without the game

1. `python3 scripts/oracle.py fetch <n>` puts the oracle's files exactly where
   the record script would have written them, under `/tmp/mechcore/`. It uses
   `gh` when one is signed in and plain HTTPS otherwise, because the release is
   public.
2. `mechcore fight verify <recording>...` simulates each recording's own
   layout and names the first tick the simulator differs at; `mechcore fight
   compare --fields <group>` reads the content layer. Iterate on the model
   until the ticks agree, then on the reading until the rule says why.
3. Land it the way `plan.md` says a mechanism lands: the rule and its scope in
   `docs/rules/`, the number's source stated, a refusal in the code for what
   the scope does not cover, and the topic's offline `regressions.mcscript`
   pinning each oracle fight the simulator can run, physics and content hash
   both. `work/research/README.md`'s two gates decide what a rule may claim.
4. Before every push: `cargo fmt --all`, the clippy and test lines
   `AGENTS.md` names, and `scripts/check-scripts.sh`, which runs every offline
   script and is the same loop CI runs. Every pin that held before has to
   hold after; a pin that moves is a finding to explain, not a table to edit.
5. Mark the pull request ready when the issue's `Accept` block holds. A commit
   carries its model's `Co-Authored-By` trailer, as `AGENTS.md` requires.

### Asking for a capture

When the oracle cannot separate the hypotheses that remain, the claimant
designs the fight that would, and asks for it:

1. commits the layout and its record script under `tests/<topic>/` on the
   branch, with the value each hypothesis predicts as an `expect` line;
2. comments on the pull request what the fight separates, and adds the label
   `capture`.

The keeper runs the script, publishes the files to the question's release,
answers with the hashes and the tick counts, and removes the label. A capture
that needs a field the current profiles do not read is a request for the
keeper's own work, and the comment says which field.

### Acceptance

CI green is necessary and not sufficient. The keeper reads a ready pull
request, and reading means:

- running `scripts/check-scripts.sh` on the branch, and the question's oracle
  through `fight verify` and `fight compare` in the content layer, and the
  sidecar where the profile recorded one;
- reading the rule against the two gates: a mechanism closes on the build's
  code or on the risk-adjusted record, a number traces to one of three
  sources;
- checking that the answer goes through the mechanism the build uses, and adds
  no branch keyed on a kind of object that the build does not have.

The keeper may push to the branch. When the pull request holds, the keeper
adds the label `accepted`, and `automerge.yml` merges it: a pull request that
claims an issue, by its `research/` branch or its `Closes` line, is not merged
without that label. Merging closes the issue, which is a finding's fifth exit:
the question's content now lives in an assertion. The keeper then deletes the
release with `scripts/oracle.py delete <n>`.

A question that turns out not to close inside its scope still lands what
holds, with a refusal for the rest, and its issue closes with a comment that
says what remains and what would reopen it. What remains is a new question
when the keeper cuts it, not this one kept open.
