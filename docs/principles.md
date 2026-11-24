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

**Today.** `Declare` is the preparing job: it runs phase 0 and hands the plan to
the jobs after it, except `Publish`, which works out the versions itself. The
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
repository unchecked. The other linters still find and read the repository's
configuration themselves.

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
Rust, TOML and YAML, the builds `cargo-package` and `docker` and the version
writer `cargo` are thin blocks that call the tool. Phase 0, the release stage in
`Publish` and every other step of `nebensound.yml` are one call of the tool
each; a test of the xtask holds every step to that. The Python check runs Python
in three places, each because only the interpreter can answer: whether pytest
imports, whether a file holds a statement, and whether a package of the wheel
imports. The action `tool`, which every job runs, is still shell.

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
curl, Cargo, rustup and Docker come from the runner image, unpinned. The
image build's BuildKit and Trivy stand in the register and nowhere else; the
database Trivy scans with is fetched in every run and pinned nowhere.

## 5. One rule per kind of problem

A case that was not thought of is answered by a rule that removes its kind, not
by a special case on top. A pull request that adds a second exception to the
same rule stops and asks for the rule.

**Why.** Exceptions pile up, and each one has to be known to every later change.
A rule that removes the kind of problem has to be known once.

**Today.** Text from the repository is escaped for the log in some places and
not in others, each place in its own way. The YAML check drops a list of
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

| Job                                   | Token                                          | Runs code of the repository                                                                                                         |
| ------------------------------------- | ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `Declare`                             | read                                           | No. The tool reads the files and the history; when the event does not name the visibility, phase 0 asks for it with the read token. |
| `Version`                             | read                                           | No. The tool reads the history.                                                                                                     |
| `Check repository`, `Check component` | read                                           | Yes: `check-rust`, `check-python` and `cargo xtask check --all`.                                                                    |
| `Check`                               | read                                           | No. It takes the tool from the cache, gathers results and reads the plan.                                                           |
| `Build repository`, `Build component` | read                                           | Yes: the builds compile the code and build images.                                                                                  |
| `Build`                               | read                                           | No. It takes the tool from the cache and gathers results.                                                                           |
| `Publish`                             | `contents: write`, after a push to `main` only | No. It builds the tool from the pipeline's source and runs it, with Git and curl; it starts nothing of the repository.              |

The table holds for a caller, which pins the workflow. In this repository the
gate calls the workflow at its own commit, so on a pull request every job runs
code of the pull request: the tool is built from its source and every step is
its own. Each of those jobs holds only the read token.

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
  hands on, and until the tool is a release binary (#24) the jobs other than
  `Publish` take the tool from the cache.
- **The rule on building blocks** in `AGENTS.md` and the README: a block
  performs one step and is named after its register entry with the category in
  front. It describes the cut above, one named block per step. No issue exists
  yet that replaces it.
- **The action `tool`** holds logic in shell: the fingerprint and the build of
  the tool from source (rule 3). The build from source in a caller: #24. The
  fingerprint: no issue exists yet.
- **Linters read their configuration themselves** (rule 2). Only the link
  check gets its configuration from the tool. The other linters find the
  repository's configuration and read it, and some of their ways load code of
  the repository: a configuration file in JavaScript, a plugin, a module a
  configuration extends. markdownlint and the reader for all of them: #199.
  Prettier: #200. Stylelint: #201. Hadolint, actionlint, the Dockerfile
  formatter and Ruff: #202. html-validate leaves the HTML check with #185.
- **Text from the repository in logs** is escaped place by place, and the file
  lists of the blocks not at all (rule 5): #143.
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
