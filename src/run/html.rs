//! The HTML check: every `.html` and `.htm` file is formatted the way Prettier
//! writes it, passes html-validate and the page rules, and its links into the
//! repository lead somewhere.

use std::fs;
use std::path::{Path, PathBuf};

use super::links::{self, Handed};
use super::{Entry, Session, Tool};
use crate::rules;

/// What runs html-validate on Node.js, through its own `HtmlValidate`.
///
/// The pipeline's default (`rules/html-validate.json`, compiled into this
/// tool: the presets `recommended`, `a11y` and `prettier`, the last switching
/// off what contradicts Prettier) lies beneath the repository's own
/// configuration, which is laid over it rule by rule: a rule the repository
/// does not name keeps the default's value, so its file names only what
/// differs, and a change of the default reaches it.
///
/// The repository's configuration is what html-validate reads for a file: the
/// `.htmlvalidate.json` (or `.mjs`, `.cjs`, `.js`) of its folder and of every
/// folder above it, the nearer one over the farther, up to one that says
/// `root: true` and no farther than the repository root. A flat configuration
/// (`html-validate.config.*`), which is JavaScript in a form of its own, is
/// not read and fails. Its `.htmlvalidateignore` files are not read either:
/// every file of the list is checked.
///
/// Over both lie the page rules ([`PAGE_RULES`]): every page has a title, a
/// description, exactly one `h1`, and alternative text on every image. They
/// are switched on last, so the repository's configuration cannot switch them
/// off, and an inline `html-validate-disable` comment does not reach them:
/// only an `[[exclude]]` entry takes a page out. A package named in a
/// configuration is resolved from where the pinned html-validate lies, so only
/// the pinned packages are found.
///
/// It prints no workflow command. Every finding is handed to the tool as data,
/// one JSON object per line of the file `FINDINGS` (see
/// [`FINDINGS`](super::FINDINGS)), and the tool writes it, on its line: a
/// warning fails nothing, an error fails the check.
///
/// It reads where the pinned package lies (`TOOLS`), the list of files, each
/// ended by a zero byte (`LIST`), the default (`DEFAULT`), the page rules
/// (`RULES`, a module it imports) and where its findings go (`FINDINGS`) from
/// its environment.
pub const HTML_VALIDATE: &str = r#"
import fs from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const { TOOLS, LIST, DEFAULT, RULES, FINDINGS } = process.env;
const root = process.cwd();
const message = (error) => (error instanceof Error ? error.message : String(error));
// html-validate leaves a rejection unhandled beside the error it
// throws when a configuration names a module that is not there.
// The error is reported with the file; anything left unhandled
// fails the check as well.
process.on("unhandledRejection", (error) => {
  console.error(`html-validate: ${message(error)}`);
  process.exitCode = 1;
});
const { ConfigLoader, FileSystemConfigLoader, HtmlValidate, Rule, esmResolver } =
  await import(pathToFileURL(
    join(TOOLS, "node_modules", "html-validate", "dist", "esm", "index.js"),
  ).href);
const { default: pageRules, RULES: PAGE_RULES } = await import(pathToFileURL(RULES).href);

// The configuration files html-validate reads for a file. Only
// those inside the repository count: what lies above its root is
// no part of it.
const inside = (file) => {
  const path = relative(root, resolve(file));
  return path !== ".." && !path.startsWith(`..${sep}`);
};
const view = { ...fs, existsSync: (file) => inside(file) && fs.existsSync(file) };
const resolvers = [esmResolver()];
const files = new FileSystemConfigLoader(resolvers, {}, { fs: view });
const base = await files.loadFromObject(JSON.parse(fs.readFileSync(DEFAULT, "utf8")));
const forced = await files.loadFromObject({ plugins: [pageRules(Rule)], rules: PAGE_RULES });

// Hands html-validate the configuration of one file, resolved.
class Resolved extends ConfigLoader {
  constructor(config) {
    super(resolvers);
    this.resolved = config;
  }
  getConfigFor() {
    return this.resolved;
  }
  flushCache() {}
  defaultConfig() {
    return this.resolved;
  }
}

// A finding goes to the tool as data, which writes it.
const found = (finding) => fs.appendFileSync(FINDINGS, `${JSON.stringify(finding)}\n`);

for (const name of [
  "html-validate.config.mjs", "html-validate.config.cjs", "html-validate.config.js",
  "html-validate.config.mts", "html-validate.config.cts", "html-validate.config.ts",
]) {
  if (fs.existsSync(name)) {
    found({ level: "error", file: name, message: "html-validate would read this flat configuration in place of every .htmlvalidate file, and check-html does not read it; write its rules into .htmlvalidate.json instead" });
    process.exitCode = 1;
  }
}
const list = fs.readFileSync(LIST, "utf8").split("\0").filter((file) => file !== "");
for (const file of list) {
  let report;
  try {
    const own = await files.fromFilenameAsync(resolve(file));
    const layered = own ? await base.merge(resolvers, own) : base;
    const config = await layered.merge(resolvers, forced);
    report = await new HtmlValidate(new Resolved(config.resolve())).validateFile(file);
  } catch (error) {
    found({ level: "error", file, message: `html-validate cannot check it: ${message(error)}` });
    process.exitCode = 1;
    continue;
  }
  // Every finding is one on its line, named by its rule. A warning
  // fails nothing, an error fails the check.
  for (const result of report.results) {
    for (const each of result.messages) {
      const level = each.severity >= 2 ? "error" : "warning";
      const text = `${each.ruleId}: ${each.message}`;
      found({ level, file, line: each.line, column: each.column, message: text });
      if (level === "error") {
        process.exitCode = 1;
      }
    }
  }
}
"#;

/// The page rules, a module of html-validate rules in JavaScript, which
/// [`HTML_VALIDATE`] imports from the folder of what the programs are handed.
pub const PAGE_RULES: &str = r#"// The page rules of the HTML check: what every page must carry, as rules of
// html-validate, so that they share its parser and name the line and column
// of what they find.
//
// A page is a file whose document begins with `<!doctype html>` or has an
// `<html>` root. Any other HTML file, such as a fragment or a header that a
// generator includes, is no page, and these rules find nothing in it.
//
// `nebensound-repo run html` forces them on after the repository's
// configuration is laid over the default, and an inline
// `html-validate-disable` comment does not reach them: only an `[[exclude]]`
// entry of the declaration takes a page out.
//
// The module exports a function that takes html-validate's `Rule` class, so
// that it imports nothing itself and runs with the pinned html-validate.

/** The name the rules go by, as `page/<rule>`. */
export const NAME = "page";

/**
 * The plugin, built on html-validate's `Rule`.
 *
 * @param {typeof import("html-validate").Rule} Rule
 */
export default function pageRules(Rule) {
  /** A page rule: always on, and deaf to the disable comments. */
  class PageRule extends Rule {
    isEnabled() {
      return true;
    }

    isBlocked() {
      return false;
    }

    /**
     * Runs `check` on every page once its tree is complete.
     *
     * @param {(document: import("html-validate").DOMTree) => void} check
     */
    onPage(check) {
      let doctype = false;
      this.on("doctype", () => {
        doctype = true;
      });
      this.on("dom:ready", (event) => {
        const { document } = event;
        const rooted = document.root.childElements.some(
          (element) => element.tagName === "html",
        );
        if (doctype || rooted) {
          check(document);
        }
      });
    }

    /**
     * Where a finding about the whole page stands: at its `<head>`, else at
     * its `<html>`, else at its first element.
     *
     * @param {import("html-validate").DOMTree} document
     */
    pageNode(document) {
      return (
        document.querySelector("head") ??
        document.querySelector("html") ??
        document.root.childElements[0] ??
        document.root
      );
    }
  }

  class Title extends PageRule {
    documentation() {
      return {
        description:
          "Every page has a title: a <title> element with text in it.",
      };
    }

    setup() {
      this.onPage((document) => {
        // html-validate names the <title> of an inline <svg> `svg:title`, so
        // only a title of the document matches here (case svg-title).
        const titled = document
          .querySelectorAll("title")
          .some((title) => title.textContent.trim() !== "");
        if (!titled) {
          this.report({
            node: this.pageNode(document),
            message:
              "the page has no title: give it a <title> element with text in it",
          });
        }
      });
    }
  }

  class Description extends PageRule {
    documentation() {
      return {
        description:
          'Every page has a description: <meta name="description"> with a content that is not empty.',
      };
    }

    setup() {
      this.onPage((document) => {
        const described = document
          .querySelectorAll("meta")
          .some(
            (meta) =>
              (meta.getAttributeValue("name") ?? "").toLowerCase() ===
                "description" &&
              (meta.getAttributeValue("content") ?? "").trim() !== "",
          );
        if (!described) {
          this.report({
            node: this.pageNode(document),
            message:
              'the page has no description: give it <meta name="description" content="..."> with text in the content',
          });
        }
      });
    }
  }

  class OneH1 extends PageRule {
    documentation() {
      return {
        description: "Every page has exactly one <h1>.",
      };
    }

    setup() {
      this.onPage((document) => {
        const headings = document.querySelectorAll("h1");
        if (headings.length === 0) {
          this.report({
            node: document.querySelector("body") ?? this.pageNode(document),
            message: "the page has no <h1>, and a page has exactly one",
          });
          return;
        }
        for (const heading of headings.slice(1)) {
          this.report({
            node: heading,
            message: `the page has ${String(headings.length)} <h1> elements, and a page has exactly one; this is one too many`,
          });
        }
      });
    }
  }

  class ImageAlt extends PageRule {
    documentation() {
      return {
        description:
          'Every image of a page has alternative text: an alt attribute, which may be empty (alt="") for an image that only decorates.',
      };
    }

    setup() {
      this.onPage((document) => {
        for (const image of document.querySelectorAll("img")) {
          if (!image.hasAttribute("alt")) {
            const source = image.getAttributeValue("src");
            const named =
              typeof source === "string" && source !== ""
                ? `the image ${source}`
                : "this image";
            this.report({
              node: image,
              message: `${named} has no alternative text: give it an alt attribute, alt="" where it only decorates`,
            });
          }
        }
      });
    }
  }

  return {
    name: NAME,
    rules: {
      [`${NAME}/title`]: Title,
      [`${NAME}/description`]: Description,
      [`${NAME}/one-h1`]: OneH1,
      [`${NAME}/image-alt`]: ImageAlt,
    },
  };
}

/** The page rules, each switched on as an error. */
export const RULES = Object.fromEntries(
  ["title", "description", "one-h1", "image-alt"].map((rule) => [
    `${NAME}/${rule}`,
    "error",
  ]),
);
"#;

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    // A missing program is named and fails the check, and the parts whose
    // programs were found still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let validate = session.tool("html-validate", true);
    let lychee = session.tool("lychee", false);
    let handed = links::hand_over(session);
    let mut list = Vec::new();
    for file in &session.files {
        list.extend_from_slice(file.as_bytes());
        list.push(0);
    }
    let mut root = PathBuf::new();
    let Some(temp) = session.folder(|path| {
        root = rooted(path);
        let mut entries: Vec<Entry> = vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            (super::FINDINGS.to_owned(), Some(Vec::new())),
            ("files".to_owned(), Some(list)),
            ("root".to_owned(), None),
            (
                "lychee-rooted.toml".to_owned(),
                Some(excluded(&root).into_bytes()),
            ),
            (
                "html-validate.json".to_owned(),
                Some(rules::HTML_VALIDATE.as_bytes().to_vec()),
            ),
            (
                "page-rules.mjs".to_owned(),
                Some(PAGE_RULES.as_bytes().to_vec()),
            ),
        ];
        entries.extend(Handed::entries(handed.as_ref()));
        entries
    }) else {
        return false;
    };
    let files = session.files.clone();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when one before it failed, so that one run reports
    // every finding at once.
    let formatted = prettier
        .is_some_and(|prettier| session.prettier(&prettier, &temp.join("nothing-ignored"), &files));
    let validated = validate.is_some_and(|validate| validate_run(session, &validate, &temp));
    let site = temp.join("lychee-rooted.toml");
    let links_lead = handed.is_some_and(|handed| {
        lychee.is_some_and(|lychee| links::check(session, &lychee, &handed, &temp, &[&site], &root))
    });
    formatted && validated && links_lead
}

/// The folder a link from the root of a site, such as `/style.css` or
/// `/login`, is resolved below: an empty one in the folder of what the
/// programs are handed, `folder`. Such a link is not checked, since where the
/// site is served and which routes its server answers the declaration does
/// not say. The path is taken without links, as lychee writes the addresses
/// it resolves below it; on macOS the temporary folder lies behind one.
#[must_use]
pub fn rooted(folder: &Path) -> PathBuf {
    fs::canonicalize(folder)
        .unwrap_or(folder.to_path_buf())
        .join("root")
}

/// The second configuration of the link check, behind the repository's: it
/// leaves out every address below `root` (see [`rooted`]), through a pattern
/// that takes every character of the folder's path literally.
#[must_use]
pub fn excluded(root: &Path) -> String {
    let pattern = format!("^file://{}/", regex::escape(&root.to_string_lossy()));
    let mut table = toml::Table::new();
    table.insert(
        "exclude".to_owned(),
        toml::Value::Array(vec![toml::Value::String(pattern)]),
    );
    table.to_string()
}

/// html-validate over the files, with the repository's configuration laid
/// over the pipeline's default and the page rules over both: see
/// [`HTML_VALIDATE`]. What it prints goes into the output as it is, and the
/// findings it hands over after it.
fn validate_run(session: &mut Session<'_, '_, '_>, validate: &Tool, temp: &Path) -> bool {
    // The pinned copy, which `Session::tool` takes only with its runtime
    // when it is installed with npm, as the register has it.
    let Some(node) = &validate.runtime else {
        session.log.error(
            None,
            None,
            "html-validate runs on Node.js, and the register gives it no runtime to run on",
        );
        return false;
    };
    session.log.line(
        "Rules: the pipeline default, with the repository's configuration and the page rules \
         laid over it",
    );
    let mut command = session.command(node, &validate.search_path);
    command
        .args(["--input-type=module", "--eval", HTML_VALIDATE])
        .env("TOOLS", &validate.folder)
        .env("LIST", temp.join("files"))
        .env("DEFAULT", temp.join("html-validate.json"))
        .env("RULES", temp.join("page-rules.mjs"))
        .env("FINDINGS", temp.join(super::FINDINGS));
    let ran = session.capture(command, "html-validate");
    session.log.lines(&ran.text);
    session.handed(&temp.join(super::FINDINGS), "html-validate") && ran.success
}
