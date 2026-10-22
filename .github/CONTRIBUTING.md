# Issues

An issue records a finding, deferred work, or behavior reached by a
reproducible input. It has no owner, priority or status beyond open.

| Kind | Label | What it records |
| --- | --- | --- |
| Finding | none | Something found while doing other work that contradicts an expectation and is deliberately not fixed now |
| Plan | `plan` | Deferred work with a scope and a completion criterion |
| Unimplemented | `unimplemented` | An actual layout reaches a simulator branch that refuses an unimplemented mechanism or unsupported shape |
| Divergence | `divergence` | The simulator fights differently from the game, or newly refuses a corpus round |
| Match | `match` | A complete recorded match fails simulator verification; all its failures belong to one issue |

A corpus measurement can establish a `divergence` when a commit fights a
round wrong or newly refuses it. Findings of the same kind carry it too. A
refusal discovered by running a layout through an unimplemented branch carries
`unimplemented`.

## Match reports

Tournament CI lives in qbisi/mechcore-replay and checks newly added tournament
matches with a release build of this repository's master. It reports failures
here with the `match` label. One BattleID has one issue: later failing rounds
update that issue rather than producing new round issues. Reports pin both
repository commits and the source match, retain the full verification output
as workflow artifacts, and state the first failure as a starting point.

Resolution requires the entire `mechcore verify --backend simulator match.yaml`
command to exit 0 with a `kind: match`, `valid: true` report. A repaired first
round is not a resolution if a later round still fails. Re-run Tournament
matches in mechcore-replay with the issue's complete source path as the
`match` input; it checks current master and closes the issue only after the
whole match passes. A later failing recheck reopens the same issue. Explicit
warnings keep the CLI's documented verification semantics. A simulator crash,
timeout, or incomplete report cannot resolve an issue.

These measured match failures do not need the Finding template's admission
tests. A match issue is the unit of work; separate mechanism research can
still produce a pinned fight fixture.

## Plans and unimplemented branches

The Plan template states the work's goal, scope and completion criterion.
It does not require a discrepancy or a game recording. Close it when the
stated work lands, or as not planned when the work is no longer intended.

The Unimplemented template needs only the triggered error and a reproducing
layout. Include the commit, command, exact diagnostic and source locations
when available. Embed the complete layout, or link to one tracked at a pinned
commit: a scratch path or a generation seed alone cannot reproduce it after
the code changes. A hypothetical refusal found by reading code does not
qualify. Group layouts reaching the same refusal branch in one issue.

An unimplemented issue closes with the fix's `Closes #n`, or as not planned
when investigation establishes that the refusal is correct. A plan or an
unimplemented report does not need the Finding template's admission tests.

## Admission

A finding has to pass all four:

- **It was found, not sought.** Went looking? That is research, and it
  lands as a pull request.
- **It is a discrepancy, not a preference.** It names the document, assertion
  or commit it disagrees with, or says the belief was never written down. An
  issue whose expectation is unstated cannot be closed.
- **Fixing it now would derail you.** A two-line fix in a file already open is
  a fix.
- **Re-finding it would cost real work.** A number a command prints on demand
  needs no issue.

The Finding template gives the form. The evidence goes in the body, the
numbers, the command and a fixture anyone can run, since a scratch directory
dies and a reader on the web has no checkout.

## Exits

A finding leaves in one of five ways, and the closing comment says which and
where its content went:

1. **to a spec's `Unresolved`**, when it is a design choice nobody made;
2. **to `docs/rules/`**, when the game works that way and the belief was ours;
3. **to research**, when it is worth pursuing as a question;
4. **into an assertion**, when a fix lands and a test holds it, closed by the
   pull request's `Closes #n`;
5. **closed as not planned**, when the observation was wrong or what it
   disagreed with is gone.

The first four close as completed; nothing else closes an issue, and none ages
out. Copy the evidence to where it lands rather than linking back. A rules
document or spec never cites an issue: an issue is not understood yet, and one
that matters enough to cite has already earned its exit.
