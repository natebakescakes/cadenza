// Local-LLM "Sentence" practice mode.
//
// Generates a natural sentence by shelling out to a local `llama-completion`
// binary. We TRUST the model to write correct, natural English (it handles all
// conjugations/plurals/irregulars) and bias it toward the user's chords via a
// seed-word prompt. Words are GRADED after the fact by recognizing whether a
// word's base (lemma) form is a known chord — genuinely-novel words are surfaced
// (not graded) so the user can expand their chord library.
//
// Bundling/download of the binary + model is OUT OF SCOPE — they're resolved
// from `Storage::data_dir()/llm/` (already staged on the machine):
//   binary = llm/llama-completion, model = llm/model.gguf, dylibs alongside.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::storage::{lemma_bases, Storage};
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

/// The fixed glue set: common function words the LLM uses to stitch library
/// words into a natural sentence. The user has confirmed these are all chorded,
/// so for GRADING purposes they count as known chords (recognized, not flagged
/// as novel/expansion words).
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

/// The glue set as a `HashSet` for O(1) recognition lookups.
pub fn glue_set() -> HashSet<&'static str> {
    GLUE_WORDS.iter().copied().collect()
}

/// Decide whether `word` (any case) is a KNOWN chord for grading purposes:
///   1. it (lowercased) is directly in the user's `library_set`, OR
///   2. any of its base/lemma forms (inverse-inflected) is in `library_set`
///      (so "changing"/"changes" grade when "change" is a chord), OR
///   3. it's a glue word (the user has confirmed glue words are all chorded).
///
/// `library_set` and `glue` hold lowercased entries. Returns false for
/// genuinely-novel words — the "expand your library" tokens, which are typed
/// but not graded.
pub fn is_known_chord(word: &str, library_set: &HashSet<String>, glue: &HashSet<&str>) -> bool {
    let lc = word.to_lowercase();
    if lc.is_empty() {
        return false;
    }
    if library_set.contains(&lc) {
        return true;
    }
    if glue.contains(lc.as_str()) {
        return true;
    }
    lemma_bases(&lc)
        .into_iter()
        .any(|base| library_set.contains(&base))
}

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

    /// Upper word-count bound for the size, used to scale the model's token
    /// budget (`-n`) so a longer sentence isn't cut short.
    pub fn max_words(self) -> usize {
        match self {
            FlowSize::S => 10,
            FlowSize::M => 18,
            FlowSize::L => 36,
        }
    }

    /// The natural-language length adjective spliced into the generation prompt.
    pub fn length_word(self) -> &'static str {
        match self {
            FlowSize::S => "short",
            FlowSize::M => "medium-length",
            FlowSize::L => "long",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(words: &[&str]) -> HashSet<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn known_chord_direct_membership() {
        let library = lib(&["change", "point", "touch"]);
        let glue = glue_set();
        assert!(is_known_chord("change", &library, &glue));
        assert!(is_known_chord("Point", &library, &glue)); // case-insensitive
        assert!(!is_known_chord("zebra", &library, &glue));
    }

    #[test]
    fn known_chord_via_lemma() {
        // The library has the base "change"; inflected forms grade via lemma.
        let library = lib(&["change"]);
        let glue = glue_set();
        assert!(is_known_chord("changing", &library, &glue));
        assert!(is_known_chord("changes", &library, &glue));
        assert!(is_known_chord("changed", &library, &glue));
    }

    #[test]
    fn glue_words_count_as_known() {
        // No library at all, but glue words still grade as known chords.
        let library: HashSet<String> = HashSet::new();
        let glue = glue_set();
        assert!(is_known_chord("the", &library, &glue));
        assert!(is_known_chord("And", &library, &glue));
        assert!(is_known_chord("with", &library, &glue));
        // A genuinely-novel content word is NOT known → surfaced for expansion.
        assert!(!is_known_chord("quokka", &library, &glue));
    }

    #[test]
    fn length_word_matches_size() {
        assert_eq!(FlowSize::S.length_word(), "short");
        assert_eq!(FlowSize::M.length_word(), "medium-length");
        assert_eq!(FlowSize::L.length_word(), "long");
    }
}
