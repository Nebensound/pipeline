// The page rules of check-html: what every page must carry, as rules of
// html-validate, so that they share its parser and name the line and column
// of what they find.
//
// A page is a file whose document begins with `<!doctype html>` or has an
// `<html>` root. Any other HTML file, such as a fragment or a header that a
// generator includes, is no page, and these rules find nothing in it.
//
// The block forces them on after the repository's configuration is laid over
// the default, and an inline `html-validate-disable` comment does not reach
// them: only an `[[exclude]]` entry of the declaration takes a page out.
//
// The module exports a function that takes html-validate's `Rule` class, so
// that it imports nothing itself and runs with the html-validate the block
// installs.

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
