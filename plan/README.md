# Lanes

One file per lane of [`plan.md`](../plan.md), named after the lane. A lane is
one sub-goal of the plan, and its file is owned by the work in that lane: two
tasks in different lanes never write the same file, and two tasks in the same
lane are not run at once, because nodes in one lane block or conflict with
each other.

A lane file holds intent, never state:

- **the sub-goal and what counts as done**, and why the lane is shaped the way
  it is;
- **the stack**: the node being worked at the top and, under it, the nodes it
  was pushed over, each with what blocks it. A node that is done leaves the
  stack; its record is the pull request that did it;
- **the parking lot**: nodes nobody is working, each with a `reopen_when` that
  can actually be decided;
- the method particular to the lane, if it has one.

Numbers, pass counts and "done up to X" do not go here. They go in the pull
request body, in `tests/<topic>/README.md`, or in an issue, which are where they
can be checked. A new edge to another lane, or a change of the lanes' order, is
a change to `plan.md`, not to a lane file.
