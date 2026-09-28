# Issues

An issue here is a **finding**: something you found while doing something
else, which disagrees with something you believed, and which you are
deliberately not fixing now. It is a holding pen, not a backlog: no owner, no
priority, no labels, no status beyond open. The only thing that happens to it
is that it leaves.

## Admission

An observation has to pass all four:

- **It was found, not sought.** Went looking? That is research, in
  `work/research/`.
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

An issue leaves in one of six ways, and the closing comment says which and
where its content went:

1. **to the plan**, as a node under `plan/`, when someone decides to act;
2. **to a spec's `Unresolved`**, when it is a design choice nobody made;
3. **to `docs/rules/`**, when the game works that way and the belief was ours;
4. **to `work/research/`**, when it is worth pursuing as a question;
5. **into an assertion**, when a fix lands and a test holds it, closed by the
   pull request's `Closes #n`;
6. **closed as not planned**, when the observation was wrong or what it
   disagreed with is gone.

The first five close as completed; nothing else closes an issue, and none ages
out. Copy the evidence to where it lands rather than linking back. A rules
document or spec never cites an issue: an issue is not understood yet, and one
that matters enough to cite has already earned its exit.
