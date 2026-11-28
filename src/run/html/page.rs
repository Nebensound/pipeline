//! The page rules of the HTML check: what every page must carry, read off the
//! document as a browser builds it.
//!
//! html5ever, linked into the tool, parses a file by the HTML standard: its
//! tokenizer and its tree construction, with scripting off, so that what a
//! `<noscript>` holds is read as markup. The rules look at the tree it builds
//! ([`Tree`]), and a finding stands at the line and column of the name in the
//! start tag of the element it is about (`<head>` is found at its `head`),
//! counted in characters from 1.
//!
//! A page is a file that holds a doctype, or whose `<html>` root is written as
//! a tag. Any other HTML file, such as a fragment or a header that a generator
//! includes, is no page, and the rules find nothing in it. Every page has
//!
//! - a title: a `<title>` element with text in it (a `<title>` of an inline
//!   `<svg>` belongs to the image, not to the page);
//! - a description: `<meta name="description">` with a content that is not
//!   empty;
//! - exactly one `<h1>`;
//! - alternative text on every image: an `alt` attribute, which may be empty
//!   (`alt=""`) for an image that only decorates.
//!
//! What a `<template>` holds counts as part of the page. A finding about the
//! whole page stands at its `<head>`, else at its `<html>`, else at the first
//! element written in the file, else at the file's first line; a missing
//! `<h1>` stands at the `<body>` where one is written. An element the parser
//! adds without a tag, such as the `<head>` of a page that writes none, has no
//! place in the file, so a finding never stands there.
//!
//! Nothing in the file switches a rule off: no configuration of the
//! repository is read, and no comment reaches them. Only an `[[exclude]]`
//! entry of the declaration takes a page out.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use html5ever::interface::ElemName;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use html5ever::tree_builder::{
    ElementFlags, NodeOrText, QuirksMode, TreeBuilder, TreeBuilderOpts, TreeSink,
};
use html5ever::{Attribute, LocalName, Namespace, QualName, TokenizerResult, local_name, ns};

use crate::run::Finding;

/// The name the rules go by, as `page/<rule>` in front of every finding.
pub const NAME: &str = "page";

/// One node of a [`Tree`]: the document, an element, a text, or anything
/// else the parser makes (a comment, a fragment), which the rules pass over.
#[derive(Debug)]
struct Node {
    /// The node it hangs from; none for the document, a fragment and a node
    /// not placed yet.
    parent: Option<usize>,
    /// The nodes that hang from it, in order.
    children: Vec<usize>,
    /// The element's name; empty for every other node.
    name: QualName,
    /// The element's attributes.
    attributes: Vec<Attribute>,
    /// What a text holds; none for every other node.
    text: Option<String>,
    /// What a `<template>` holds: a fragment of its own.
    contents: Option<usize>,
    /// Where the name in the element's start tag begins, as a byte offset
    /// into the file; none for an element the parser adds without a tag.
    written: Option<usize>,
}

impl Node {
    /// A node with no name, no text and no place yet.
    fn new() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            name: QualName::new(None, ns!(), local_name!("")),
            attributes: Vec::new(),
            text: None,
            contents: None,
            written: None,
        }
    }

    /// Whether it is the HTML element of this name.
    fn is(&self, local: &str) -> bool {
        self.name.ns == ns!(html) && &*self.name.local == local
    }

    /// The value of an attribute without a namespace.
    fn attribute(&self, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name.ns == ns!() && &*attribute.name.local == local)
            .map(|attribute| &*attribute.value)
    }
}

/// The name of an element, as html5ever asks for it.
#[derive(Debug)]
pub struct Name(QualName);

impl ElemName for Name {
    fn ns(&self) -> &Namespace {
        &self.0.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.0.local
    }
}

/// The tree html5ever builds a document into: every node in the order it was
/// made, the document first. The page rules read it; it serves nothing else.
#[derive(Debug)]
pub struct Tree {
    nodes: RefCell<Vec<Node>>,
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            nodes: RefCell::new(vec![Node::new()]),
        }
    }
}

impl Tree {
    /// Adds a node and gives back its handle.
    fn add(&self, node: Node) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(node);
        nodes.len() - 1
    }

    /// Takes a node from the node it hangs from, if any.
    fn detach(nodes: &mut [Node], child: usize) {
        if let Some(parent) = nodes[child].parent.take() {
            nodes[parent].children.retain(|&each| each != child);
        }
    }

    /// Puts a node or a text into `parent` at `index` of its children. A
    /// text that would follow a text is joined to it, as the standard builds
    /// the tree.
    fn insert(&self, parent: usize, index: usize, child: NodeOrText<usize>) {
        let node = match child {
            NodeOrText::AppendNode(node) => node,
            NodeOrText::AppendText(text) => {
                let mut nodes = self.nodes.borrow_mut();
                let before = index.checked_sub(1).map(|at| nodes[parent].children[at]);
                if let Some(joined) = before.and_then(|at| nodes[at].text.as_mut()) {
                    joined.push_str(&text);
                    return;
                }
                drop(nodes);
                let mut node = Node::new();
                node.text = Some(text.to_string());
                self.add(node)
            }
        };
        let mut nodes = self.nodes.borrow_mut();
        Self::detach(&mut nodes, node);
        nodes[node].parent = Some(parent);
        nodes[parent].children.insert(index, node);
    }
}

impl TreeSink for Tree {
    type Handle = usize;
    type Output = Self;
    type ElemName<'a> = Name;

    fn finish(self) -> Self {
        self
    }

    // Whether the file breaks the rules of HTML is no page rule.
    fn parse_error(&self, _: Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a usize) -> Name {
        Name(self.nodes.borrow()[*target].name.clone())
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        // The fragment first, so that the element is the last node made for
        // its tag (see `Watch`).
        let contents = flags.template.then(|| self.add(Node::new()));
        let mut node = Node::new();
        node.name = name;
        node.attributes = attrs;
        node.contents = contents;
        self.add(node)
    }

    fn create_comment(&self, _: StrTendril) -> usize {
        self.add(Node::new())
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> usize {
        self.add(Node::new())
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        let index = self.nodes.borrow()[*parent].children.len();
        self.insert(*parent, index, child);
    }

    fn append_based_on_parent_node(
        &self,
        element: &usize,
        prev_element: &usize,
        child: NodeOrText<usize>,
    ) {
        if self.nodes.borrow()[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    // The doctype is seen as a token (see `Watch`), wherever it stands.
    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, target: &usize) -> usize {
        self.nodes.borrow()[*target].contents.unwrap_or(*target)
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &usize, new_node: NodeOrText<usize>) {
        let nodes = self.nodes.borrow();
        let place = nodes[*sibling].parent.map(|parent| {
            let index = nodes[parent]
                .children
                .iter()
                .position(|&child| child == *sibling)
                .unwrap_or_default();
            (parent, index)
        });
        drop(nodes);
        if let Some((parent, index)) = place {
            self.insert(parent, index, new_node);
        }
    }

    fn add_attrs_if_missing(&self, target: &usize, attrs: Vec<Attribute>) {
        let mut nodes = self.nodes.borrow_mut();
        let attributes = &mut nodes[*target].attributes;
        for attribute in attrs {
            if !attributes.iter().any(|each| each.name == attribute.name) {
                attributes.push(attribute);
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        Self::detach(&mut self.nodes.borrow_mut(), *target);
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        let mut nodes = self.nodes.borrow_mut();
        let children = std::mem::take(&mut nodes[*node].children);
        for &child in &children {
            nodes[child].parent = Some(*new_parent);
        }
        nodes[*new_parent].children.extend(children);
    }
}

/// What stands between html5ever's tokenizer and its tree builder: it hands
/// every token on, and notes what the tree does not keep, namely whether the
/// file holds a doctype and where the start tag of each element begins.
///
/// The file is fed in pieces, each beginning at a `<` ([`parse`]), and every
/// such `<` is noted in `starts` before its piece is fed. A tag, a comment and
/// a doctype are each handed on when their `>` is read. So the `<` of a start
/// tag is the first noted one after the last of them that its name follows:
/// what lies between them is text, where a `<` before a letter opens a tag,
/// and a `<` inside a value of the tag's attributes comes after its own. The
/// element made for the tag is the last node the tree builder makes for it:
/// what the standard adds without a tag, such as a `<tbody>` around a `<tr>`,
/// is made before it.
struct Watch<'s> {
    /// The tree builder, with the [`Tree`] it builds.
    builder: TreeBuilder<usize, Tree>,
    /// The file.
    source: &'s str,
    /// The byte offset of every `<` fed so far.
    starts: RefCell<Vec<usize>>,
    /// How many of `starts` were noted when the last tag, comment or doctype
    /// was handed on.
    closed: Cell<usize>,
    /// Whether a doctype was read.
    doctype: Cell<bool>,
}

impl Watch<'_> {
    /// Where the name of the start tag just read begins: after the first `<`
    /// since the last tag, comment or doctype that is followed by `name`, in
    /// any case, and then by what ends a name.
    fn written(&self, name: &str) -> Option<usize> {
        let bytes = self.source.as_bytes();
        let starts = self.starts.borrow();
        starts[self.closed.get()..].iter().find_map(|&start| {
            let from = start + 1;
            let end = from + name.len();
            let named = bytes
                .get(from..end)
                .is_some_and(|written| written.eq_ignore_ascii_case(name.as_bytes()));
            let ended = bytes.get(end).is_none_or(|byte| {
                matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ' | b'/' | b'>')
            });
            (named && ended).then_some(from)
        })
    }

    /// Notes that a tag, a comment or a doctype was handed on.
    fn close(&self) {
        self.closed.set(self.starts.borrow().len());
    }
}

impl TokenSink for Watch<'_> {
    type Handle = usize;

    fn process_token(&self, token: Token, line_number: u64) -> TokenSinkResult<usize> {
        // For a start tag, where its name begins, if it can be found.
        let start = match &token {
            Token::TagToken(tag) => {
                let start = (tag.kind == TagKind::StartTag).then(|| self.written(&tag.name));
                self.close();
                start
            }
            Token::CommentToken(_) => {
                self.close();
                None
            }
            Token::DoctypeToken(_) => {
                self.doctype.set(true);
                self.close();
                None
            }
            _ => None,
        };
        let first = self.builder.sink.nodes.borrow().len();
        let result = self.builder.process_token(token, line_number);
        if let Some(written) = start {
            let mut nodes = self.builder.sink.nodes.borrow_mut();
            let made = nodes.len();
            if let Some(element) = nodes[first..made]
                .last_mut()
                .filter(|node| !node.name.local.is_empty())
            {
                element.written = written;
            }
        }
        result
    }

    fn end(&self) {
        self.builder.end();
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.builder
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

/// A document as parsed: its nodes and whether it holds a doctype.
struct Parsed {
    nodes: Vec<Node>,
    doctype: bool,
}

/// Parses a file, fed in pieces that each begin at a `<` (see [`Watch`]).
fn parse(source: &str) -> Parsed {
    let options = TreeBuilderOpts {
        scripting_enabled: false,
        ..TreeBuilderOpts::default()
    };
    let watch = Watch {
        builder: TreeBuilder::new(Tree::default(), options),
        source,
        starts: RefCell::new(Vec::new()),
        closed: Cell::new(0),
        doctype: Cell::new(false),
    };
    let tokenizer = Tokenizer::new(watch, TokenizerOpts::default());
    let queue = BufferQueue::default();
    // The tokenizer stops after a `</script>`, for a script to run, and goes
    // on when it is fed again.
    let feed = |piece: &str| {
        queue.push_back(StrTendril::from_slice(piece));
        while !matches!(tokenizer.feed(&queue), TokenizerResult::Done) {}
    };
    let mut from = 0;
    for (at, _) in source.match_indices('<') {
        feed(&source[from..at]);
        tokenizer.sink.starts.borrow_mut().push(at);
        from = at;
    }
    feed(&source[from..]);
    tokenizer.end();
    let Watch {
        builder, doctype, ..
    } = tokenizer.sink;
    Parsed {
        nodes: builder.sink.nodes.into_inner(),
        doctype: doctype.get(),
    }
}

/// Where each line of a file begins, to turn a byte offset into a line and a
/// column. A line ends at a line feed, a carriage return, or both together.
struct Lines<'s> {
    source: &'s str,
    starts: Vec<usize>,
}

impl<'s> Lines<'s> {
    fn new(source: &'s str) -> Self {
        let bytes = source.as_bytes();
        let mut starts = vec![0];
        for (at, byte) in bytes.iter().enumerate() {
            let ends = *byte == b'\n' || (*byte == b'\r' && bytes.get(at + 1) != Some(&b'\n'));
            if ends {
                starts.push(at + 1);
            }
        }
        Self { source, starts }
    }

    /// The line and the column of a byte offset, both from 1, the column in
    /// characters.
    fn position(&self, offset: usize) -> (usize, usize) {
        let line = self.starts.partition_point(|&start| start <= offset);
        let begins = self.starts[line - 1];
        let column = self.source[begins..offset].chars().count() + 1;
        (line, column)
    }
}

impl Parsed {
    /// Every element and text in the order of the tree, what a `<template>`
    /// holds right after it.
    fn order(&self) -> Vec<usize> {
        let mut order = Vec::new();
        let mut stack = vec![0];
        while let Some(node) = stack.pop() {
            order.push(node);
            let node = &self.nodes[node];
            stack.extend(node.children.iter().rev());
            if let Some(contents) = node.contents {
                stack.extend(self.nodes[contents].children.iter().rev());
            }
        }
        order
    }

    /// The text a node holds, with the text of every node below it.
    fn text(&self, node: usize) -> String {
        let node = &self.nodes[node];
        let mut text = node.text.clone().unwrap_or_default();
        for &child in &node.children {
            text.push_str(&self.text(child));
        }
        text
    }

    /// What the page rules find, at their places; nothing for a file that is
    /// no page.
    fn findings(&self, lines: &Lines<'_>) -> Vec<Finding> {
        let order = self.order();
        let elements = |local: &'static str| {
            order
                .iter()
                .map(|&node| &self.nodes[node])
                .filter(move |node| node.is(local))
        };
        let written = |local: &'static str| elements(local).find_map(|node| node.written);
        if !self.doctype && written("html").is_none() {
            return Vec::new();
        }
        let first = order
            .iter()
            .filter_map(|&node| self.nodes[node].written)
            .min();
        let at = |offset: Option<usize>| offset.map(|offset| lines.position(offset));
        // A file with no element written: its first line.
        let page = at(written("head").or_else(|| written("html")).or(first)).or(Some((1, 1)));
        let mut findings: Vec<Finding> = Vec::new();
        let mut report = |position: Option<(usize, usize)>, rule: &str, message: String| {
            findings.push(Finding {
                position,
                message: format!("{NAME}/{rule}: {message}"),
            });
        };

        let titled = order
            .iter()
            .filter(|&&node| self.nodes[node].is("title"))
            .any(|&node| !self.text(node).trim().is_empty());
        if !titled {
            report(
                page,
                "title",
                "the page has no title: give it a <title> element with text in it".to_owned(),
            );
        }

        let described = elements("meta").any(|meta| {
            meta.attribute("name")
                .is_some_and(|name| name.to_lowercase() == "description")
                && meta
                    .attribute("content")
                    .is_some_and(|content| !content.trim().is_empty())
        });
        if !described {
            report(
                page,
                "description",
                "the page has no description: give it <meta name=\"description\" content=\"...\"> \
                 with text in the content"
                    .to_owned(),
            );
        }

        let headings: Vec<&Node> = elements("h1").collect();
        match headings.split_first() {
            None => report(
                at(written("body")).or(page),
                "one-h1",
                "the page has no <h1>, and a page has exactly one".to_owned(),
            ),
            Some((_, more)) => {
                for heading in more {
                    report(
                        at(heading.written),
                        "one-h1",
                        format!(
                            "the page has {} <h1> elements, and a page has exactly one; this is \
                             one too many",
                            headings.len()
                        ),
                    );
                }
            }
        }

        for image in elements("img").filter(|image| image.attribute("alt").is_none()) {
            let named = image
                .attribute("src")
                .filter(|source| !source.is_empty())
                .map_or_else(
                    || "this image".to_owned(),
                    |source| format!("the image {source}"),
                );
            report(
                at(image.written),
                "image-alt",
                format!(
                    "{named} has no alternative text: give it an alt attribute, alt=\"\" where it \
                     only decorates"
                ),
            );
        }

        // In the order of the file, each rule in its order where two stand
        // at one place.
        findings.sort_by_key(|finding| finding.position);
        findings
    }
}

/// What the page rules find in a file, each at its line and column. A file
/// that is not UTF-8 is read with every byte that is not replaced, and a byte
/// order mark it opens with is no part of it.
#[must_use]
pub fn findings(bytes: &[u8]) -> Vec<Finding> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    parse(text).findings(&Lines::new(text))
}
