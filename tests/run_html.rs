//! The page rules of the HTML check (`nebensound-repo run html`): what every
//! page must carry, read off the document html5ever parses, each finding at
//! the line and column of the element it is about.
//!
//! The check as a whole, with Prettier and lychee, is in `run.rs`; the cases
//! of `check-html` show the rules on real files through the tool.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use html5ever::tendril::StrTendril;
use html5ever::tree_builder::{NodeOrText, TreeSink};
use nebensound_repo::run::Finding;
use nebensound_repo::run::html::page::{self, Tree};

/// What the page rules find in a text, each as `line:column: message`, or
/// `-: message` where it has no place.
fn found(text: &str) -> Vec<String> {
    shown(&page::findings(text.as_bytes()))
}

fn shown(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| match finding.position {
            Some((line, column)) => format!("{line}:{column}: {}", finding.message),
            None => format!("-: {}", finding.message),
        })
        .collect()
}

const TITLE: &str = "page/title: the page has no title: give it a <title> element with text in it";
const DESCRIPTION: &str = "page/description: the page has no description: give it <meta \
                           name=\"description\" content=\"...\"> with text in the content";
const NO_H1: &str = "page/one-h1: the page has no <h1>, and a page has exactly one";

/// A page with everything a page must carry, with `head` and `body` added to
/// its `<head>` and its `<body>`.
fn page(head: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <title>A page</title>\n    <meta \
         name=\"description\" content=\"A page.\" />\n{head}  </head>\n  <body>\n    \
         <h1>A page</h1>\n{body}  </body>\n</html>\n"
    )
}

#[test]
fn a_page_with_everything_it_must_carry_passes() {
    assert_eq!(found(&page("", "")), Vec::<String>::new());
    // A title anywhere in the page counts, and so does what a <template>
    // holds; the name of the description is read in any case.
    let text = "<!DOCTYPE html>\n<body>\n<h1>x</h1>\n<title> A title </title>\n\
                <template><meta name=\"Description\" content=\"d\"></template>\n";
    assert_eq!(found(text), Vec::<String>::new());
}

#[test]
fn a_file_that_is_no_page_is_not_judged() {
    for text in [
        "<header>\n  <p>A part of a page.</p>\n</header>\n",
        "",
        "plain text",
        "<div><html><img src=a></html></div>",
    ] {
        assert_eq!(found(text), Vec::<String>::new(), "{text}");
    }
}

#[test]
fn a_page_without_a_title_description_or_h1_is_found_at_its_head_and_body() {
    let text = "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" \
                />\n  </head>\n  <body>\n    <p>x</p>\n  </body>\n</html>\n";
    assert_eq!(
        found(text),
        [
            format!("3:4: {TITLE}"),
            format!("3:4: {DESCRIPTION}"),
            format!("6:4: {NO_H1}"),
        ]
    );
}

#[test]
fn a_finding_about_the_page_falls_back_to_the_html_the_first_element_and_the_first_line() {
    // No <head> written: the <html>, and without a <body> the <h1> as well.
    let text = "\n<html>\n<p>x</p>\n";
    assert_eq!(
        found(text),
        [
            format!("2:2: {TITLE}"),
            format!("2:2: {DESCRIPTION}"),
            format!("2:2: {NO_H1}"),
        ]
    );
    // Neither written: the first element written in the file.
    let text = "<!doctype html>\n<!-- a comment -->\n  <p>x</p><div>y</div>\n";
    assert_eq!(
        found(text),
        [
            format!("3:4: {TITLE}"),
            format!("3:4: {DESCRIPTION}"),
            format!("3:4: {NO_H1}"),
        ]
    );
    // No element written at all: the first line.
    assert_eq!(
        found("<!doctype html>\n"),
        [
            format!("1:1: {TITLE}"),
            format!("1:1: {DESCRIPTION}"),
            format!("1:1: {NO_H1}"),
        ]
    );
}

#[test]
fn a_title_of_an_svg_or_without_text_is_no_title_of_the_page() {
    let svg = "    <svg role=\"img\"><title>A dot</title></svg>\n";
    let text = page("", svg).replace("    <title>A page</title>\n", "");
    assert_eq!(found(&text), [format!("3:4: {TITLE}")]);
    let text = page("", "").replace("<title>A page</title>", "<title> \n\t</title>");
    assert_eq!(found(&text), [format!("3:4: {TITLE}")]);
}

#[test]
fn a_description_needs_its_name_and_a_content_with_text() {
    for meta in [
        "<meta name=\"description\" content=\"  \" />",
        "<meta name=\"description\" />",
        "<meta content=\"A page.\" />",
        "<meta name=\"keywords\" content=\"A page.\" />",
    ] {
        let text = page("", "").replace("<meta name=\"description\" content=\"A page.\" />", meta);
        assert_eq!(found(&text), [format!("3:4: {DESCRIPTION}")], "{meta}");
    }
}

#[test]
fn every_h1_after_the_first_is_one_too_many() {
    let text = page(
        "",
        "    <h1>Two</h1>\n    <section><h1>Three</h1></section>\n",
    );
    let many = "the page has 3 <h1> elements, and a page has exactly one; this is one too many";
    assert_eq!(
        found(&text),
        [
            format!("9:6: page/one-h1: {many}"),
            format!("10:15: page/one-h1: {many}"),
        ]
    );
}

#[test]
fn every_image_without_alternative_text_is_named_by_its_source() {
    let body = "    <img src=\"dot.svg\" />\n    <img src=\"\" />\n    <img />\n    \
                <img src=\"x.png\" alt=\"\" />\n    <noscript><img src=\"n.png\"></noscript>\n    \
                <template><img src=\"t.png\"></template>\n";
    let alt = "has no alternative text: give it an alt attribute, alt=\"\" where it only decorates";
    assert_eq!(
        found(&page("", body)),
        [
            format!("9:6: page/image-alt: the image dot.svg {alt}"),
            format!("10:6: page/image-alt: this image {alt}"),
            format!("11:6: page/image-alt: this image {alt}"),
            format!("13:16: page/image-alt: the image n.png {alt}"),
            format!("14:16: page/image-alt: the image t.png {alt}"),
        ]
    );
}

#[test]
fn a_finding_stands_where_the_start_tag_names_its_element() {
    let alt = "has no alternative text: give it an alt attribute, alt=\"\" where it only decorates";
    // A start tag over several lines, as Prettier writes a long one: the line
    // of its name.
    let body = "    <img\n      class=\"wide\"\n      src=\"a.png\"\n    />\n";
    assert_eq!(
        found(&page("", body)),
        [format!("9:6: page/image-alt: the image a.png {alt}")]
    );
    // A name in capitals, a `<` in a value of an attribute, and the name
    // again in a value of the same tag.
    let body = "<IMG SRC=\"b.png\"><img title=\"a < b\" src=\"c.png\"><img title=\"<img \" \
                src=\"d.png\">\n";
    assert_eq!(
        found(&page("", body)),
        [
            format!("9:2: page/image-alt: the image b.png {alt}"),
            format!("9:19: page/image-alt: the image c.png {alt}"),
            format!("9:50: page/image-alt: the image d.png {alt}"),
        ]
    );
    // Lines that end in a carriage return, alone or before a line feed, and
    // a column counted in characters.
    let text = "<!doctype html>\r\n<title>t</title>\r<meta name=description content=d>\r\n\
                <h1>ü</h1>\n<p>äö<img src=e.png></p>";
    assert_eq!(
        found(text),
        [format!("5:7: page/image-alt: the image e.png {alt}")]
    );
}

#[test]
fn a_byte_order_mark_and_bytes_that_are_not_utf8_are_read_past() {
    let mut bytes = "\u{feff}<!doctype html><title>\u{0}".as_bytes().to_vec();
    bytes.extend([0xff, b'<', b'/', b't', b'i', b't', b'l', b'e', b'>']);
    bytes.extend(b"<img src=x>");
    assert_eq!(
        shown(&page::findings(&bytes)),
        [
            format!("1:17: {DESCRIPTION}"),
            format!("1:17: {NO_H1}"),
            "1:34: page/image-alt: the image x has no alternative text: give it an alt \
             attribute, alt=\"\" where it only decorates"
                .to_owned(),
        ]
    );
}

#[test]
fn what_the_standard_builds_without_a_tag_has_no_place() {
    // A <tbody> and a <tr> the parser adds around a cell, text a table moves
    // in front of itself, an element it moves out of a misnested one, a
    // second <html> and <body> whose attributes join the first, a <frameset>
    // that takes the place of the <body>, a script, and foreign content.
    let text = "<!doctype html>\n<html lang=en><html lang=de dir=ltr><body><body class=a>\
                <table>a < b<td>c</td>d<p>e</table>\
                <b><p>f</b>g</p><a><div>h</a>i</div>\
                <script>if (1 < 2) {}</script><svg><![CDATA[j]]><title>k</title></svg>\
                <math><mi>l</mi></math>\n";
    assert_eq!(
        found(text),
        [
            format!("2:2: {TITLE}"),
            format!("2:2: {DESCRIPTION}"),
            format!("2:38: {NO_H1}"),
        ]
    );
    let text = "<!doctype html>\n<html><frameset><frame></frameset></html>\n";
    assert_eq!(found(text).len(), 3);
    let text = "<!doctype html><html><body></body></html><frameset>";
    assert_eq!(found(text).len(), 3);
}

#[test]
fn the_tree_takes_what_html_parsing_never_hands_it() {
    let tree = Tree::default();
    // A processing instruction comes from XML only.
    let instruction = tree.create_pi(StrTendril::from("a"), StrTendril::from("b"));
    // A table that hangs from nothing, which only a script can make: what is
    // moved in front of it goes into the element before it.
    let table = tree.create_comment(StrTendril::from("x"));
    let before = tree.get_document();
    tree.append_based_on_parent_node(&table, &before, NodeOrText::AppendNode(instruction));
    tree.append_before_sibling(&table, NodeOrText::AppendText(StrTendril::from("y")));
    assert_eq!(tree.get_template_contents(&table), table);
    assert!(format!("{:?}", tree.finish()).contains("parent: Some(0)"));
}
