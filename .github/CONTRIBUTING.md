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
an owner once it is worked, it has a size, and it has a definition of done
written before anyone starts. `plan.md` describes the loop a mechanism is
studied by; this section is how that loop is split between the one session
that holds the game, the **keeper**, and the agents it hands the work to.

### Why the game is the boundary

One machine runs one game, and one process drives it: the Adapter serves one
client, refuses a second at the same level, and a higher level takes the game
by tearing down the capture in flight. So exactly one session records, and
that session is the one that keeps `plan.md`. Every other agent never runs a
script that declares `game:`; `mechcore run <script> --check` says whether one
does, without touching anything. A recording an agent needs is asked of the
keeper, which records it; the Adapter's level check is what happens when this
rule is broken, and it is a loss, not a safeguard.

### Referring to other issues

An issue number written in an issue, a pull request or a commit links the two
on GitHub, both ways, and a link that carries no dependency buries the ones
that do. So a number is written only for a dependency: `Closes #n`,
`Blocked by #n`, or a decision that moves another item. Where something came
from is told by its mechanism, its fixture or its file, not by the question
that found it. A tracked file (a document, a script, a layout, a README)
carries no issue number at all, as it carries no release: it outlives both.

### Where the evidence lives

Three places hold what a question is answered from, and none of them is this
repository's history.

| Evidence | Where | Who reaches it |
| --- | --- | --- |
| the build's decompilation, one directory per build, and its symbol index as a release | [`qbisi/mechcore-decomp`](https://github.com/qbisi/mechcore-decomp), private, put under `work/decomp/<build>/` by `scripts/decomp.py sync`, which reuses what the machine already holds | a cloud session through GitHub: attached to the session or through the GitHub connector in Claude Code, through a read-only token in `MECHCORE_DECOMP_TOKEN` on that repository alone in Codex |
| the native replays, one directory per game version | [`qbisi/mechcore-replay`](https://github.com/qbisi/mechcore-replay), public, append-only, fetched at `master` by `scripts/replay.py sync` | anyone |
| the recordings and sidecars a question is answered against | a release of this repository named `oracle/issue-<n>`, one per question, published by `scripts/oracle.py` | anyone |

A recording is an unstable product: it is captured again when the build
changes and added to when a question needs more. What the repository keeps of
it is what reproduces it, the layout and the script under `tests/<topic>/`,
and what it established, the hashes the topic's `regressions.mcscript` pins.
The release outlives its issue, because it is the evidence behind those
hashes: a pin whose recording is gone can be recorded again but not read
again. It is deleted only when a re-recording has replaced what it holds. No
tracked document cites a release; an issue and a commit message may.

### Cutting a question

A question is published with everything its hypotheses need already recorded,
because a recording asked for later costs a round trip through the one game.
The keeper:

1. **Places it against what is open.** A question is placed by the modules
   and files its answer will change. Two questions whose answers change the
   same module are taken one after the other, not side by side, and a
   question that needs another's answer waits for it. Parallel questions that
   shared `modifier/` and the layout compiler blocked and rebased each other;
   the overlap was visible before either was cut.
2. **Cuts it to one number or one decision**, and writes what each
   hypothesis predicts for it, before recording. A question two hypotheses
   answer alike is not cut yet.
3. **Reads the build**, and names in the issue what was read: the class, the
   method, the address, the file under `mechcore-decomp`, and what the reading
   cannot answer.
4. **Designs the fixtures**: one layout per branch the hypotheses take, each
   with the reason it exists, and the script that records them with a
   control. They are written into the issue, not into the repository, until
   the answer lands the ones that separated what they were built to separate.
5. **Records, and checks the coverage.** Every hypothesis has to differ from
   every other in something a recording shows; a fight no hypothesis predicts
   differently is dropped, and a hypothesis no fight separates gets another
   layout and another recording before the issue opens.
6. **Opens the issue** with the Research template, labelled `research`, the
   layouts and the script inline. **The issue says what is observed, never how
   to build it.** `Accept` names the recordings to reproduce, the pins, the
   rule and the refusals; it does not name the module an answer must go
   through, a table to add or a field to claim. That is decided from the
   build at the structure step below, and an issue that prescribed it has
   been wrong: one required a unit's level to be a config table read through
   `Modifier`, and the build multiplies by the level. `Touches` is the
   keeper's forecast, which placing questions reads; it is not a fence, and an
   answer that needs a file outside it says so in its pull request.
7. **Publishes the oracle.** The files the script wrote under
   `/tmp/mechcore/<topic>/<script>/` go to the release `oracle/issue-<n>` with
   `scripts/oracle.py publish <n> <path>...`, and the issue's `Oracle` block
   lists each file with its tick count and physics hash.

A question the keeper's own agents work is not labelled further. A question
offered to an agent outside the keeper's session is labelled `claimable`; see
[Claiming from outside](#claiming-from-outside).

### Working a question

The keeper works a question through agents it starts itself, each in a
worktree of its own. They report to the keeper, not through GitHub: a claim, a
label or a comment is ceremony between sessions that cannot talk, and these
can.

1. **Structure first.** A read-only agent reads the build for the mechanism
   and returns a structure map: the build methods the answer mirrors, where
   the build branches and on what (an override, a type test, a field such as
   `SkillData.isMeleeAttack`), and which simulator functions already mirror
   those methods. The keeper answers the four questions of
   [Acceptance](#acceptance) against the map and writes the decision down
   before any code: what changes, what is reused, what must not be added.
   Most of what a review used to send back is decided here: a second skill
   machine for constructions, a per-unit special case for a targeting
   category, a table for a regularity.
2. **Implement.** One agent per question, in a worktree on
   `research/<n>-<slug>`, given the issue, the oracle
   (`scripts/oracle.py fetch <n>`) and the structure decision. It iterates on
   `mechcore fight verify` until the ticks agree, then lands the answer the
   way `plan.md` says a mechanism lands: the rule and its scope in
   `docs/rules/`, the number's source stated, a refusal in the code for what
   the scope does not cover, and under `tests/<topic>/` the layouts and record
   script the answer used, with the offline `regressions.mcscript` pinning
   each oracle fight, physics and content hash both. `work/research/README.md`'s
   two gates decide what a rule may claim. Every pin that held before holds
   after; a pin that moves is a finding to explain, not a table to edit.
3. **Captures on demand.** An agent that needs a recording does not work
   around its absence, neither by computing a hash with the simulator nor by
   special-casing the one unit that was recorded. It stops and returns a
   capture request: the layout, the record script with an `expect` line per
   hypothesis, and what it separates. The keeper records it, publishes it to
   the question's release, and resumes the same agent with the hashes. **A
   pinned hash always comes from a recording.** Captures run one at a time,
   because there is one game.
4. **Blockers are decided at once.** An agent that runs into a mechanism
   outside the question returns the fixture, the first tick the simulator
   parts from the recording, and what the recording shows there. The keeper
   decides in the same session, and writes the decision into the pull
   request's thread when there is one:
   - **cut the blocker** as a research question of its own, and the blocked
     question waits for it;
   - **record around it**: a fixture that separates the same hypotheses
     without reaching the blocker replaces the blocked one in the release,
     published with `scripts/oracle.py publish` and the blocked recording
     taken out with `scripts/oracle.py retract <n> <path>...`;
   - **accept what holds**: the answer lands without the blocked fixture, and
     the blocker is filed as [a finding](#a-finding).
5. **A second agent reviews.** Before the keeper reads the branch, an agent
   that did not write it reviews it against [Acceptance](#acceptance): the
   four questions, every pinned hash traced to a file in the release, no
   config table where the build's rows are a plain regularity, every earlier
   pin unchanged. The keeper checks each finding and sends the real ones back
   to the implementing agent.
6. **The keeper opens the pull request**, from `research/<n>-<slug>`, its
   title and body the commit master will hold, ending with `Closes #<n>`
   before the `Co-Authored-By` trailers. It then asks the committer, as
   [Acceptance](#acceptance) says.

How many questions run at once is bounded by two things: the captures, which
the one game takes one at a time, and the overlap between answers, which
placing a question decides. Two or three is the usual depth.

### Claiming from outside

An agent outside the keeper's session, a cloud session or another machine,
can still answer a question labelled `claimable`. It claims one by opening a
**draft** pull request from `research/<n>-<slug>`, based on master, whose
body says which agent is working it and ends with `Closes #<n>`; the draft is
the claim, and a second claimant yields to the first. The thread is its
working record, since its checkout dies with its session. It answers as
[Working a question](#working-a-question) steps 2 to 4 say, over GitHub
instead of in a session:

- it asks for a capture by committing the layout and its record script, with
  the `expect` lines, under `tests/<topic>/` on the branch, saying in the
  thread what the fight separates, and adding the label `capture`;
- it reports a blocker by filing [a finding](#a-finding), pinning what holds,
  writing `Blocked by #<m>` in the thread and leaving the draft a draft.

It marks the pull request ready when the issue's `Accept` block holds, and
the keeper takes it from step 5.

### The keeper's loop

The keeper follows one thing at a time, in this order:

1. **A capture request**, from one of its agents or a `capture` label: record,
   publish, answer with the hashes, resume the agent or remove the label.
2. **A blocker without a decision**: decide as step 4 of
   [Working a question](#working-a-question) says.
3. **A branch done**: have it reviewed, read it, and send the committer its
   change sheet.
4. **A merged answer**: check that the issue closed and close it by hand if
   not, and note in `plan.md` what the answer moved.
5. **Room for another question**: cut the next one in `plan.md`'s order, placed
   as [Cutting a question](#cutting-a-question) says.

Nothing polls on a timer. The keeper is woken by its agents finishing and by
the committer; `gh pr list --label capture`, `gh pr list --search "is:open
-is:draft"` and `gh issue list --label research` find what came from outside.

### Acceptance

CI runs every check that can be written down: the test suite, the offline
scripts with every pin, and, for a branch named `research/<n>-…`, the
issue's oracle played back through the branch's simulator, physics and
content, recording by recording. A check that CI does not run is a gap in
CI, not a step for the keeper; an agent or the keeper runs a check locally to
see why it failed, never to stand in for it.

CI green is necessary and not sufficient. The keeper reads the branch, and
reading means:

- reading the rule against the two gates: a mechanism closes on the build's
  code or on the risk-adjusted record, a number traces to one of three
  sources;
- checking that the answer goes through the mechanism the build uses, and
  refuses by name what it did not read rather than approximating it. A new
  kind of object is a question about abstraction, and the new code is read
  for how it relates to the code already there. Four questions decide it:
  1. Which build method does each new function mirror? Two functions that
     mirror one method are a divergence, and a function named for a kind
     beside one that already does the same for units is its usual shape.
  2. Does what differs between two owners reach the shared code only through
     the interface the build asks it through, such as `ISkillOwner` and
     `IAttacker`?
  3. Does every branch on a kind of object in shared code point to the place
     the build branches: an override, or a type test?
  4. Is a new variant of a shared type handled by the shared code, or
     refused by name, rather than handled only on the new kind's path?

  A divergence the build does not have is a step back even when every
  recording agrees. The keeper sends it back, or takes the branch over and
  folds it;
- checking that the pull request's title and body are the commit master
  will hold, and that the documents say what the build does and not what
  the simulator does.

**The committer approves.** A pull request that resolves an issue, by its
`research/` branch or its `Closes` line, merges only with an approving review
on its head commit from a committer: someone with write access to this
repository who is not the pull request's author. The keeper's reading ends
with a **change sheet**, posted on the pull request and handed to the
committer, which says in a few lines what the merge changes:

- behaviour: what the simulator did before and does after, with numbers;
- structure: modules, functions or tables added or removed;
- pins: which hashes were added or moved, and the recording each came from;
- refusals: which were added and which lifted;
- what stays unverified.

The committer approves on GitHub, or tells the keeper in so many words to
approve that one pull request, and the keeper runs `gh pr review <n>
--approve`. An approval covers the pull request it was given for and the
commit it was given on: a push after it needs a new one, and an approval for
one pull request is not one for the next. `automerge.yml` merges once the
approval and every check are in; `review.yml` exists only so that submitting a
review wakes it.

GitHub refuses an approval from a pull request's own author, so an approval
needs the agents to open pull requests under an account that is not the
committer's. While they act through the committer's own account, no review
can approve, and the committer merges a pull request whose change sheet they
accepted by hand.

A question that turns out not to close inside its scope still lands what
holds, with a refusal for the rest, and its issue closes with a comment that
says what remains and what would reopen it. What remains is a new question
when the keeper cuts it, not this one kept open.
