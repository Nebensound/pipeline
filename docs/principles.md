# Principles and trust model

The pipeline sees all code of every repository that calls it. It can change
that code, and it can place something into everything that is shipped. So it
has to be simple, so that it can be maintained and checked, and it has to be
safe. This page states the six rules every change is held against, the reason
for each, and what it protects. [`AGENTS.md`](../AGENTS.md) carries the same
rules in short.

The rules describe where the pipeline goes. Parts of it are older than the
rules; [What does not hold yet](#what-does-not-hold-yet) names each of them and
the issue that changes it, or says that none exists yet. Where another page of
the reference describes such a part, it describes the state of today.

## 1. A simple flow

A preparing job reads the declaration, decides which jobs start and hands on the
matrix for that. Every job runs the tool itself and works out what it needs from
the repository at the commit of the run; no plan is handed from job to job. The
checks run as a matrix of the same call: one job for all checks that
only read the repository's files, and one job for each check that runs code of
the repository. What can be put together is put together. Adding a check does
not add a special path to the workflow.

**Why.** A flow with few shapes can be read in one sitting. Every special path
is one more case to think through in each later change, and one more place
where a change can go wrong in every repository at once.

**Today.** `Declare` is the preparing job: it runs phase 0, reports the versions
and hands the plan to the jobs after it, except `Publish`, which works out the
versions itself. The
checks are cut differently: one job per unit, with one step per check, so
reading checks and checks that run code share a job. Every check adds a step of
its own to `nebensound.yml` and its name to a list there.

## 2. Reading and executing are kept apart

A reading check runs only programs of the pipeline on the repository's files,
with configuration that is data. An executing check runs code of the repository:
build scripts, tests, an image build. The two never share a job. Only "passed"
or "failed" crosses from one job to another.

**Why.** Code of the repository can do anything inside the job it runs in:
change files, replace a program on the `PATH`, write into the log. A reading
check in the same job can therefore no longer vouch for its own result. Kept
apart, the reading checks hold whatever the code of the repository does.

**Today.** The unit job runs `check-rust` and `check-python`, which build and
test the repository's code, in the same job as the reading checks of that
unit. The job of the repository also runs `cargo xtask check --all` there.
The link check of the Markdown and the HTML check hands lychee only the
configuration the tool writes from the keys of `lychee.toml` it knows, and
reads `.lycheeignore` itself: no key can make lychee start a command, end
without checking, send a link to an address it does not check, or read
another file. The keys that choose which addresses are checked (`exclude`,
`include`, `scheme`) and `.lycheeignore` are carried over as the repository's
stated choice of what it leaves out, so they can still leave a link into the
repository unchecked. markdownlint reads no file of the repository but the ones
it lints: the tool reads its configuration from one file at the root, as data,
fails on a key it does not know, on one that names code or another file and on
every file it does not read, and hands markdownlint's library the rules of each
file (see [the tool](tool.md#the-configuration-of-a-linter)). The other linters
still find and read the repository's configuration themselves.

## 3. All logic is Rust, in the tool, and tested

A step of a workflow or of an action is one call of the tool. There is no logic
in shell, in YAML expressions or in embedded scripts, beyond what a program that
is a library needs to be called. Python is used only where nothing else is
possible.

**Why.** Rust in the tool has unit tests, a coverage gate and memory safety, and
it runs the same way on a developer's computer. Shell in a workflow has none of
that: it is proven only by runs, and every caller runs it. The gate still
decides: a run on a developer's computer comes as close to it as is sensible,
says in its first line that its result can differ, and can differ through
settings that come as files, such as a rustup override on the checkout or
Cargo's configuration files, which the tool does not neutralise.

**Today.** The checks for CSS, Dockerfiles, HTML, JSON, Markdown, Python,
Rust, TOML and YAML, the builds `cargo-package`, `cargo-release` and `docker`
and the version writer `cargo` are thin blocks that call the tool. Phase 0, the
release stage in `Publish` and every other step of `nebensound.yml` are one
call of the tool each; a test of the xtask holds every step to that. The Python
check runs Python in three places, each because only the interpreter can
answer: whether pytest imports, whether a file holds a statement, and whether a
package of the wheel imports. The action `tool`, which every job runs, is the
one exception: it gets the tool, so it cannot call it. Its step that fetches
the release's binary holds only what cannot be done without the tool: name the
archive of the runner's system, find the release on the pipeline's commit, ask
whether `gh` can verify, fetch the archive, verify it and unpack it. A test of
the xtask runs that step against cases of its own. The fingerprint and the
build from source, for a commit that carries no release, are shell as well.

## 4. External programs are the exception and are started from the tool

What can sensibly be done in Rust inside the tool is done there. Where an
external program is used, it is a standard tool: one that is very widely used
and maintained. Every external program, action and package stands in one list,
with its exact version and a digest, in a form an automatic updater reads. So it
is always clear what is used and what needs an update or a replacement. For the
language's own toolchain the verification of its installer counts as the
digest: the register pins the coverage toolchain of the Rust check by its exact
name, and rustup checks what it downloads. Which program the tool may start,
and which check or build may start it, is one rule, stated in
[`AGENTS.md`](../AGENTS.md#the-tool) and [the README](../README.md#what-it-is).

**Why.** Each external program is code the pipeline trusts with every
repository. One list makes that trust visible, lets an updater raise it, and
shows at once what to replace when an upstream stops.

**Today.** The programs of the checks stand in the register, each with its
version and the digest of every download, and the tool refuses a download
without that digest. Dependabot does not read the register. Actions are pinned
to a major version. The Dependabot file of the repository covers the actions of
the workflows, not those used inside the actions under `.github/actions/`. Git,
curl, Cargo, rustup and Docker come from the runner image, unpinned, and so
does `gh`, with which the action `tool` verifies the release's binary, at
least in version 2.93.0. The
image build's BuildKit and Trivy stand in the register and nowhere else; the
database Trivy scans with is fetched in every run and pinned nowhere.

## 5. One rule per kind of problem

A case that was not thought of is answered by a rule that removes its kind, not
by a special case on top. A pull request that adds a second exception to the
same rule stops and asks for the rule.

**Why.** Exceptions pile up, and each one has to be known to every later change.
A rule that removes the kind of problem has to be known once.

**Example.** Text from the repository can start a workflow command in a
step's log: a file name or a finding that holds a line break followed by `::`,
that starts with `::`, or that holds `##[` anywhere, and the runner reads more
forms still. Rewriting such text at every place that prints it mirrors the
runner's parser, and every form it misses starts a command. The rule that
removes the kind: only the pipeline's own code writes workflow commands, and
everything that comes from a program or from the repository is printed while
the runner's command processing is stopped, with a random token for each
stretch that no program is given; the tool resumes it only to write its own
commands. Nothing is rewritten for the sake of commands. In a job that runs
code of the repository, that code can do more than print, such as write the
runner's files, so there the stopped stretch is a second line behind rule 2,
not the only one.

**Today.** The YAML check drops a list of
actionlint messages, which grows with each addition GitHub makes to the
workflow syntax.

## 6. What the pipeline protects

Secrets and write permissions reach only what has been merged. No job that runs
code of the repository gets a secret or a token that can write, and no step that
holds a secret or such a token compiles or starts code of the repository.

A check cannot protect its result against the code it checks; the review of the
change does that. Annotations are a reading aid, not proof.

**Why.** A pull request is code nobody has accepted yet. Whatever runs it must
not be able to publish, to tag, or to reach a credential. After the merge the
code has been reviewed, and only then may a job with more rights act on it.

### The trust model today

`nebensound.yml` reads no secret, and the caller file in
[the README](../README.md#calling-the-pipeline) passes none. The workflow grants
its jobs `contents: read`, and one job asks for more:

| Job                                   | Token                                          | Runs code of the repository                                                                                                                                                       |
| ------------------------------------- | ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Declare`                             | read                                           | No. The tool reads the files and the history; when the event does not name the visibility, phase 0 asks for it with the read token.                                               |
| `Check repository`, `Check component` | read                                           | Yes: `check-rust`, `check-python` and `cargo xtask check --all`.                                                                                                                  |
| `Check`                               | read                                           | No. It gets the tool, gathers results and reads the plan.                                                                                                                         |
| `Build repository`, `Build component` | read                                           | Yes: the builds compile the code, start what they built and build images, a build with targets in one job per target.                                                             |
| `Build`                               | read                                           | No. It gets the tool and gathers results.                                                                                                                                         |
| `Publish`                             | `contents: write`, after a push to `main` only | No. It runs the tool, fetched or built without the cache, with Git and curl, and gh, which only reads, to verify a fetched tool; it uploads the builds' archives and starts none. |

The table holds for a caller, which pins the workflow. In this repository the
gate calls the workflow at its own commit, so on a pull request every job runs
code of the pull request: the tool is built from its source and every step is
its own. Each of those jobs holds only the read token.

Every job of a caller gets the tool the same way: where the pinned commit
carries a release of the pipeline, the release's binary for the runner's
system, verified with `gh release verify-asset` against the attestation GitHub
signs for the immutable release, and never taken from a cache. That proves that
the file is an asset of exactly that release and that its tag stood on that
commit; it does not prove what the binary was built from, which rests on the
pipeline's own Build and Publish. gh gets the job's token for it, in `Publish`
the one that can write, and only reads with it. Where no release carries a
binary for the job, the tool is built from the pipeline's source as in this
repository, with a warning that names why, and a binary that does not verify
fails the job.

So on `main` today, no job that runs code of the repository holds a token that
can write, and the one job that can write runs only after a push to `main`, when
the change has been merged and every check has passed. Every checkout of the
workflow leaves no credentials behind (`persist-credentials: false`). The caller
grants more permissions than any job asks for, so that later parts need no
change in every repository (see [Calling the workflow](calling.md#permissions));
no job uses them today.

What is a goal and not yet true:

- Reading and executing checks share a job (rule 2), so a result of a reading
  check in a unit with Rust or Python code is only as good as that code allows.

## What does not hold yet

What differs today, and the open issue that changes it. Where no issue exists
yet, it says so.

- **The cut of the jobs** (rules 1 and 2). The checks run in one job per unit,
  reading and executing together, and each check adds a step to
  `nebensound.yml` and its name to the list in `Not written yet`. The
  [phases](phases.md#one-job-per-unit) and the README describe this cut. No
  issue exists yet.
- **Every job works out what it starts from** (rule 1). Only `Publish` does so
  today; the other jobs follow with the cut of the jobs. Until then the
  matrices, the version writers, the Rust check and the steps of `Check` that
  look at the version writers and publish targets take the plan `Declare`
  hands on.
- **The rule on building blocks** in `AGENTS.md` and the README: a block
  performs one step and is named after its register entry with the category in
  front. It describes the cut above, one named block per step. No issue exists
  yet that replaces it.
- **The action `tool`** holds logic in shell (rule 3): the step that fetches
  the release's binary, which cannot be anything else, and the fingerprint and
  the build of the tool from source, for a commit that carries no release. No
  issue exists yet for the fingerprint and the build.
- **Linters read their configuration themselves** (rule 2). markdownlint,
  Prettier and the link check get their configuration from the tool. The other
  linters find the repository's configuration and read it, and some of their
  ways load code of the repository: a configuration file in JavaScript, a
  plugin, a module a configuration extends. Stylelint: #201. Hadolint, actionlint,
  the Dockerfile formatter and Ruff: #202. html-validate leaves the HTML check
  with #185. Taplo's formatter finds the
  repository's `taplo.toml` at the root itself; it becomes a library linked
  into the tool with #184. Pyright gets the configuration the tool wrote, but
  that keeps an `extends`, which Pyright follows to another file; no issue
  exists yet.
- **The source of actionlint**, whose upstream stands still, and the growing
  list of its messages the YAML check drops (rules 4 and 5): #169.
- **The programs the tool may start.** The rule in
  [`AGENTS.md`](../AGENTS.md#the-tool) and the README names each program and
  who may start it, and uv, which provides the Python of the Python check, is
  pinned in the register. Git, curl, Cargo, rustup and Docker, and further
  programs the workflow and the blocks call, come from the runner image without
  a pinned version or digest (rule 4). No issue exists yet.
- **The database the image build's scan uses** (rule 4). Trivy fetches its
  database of vulnerabilities fresh in every run, verified only by the digests
  the registry serves for its tag at that moment; it is pinned nowhere. Whether
  and how to pin it is open. No issue exists yet.
- **One list in a form an updater reads** (rule 4). The register pins programs
  with digests, but Dependabot does not read it, and `AGENTS.md` allows actions
  pinned to a major version, without a digest. The Dependabot file does not
  cover the actions used inside the actions under `.github/actions/`. No issue
  exists yet.
