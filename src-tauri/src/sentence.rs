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
use crate::types::Settings;

/// A downloadable single-file GGUF model in the static catalog. URLs point at
/// Hugging Face `resolve/main/...` paths that 302-redirect to the HF CDN
/// (reqwest follows redirects by default). `filename` is the stable on-disk name
/// under `models_dir()`; `id` is the wire/Settings key.
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub size_mb: u32,
    pub url: &'static str,
    pub filename: &'static str,
}

/// The static model catalog. The FIRST entry is the default (used when
/// `Settings.sentence_model` is empty or names an un-downloaded model).
pub const MODEL_CATALOG: &[ModelInfo] = &[
    ModelInfo {
        id: "smollm2-360m",
        name: "SmolLM2 360M",
        description: "Smallest & fastest",
        size_mb: 270,
        url: "https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/main/SmolLM2-360M-Instruct-Q4_K_M.gguf",
        filename: "smollm2-360m.gguf",
    },
    ModelInfo {
        id: "qwen2.5-0.5b",
        name: "Qwen2.5 0.5B",
        description: "Balanced quality",
        size_mb: 400,
        url: "https://huggingface.co/bartowski/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/Qwen2.5-0.5B-Instruct-Q4_K_M.gguf",
        filename: "qwen2.5-0.5b.gguf",
    },
    ModelInfo {
        id: "gemma-3-1b",
        name: "Gemma 3 1B",
        description: "Best quality, larger",
        size_mb: 800,
        url: "https://huggingface.co/unsloth/gemma-3-1b-it-GGUF/resolve/main/gemma-3-1b-it-Q4_K_M.gguf",
        filename: "gemma-3-1b.gguf",
    },
];

/// Look up a catalog entry by id.
pub fn model_by_id(id: &str) -> Option<&'static ModelInfo> {
    MODEL_CATALOG.iter().find(|m| m.id == id)
}

/// The default catalog id (first entry). Used when no valid downloaded model is
/// selected.
pub fn default_model_id() -> &'static str {
    MODEL_CATALOG[0].id
}

/// Directory holding the managed, downloadable GGUF models: `llm_dir()/models`.
pub fn models_dir() -> PathBuf {
    llm_dir().join("models")
}

/// On-disk path for a catalog model's file under `models_dir()`. `None` for an
/// unknown id.
pub fn model_file(id: &str) -> Option<PathBuf> {
    model_by_id(id).map(|m| models_dir().join(m.filename))
}

/// Whether a catalog model has been fully downloaded (its final file exists).
pub fn is_model_downloaded(id: &str) -> bool {
    model_file(id).map(|p| p.exists()).unwrap_or(false)
}

/// The active model id: the user's `Settings.sentence_model` when it names a
/// downloaded catalog model, otherwise the catalog default.
pub fn active_model_id(settings: &Settings) -> String {
    let chosen = settings.sentence_model.trim();
    if !chosen.is_empty() && is_model_downloaded(chosen) {
        chosen.to_string()
    } else {
        default_model_id().to_string()
    }
}

/// Resolve the absolute path to the model the sentence generator should load:
///   1. the active catalog model's file under `models_dir()` if downloaded, else
///   2. the legacy already-staged `llm_dir()/model.gguf` if present, else
///   3. `None` (no model installed).
pub fn active_model_path(settings: &Settings) -> Option<PathBuf> {
    if let Some(p) = model_file(&active_model_id(settings)) {
        if p.exists() {
            return Some(p);
        }
    }
    let legacy = model_path();
    if legacy.exists() {
        return Some(legacy);
    }
    None
}

/// The fixed glue set: common function words the LLM may use to stitch library
/// words into a natural sentence. These are NOT graded (they're not chords).
pub const GLUE_WORDS: &[&str] = &[
    // Determiners / articles / quantifiers.
    "the", "a", "an", "this", "that", "these", "those", "some", "any", "all", "each", "every",
    "both", "few", "many", "much", "most", "more", "less", "no", "other", "another", "such",
    // Pronouns + possessives.
    "i", "you", "he", "she", "it", "we", "they", "me", "him", "her", "us", "them", "my", "your",
    "his", "its", "our", "their", "mine", "yours", "ours", "theirs", "who", "whom", "whose",
    "what", "which", "someone", "something", "anyone", "anything", "everyone", "everything",
    "nobody", "nothing", "myself", "yourself", "ourselves", "themselves",
    // Prepositions.
    "of", "to", "in", "on", "at", "for", "with", "by", "from", "into", "over", "under", "about",
    "after", "before", "between", "through", "during", "without", "within", "against", "among",
    "around", "across", "behind", "near", "off", "out", "up", "down", "upon",
    // Conjunctions / connectives.
    "and", "or", "but", "so", "nor", "yet", "if", "because", "while", "when", "where", "why",
    "how", "although", "though", "since", "unless", "until", "whether", "than", "then", "as",
    // Common adverbs.
    "not", "very", "just", "also", "only", "even", "still", "too", "well", "back", "away",
    "almost", "really", "quite", "ever", "never", "always", "often", "sometimes", "usually",
    "now", "today", "here", "there", "soon", "again", "rather", "perhaps",
    // be / have / do / modals.
    "be", "is", "am", "are", "was", "were", "been", "being", "have", "has", "had", "having",
    "do", "does", "did", "done", "doing", "will", "would", "can", "could", "shall", "should",
    "may", "might", "must",
    // High-frequency verbs + their common inflections.
    "go", "goes", "went", "going", "gone", "get", "gets", "got", "getting", "make", "makes",
    "made", "making", "take", "takes", "took", "taking", "see", "sees", "saw", "seeing", "seen",
    "know", "knows", "knew", "knowing", "known", "think", "thinks", "thought", "thinking", "come",
    "comes", "came", "coming", "want", "wants", "wanted", "wanting", "use", "uses", "used",
    "using", "find", "finds", "found", "finding", "give", "gives", "gave", "giving", "given",
    "tell", "tells", "told", "telling", "work", "works", "worked", "working", "call", "calls",
    "called", "calling", "try", "tries", "tried", "trying", "ask", "asks", "asked", "asking",
    "need", "needs", "needed", "needing", "feel", "feels", "felt", "feeling", "become", "becomes",
    "became", "becoming", "leave", "leaves", "left", "leaving", "mean", "means", "meant",
    "meaning", "keep", "keeps", "kept", "keeping", "let", "lets", "letting", "begin", "begins",
    "began", "beginning", "seem", "seems", "seemed", "help", "helps", "helped", "helping", "talk",
    "talks", "talked", "turn", "turns", "turned", "start", "starts", "started", "show", "shows",
    "showed", "shown", "hear", "hears", "heard", "play", "plays", "played", "run", "runs", "ran",
    "running", "move", "moves", "moved", "like", "likes", "liked", "live", "lives", "lived",
    "living", "believe", "bring", "brings", "brought", "happen", "happens", "happened", "write",
    "writes", "wrote", "writing", "written", "sit", "sits", "sat", "stand", "stands", "stood",
    "lose", "loses", "lost", "pay", "pays", "paid", "meet", "meets", "met", "set", "sets",
    "learn", "learns", "learned", "change", "changes", "changed", "changing", "understand",
    "understood", "watch", "follow", "stop", "stops", "stopped", "speak", "spoke", "read",
    "reads", "spend", "spent", "grow", "grows", "grew", "open", "opens", "opened", "walk",
    "walks", "walked", "win", "wins", "won", "remember", "love", "loves", "loved", "buy",
    "bought", "wait", "waits", "waited", "send", "sent", "build", "built", "stay", "stays",
    "stayed", "fall", "fell", "add", "adds", "added", "say", "says", "said", "saying", "put",
    "puts",
    // Ultra-common nouns (+ frequent plurals).
    "time", "times", "year", "years", "day", "days", "way", "ways", "thing", "things", "people",
    "person", "man", "men", "woman", "women", "child", "children", "life", "world", "hand",
    "hands", "part", "place", "places", "case", "week", "point", "home", "water", "room", "night",
    "name", "word", "words", "story", "fact", "lot", "book", "eye", "job", "side", "kind", "head",
    "house", "friend", "father", "mother", "hour", "game", "line", "end", "money", "idea", "body",
    "face", "door", "reason", "moment", "air", "past", "present", "future",
    // Ultra-common adjectives.
    "good", "new", "first", "last", "long", "great", "little", "own", "old", "right", "big",
    "high", "different", "small", "large", "next", "early", "young", "important", "public", "bad",
    "same", "able", "best", "better", "sure", "free", "true", "full", "easy", "hard", "real",
    "simple", "clear", "certain", "personal", "open", "short", "low", "late", "main",
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

/// The fixed `starter` rule: forces the FIRST token to be a sentence-opener
/// (subject/determiner) so generated sentences don't begin mid-phrase. Every
/// opener is in [`GLUE_WORDS`], so it's always a valid terminal regardless of
/// the chord library. Ships with every grammar (size- and library-independent).
pub const STARTER_LINE: &str =
    "starter ::= \"the\" | \"a\" | \"an\" | \"this\" | \"that\" | \"it\" | \"we\" | \"you\" | \"they\" | \"i\" | \"if\"";

/// Build the size-specific `root` line: `starter (" " word){lo-1,hi-1} "."`,
/// where `(lo, hi)` is the inclusive total-word target for `size`. The leading
/// `starter` is the first word, so the `{lo-1,hi-1}` repetition counts the words
/// AFTER it — total words land in `lo..=hi` before the trailing period.
pub fn root_line(size: FlowSize) -> String {
    let (lo, hi) = size.sentence_range();
    format!(
        "root ::= starter (\" \" word){{{},{}}} \".\"",
        lo - 1,
        hi - 1
    )
}

/// Assemble the full GBNF for a given size: the size-specific `root` line, the
/// fixed `starter` rule, plus the cached, size-independent trie body. Returns a
/// self-contained, well-formed grammar (every referenced rule is defined).
pub fn assemble_grammar(size: FlowSize, body: &str) -> String {
    format!("{}\n{}\n{}", root_line(size), STARTER_LINE, body)
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
    // No underscore: GBNF rule names allow only letters/digits/dashes, so
    // "node_0" fails to parse (llama then silently free-generates). "node0" is valid.
    let name = format!("node{id}");
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

        // Root + starter + word rules are present.
        assert!(g.contains("root ::="), "root rule emitted");
        assert!(g.contains("starter ::="), "starter rule emitted");
        assert!(g.contains("word ::="), "word rule emitted");
        // The first chars of the vocab appear as literals on the word rule.
        assert!(g.contains('t'), "vocab chars present");
        assert!(g.contains('p'), "vocab chars present");
    }

    #[test]
    fn root_line_range_matches_size() {
        // Each size's root line uses {lo-1,hi-1} (the words after the leading
        // starter, which counts as the first word).
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
            // Sanity: still a well-formed root rule that opens with the starter
            // and ends in a literal period.
            assert!(line.starts_with("root ::= starter "));
            assert!(line.ends_with("\".\""));
        }
    }

    #[test]
    fn grammar_defines_starter_rule() {
        let vocab: Vec<String> = ["the", "point"].iter().map(|s| s.to_string()).collect();
        let g = assemble_grammar(FlowSize::M, &build_grammar_body(&vocab));
        let starter_line = g
            .lines()
            .find(|l| l.starts_with("starter ::="))
            .expect("starter rule defined");
        // The fixed opener set is present as terminals.
        for opener in ["the", "a", "an", "this", "that", "it", "we", "you", "they", "i", "if"] {
            assert!(
                starter_line.contains(&format!("\"{opener}\"")),
                "starter should include opener {opener:?}; got: {starter_line}"
            );
        }
        // root references starter as the leading token.
        assert!(g.contains("root ::= starter "));
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

        // GBNF rule names allow only letters/digits/dashes — an underscore makes
        // llama fail to parse the grammar and silently free-generate. Guard it.
        for name in &defined {
            assert!(
                !name.contains('_'),
                "rule name {name} has an underscore (invalid GBNF)"
            );
        }

        // Every `nodeN` token referenced anywhere must be a defined rule.
        for line in g.lines() {
            for tok in line.split_whitespace() {
                let name = tok.trim_matches(|c: char| !c.is_alphanumeric());
                if name.starts_with("node") && name.len() > 4 {
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
