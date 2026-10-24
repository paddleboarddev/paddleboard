# Governance

This document describes how PaddleBoard is run today. It is deliberately
short: the project has one maintainer, and the rules fit that. They are
written down so that contributors know how decisions get made and how the
project will grow, not to create process for its own sake.

## Current model

PaddleBoard is founder-led. [Jason Smith](https://github.com/jasonsmithio)
(@jasonsmithio) is the maintainer and has the final say on the project's
direction, scope and releases. This is sometimes called the "benevolent
dictator" model. It is the right model for a project at this size and will be
replaced as the community grows (see [Growing the model](#growing-the-model)).

## Roles

**Users** run PaddleBoard. Filing bugs, asking questions in
[Discussions](https://github.com/paddleboarddev/paddleboard/discussions) and
reporting what works and what does not is a contribution in its own right.

**Contributors** have had a change merged: code, documentation, issue
templates, CI. There is no application; opening a pull request that lands
makes you a contributor. [CONTRIBUTING.md](./CONTRIBUTING.md) covers how.

**Maintainers** have write access, review and merge pull requests, triage
issues, cut releases and are accountable for the project's direction.
Maintainers are listed in [CODEOWNERS](./CODEOWNERS).

### Becoming a maintainer

A contributor becomes a maintainer through sustained, quality contributions
over time, followed by a nomination from an existing maintainer. "Sustained"
means a track record across several months rather than one large change;
"quality" means changes that respect the fork's conventions (small diffs to
shared files, `// PaddleBoard:` tags, new work in `paddleboard_*` crates) and
that the contributor can explain and support. Reviewing other people's pull
requests and helping in Discussions counts as much as writing code.

Nominations are discussed among the existing maintainers and accepted by lazy
consensus. New maintainers are added to CODEOWNERS in a pull request so the
change is visible.

Maintainers who step back, or who are inactive for six months, are moved to
emeritus status with thanks. Emeritus maintainers can return by asking.

## How decisions are made

Most decisions happen in pull requests and issues through **lazy consensus**:
a proposal is accepted if nobody objects within a reasonable window, normally
72 hours, after it has been clearly stated. Silence is assent. An objection
should say what is wrong and, where possible, what would resolve it.

When consensus is not reached, the maintainer decides. The reasoning is
written down in the issue or pull request so the decision can be understood,
and revisited, later.

Some decisions are the maintainer's alone for now: release timing, licensing,
the project's name and branding, and anything involving the relationship with
upstream [Zed](https://github.com/zed-industries/zed) (which upstream changes
to take, and what is deliberately kept inert, for example telemetry and
upsell code).

### The upstream relationship

PaddleBoard merges from Zed weekly. That shapes what the project can decide:
changes to the inherited editor surface belong upstream first, and
PaddleBoard inherits them. Decisions here are about what the fork adds, what
it deliberately removes, and how it stays mergeable. See
[CONTRIBUTING.md](./CONTRIBUTING.md) for what is likely to land.

## Growing the model

As the contributor base grows, this model will move to a multi-maintainer
model: a small group of maintainers sharing review and release duties, with
decisions among them by lazy consensus and a documented way to resolve
disagreement. The trigger is practical rather than numeric: when review or
release work regularly waits on one person, it is time. Changing this
document is itself a decision made under the rules above, in a pull request.

## Code of conduct

Everyone participating in the project is expected to follow the
[Code of Conduct](./CODE_OF_CONDUCT.md). Maintainers are responsible for
enforcing it.

## Security

Security issues are reported privately; see [SECURITY.md](./SECURITY.md).
