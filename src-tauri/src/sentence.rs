// Local-LLM "Sentence" practice mode.
//
// Generates a natural sentence built ONLY from the user's practiceable
// single-word chord-library phrases plus a small fixed glue set, by shelling
// out to a local `llama-completion` binary constrained with a TRIE-structured
// GBNF grammar. The trie shape is the key perf fix: a flat alternation over
// ~1900 words ran at ~0.2 t/s; the char-trie grammar runs at ~55 t/s.
//
// Bundling/download of the binary + model is OUT OF SCOPE — they're resolved
// from `Storage::data_dir()/llm/` (already staged on the machine):
//   binary = llm/llama-completion, model = llm/model.gguf, dylibs alongside.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::storage::Storage;

/// The fixed glue set: common function words the LLM may use to stitch library
/// words into a natural sentence. These are NOT graded (they're not chords).
pub const GLUE_WORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "and", "is", "it", "in", "on", "for", "with", "that", "this",
    "was", "are", "as", "at", "by", "be", "or", "but", "not", "have", "has", "will", "can", "do",
    "so", "if", "we", "you", "they", "i",
];

/// Directory holding the staged llama binary + model + dylibs.
pub fn llm_dir() -> PathBuf {
    Storage::data_dir().join("llm")
}

/// Path to the staged `llama-completion` executable.
pub fn llama_bin() -> PathBuf {
    llm_dir().join("llama-completion")
}

/// Path to the staged GGUF model.
pub fn model_path() -> PathBuf {
    llm_dir().join("model.gguf")
}

/// Whether both the binary and the model are present on disk.
pub fn is_set_up() -> bool {
    llama_bin().exists() && model_path().exists()
}

/// Flow/Sentence "length" preset. Maps to a target word count for the generated
/// sentence (and, on the frontend, to a queue-Flow line cap). Default = `M`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowSize {
    S,
    M,
    L,
}

impl FlowSize {
    /// Parse from the wire value (`"s"`/`"m"`/`"l"`, case-insensitive). Anything
    /// unrecognized falls back to the `M` default.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "s" => FlowSize::S,
            "l" => FlowSize::L,
            _ => FlowSize::M,
        }
    }

    /// Inclusive grammar word-count range `(lo, hi)` for the generated sentence.
    pub fn sentence_range(self) -> (usize, usize) {
        match self {
            FlowSize::S => (6, 10),
            FlowSize::M => (12, 18),
            FlowSize::L => (24, 36),
        }
    }
}

/// A trie node: children keyed by the next char, plus whether a word ends here.
#[derive(Default)]
struct TrieNode {
    children: BTreeMap<char, TrieNode>,
    is_word_end: bool,
}

impl TrieNode {
    fn insert(&mut self, chars: &[char]) {
        match chars.split_first() {
            None => self.is_word_end = true,
            Some((c, rest)) => self.children.entry(*c).or_default().insert(rest),
        }
    }
}

/// Build the SIZE-INDEPENDENT trie body from `vocab` (the full word list, in any
/// case — words are emitted verbatim, so callers should pass lowercased words).
///
/// Emits everything EXCEPT the `root` line (which is size-specific and built per
/// call by [`root_line`]):
/// ```text
/// word ::= <root-node-alternation>
/// node_<id> ::= ( "<c>" <child-rule> | ... )   ; "?" suffix on word-end nodes
/// ```
/// Every node that has children becomes a named rule (`node_<id>`); leaf nodes
/// that are pure word-ends are inlined as `""`. Each child branch reads the
/// node's char literal then recurses into the child's rule. A node that is BOTH
/// a word-end and has children gets a trailing `?` so the word may stop there.
///
/// `vocab` is deduped + sorted by the caller, so identical input yields an
/// identical body (stable cache key behaviour). The body is cached on
/// `chordmap_gen`; the size-specific `root` line is prepended per request so the
/// expensive trie build is reused across sizes.
pub fn build_grammar_body(vocab: &[String]) -> String {
    // Build the trie from the deduped, non-empty vocab.
    let mut root = TrieNode::default();
    for w in vocab {
        let chars: Vec<char> = w.chars().collect();
        if !chars.is_empty() {
            root.insert(&chars);
        }
    }

    // Emit one rule per node with children, assigning stable ids via DFS.
    let mut rules: Vec<String> = Vec::new();
    let mut next_id: usize = 0;
    // The root node's rule body becomes `word`'s definition.
    let word_body = emit_node(&root, &mut rules, &mut next_id);

    let mut out = String::new();
    out.push_str(&format!("word ::= {word_body}\n"));
    for rule in rules {
        out.push_str(&rule);
        out.push('\n');
    }
    out
}

/// Build the size-specific `root` line: `word (" " word){lo-1,hi-1} "."`, where
/// `(lo, hi)` is the inclusive total-word target for `size`. The `{lo-1,hi-1}`
/// repetition counts the words AFTER the mandatory leading `word`, so total
/// words land in `lo..=hi` before the trailing period.
pub fn root_line(size: FlowSize) -> String {
    let (lo, hi) = size.sentence_range();
    format!("root ::= word (\" \" word){{{},{}}} \".\"", lo - 1, hi - 1)
}

/// Assemble the full GBNF for a given size: the size-specific `root` line plus
/// the cached, size-independent trie body. Returns a self-contained, well-formed
/// grammar (every referenced rule is defined).
pub fn assemble_grammar(size: FlowSize, body: &str) -> String {
    format!("{}\n{}", root_line(size), body)
}

/// Emit the GBNF body for one node and (recursively) any child rules it needs.
/// Returns the body string to splice into the parent's alternative. Child rules
/// are appended to `rules`; `next_id` hands out stable rule ids.
fn emit_node(node: &TrieNode, rules: &mut Vec<String>, next_id: &mut usize) -> String {
    // A leaf word-end (no children) contributes the empty string.
    if node.children.is_empty() {
        return "\"\"".to_string();
    }

    // Build one alternative per child: the char literal then the child's rule
    // reference (or inlined body for leaf children).
    let mut alts: Vec<String> = Vec::new();
    for (c, child) in &node.children {
        let lit = escape_gbnf_char(*c);
        if child.children.is_empty() {
            // Leaf child: matching the char completes a word here.
            alts.push(format!("\"{lit}\""));
        } else {
            let child_ref = define_node_rule(child, rules, next_id);
            alts.push(format!("\"{lit}\" {child_ref}"));
        }
    }

    let body = format!("( {} )", alts.join(" | "));
    // If this node also terminates a word AND has children, the word may stop
    // here → make the continuation optional.
    if node.is_word_end {
        format!("{body}?")
    } else {
        body
    }
}

/// Define a named rule for `node` (which has children), returning its rule name.
fn define_node_rule(node: &TrieNode, rules: &mut Vec<String>, next_id: &mut usize) -> String {
    let id = *next_id;
    *next_id += 1;
    let name = format!("node_{id}");
    // Reserve this rule's slot BEFORE recursing so its body (which appends the
    // child rules) lands after it; capture the slot index directly.
    let slot = rules.len();
    rules.push(String::new());
    let body = emit_node(node, rules, next_id);
    rules[slot] = format!("{name} ::= {body}");
    name
}

/// Escape a single char for a GBNF double-quoted literal. Backslash and double
/// quote must be escaped; everything else (library words are alphabetic +
/// apostrophe/hyphen per `is_practiceable`/allowed_chars) passes through.
fn escape_gbnf_char(c: char) -> String {
    match c {
        '\\' => "\\\\".to_string(),
        '"' => "\\\"".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_emits_root_and_word_rules() {
        let vocab: Vec<String> = ["the", "point", "touch"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let g = assemble_grammar(FlowSize::M, &build_grammar_body(&vocab));

        // Root + word rules are present.
        assert!(g.contains("root ::="), "root rule emitted");
        assert!(g.contains("word ::="), "word rule emitted");
        // The first chars of the vocab appear as literals on the word rule.
        assert!(g.contains('t'), "vocab chars present");
        assert!(g.contains('p'), "vocab chars present");
    }

    #[test]
    fn root_line_range_matches_size() {
        // Each size's root line uses {lo-1,hi-1} (the words after the leading one).
        for (size, lo, hi) in [
            (FlowSize::S, 6usize, 10usize),
            (FlowSize::M, 12, 18),
            (FlowSize::L, 24, 36),
        ] {
            let line = root_line(size);
            let expected = format!("(\" \" word){{{},{}}}", lo - 1, hi - 1);
            assert!(
                line.contains(&expected),
                "size {size:?} root line should contain {expected}; got: {line}"
            );
            // Sanity: still a well-formed root rule ending in a literal period.
            assert!(line.starts_with("root ::= word "));
            assert!(line.ends_with("\".\""));
        }
    }

    #[test]
    fn every_referenced_rule_is_defined() {
        // A vocab with shared prefixes ("point"/"touch" both start 't'?) — no,
        // use words that force nested node rules: "to","toe","ton".
        let vocab: Vec<String> = ["to", "toe", "ton", "point", "touch"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let g = assemble_grammar(FlowSize::L, &build_grammar_body(&vocab));

        // Collect defined rule names (LHS of "::=") and referenced node_* names.
        let mut defined = std::collections::HashSet::new();
        for line in g.lines() {
            if let Some((lhs, _)) = line.split_once("::=") {
                defined.insert(lhs.trim().to_string());
            }
        }
        assert!(defined.contains("root"));
        assert!(defined.contains("word"));

        // Every `node_N` token referenced anywhere must be a defined rule.
        for line in g.lines() {
            for tok in line.split_whitespace() {
                let name = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                if name.starts_with("node_") {
                    assert!(
                        defined.contains(name),
                        "referenced rule {name} is undefined; grammar:\n{g}"
                    );
                }
            }
        }
    }

    #[test]
    fn shared_prefix_collapses_into_one_branch() {
        // "to","toe","ton" share the "to" prefix → the word rule must branch on
        // 't' once, not three times.
        let vocab: Vec<String> = ["to", "toe", "ton"].iter().map(|s| s.to_string()).collect();
        let g = build_grammar_body(&vocab);
        let word_line = g
            .lines()
            .find(|l| l.starts_with("word ::="))
            .expect("word rule");
        // Only one top-level alternative (the 't' branch) → no '|' at word level.
        assert!(
            !word_line.contains('|'),
            "single shared prefix should not split the word rule: {word_line}"
        );
    }
}
