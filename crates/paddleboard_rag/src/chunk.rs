//! Language-agnostic structural chunking for the RAG indexer.
//!
//! v1 splits file text on blank-line "paragraph" boundaries and merges
//! paragraphs up to a byte budget. This works uniformly for prose and code
//! (blank lines separate functions/blocks well enough) and needs no tree-sitter
//! grammar, so it runs on a background thread over raw text. Outline/AST-aware
//! chunking is a deliberate future refinement — it needs a loaded `Language`
//! and a `Buffer`, which are foreground-only.

/// Upper bound on a chunk's size in bytes. EmbeddingGemma is served at ctx 2048;
/// ~1600 bytes keeps a chunk plus its task prefix comfortably under that.
const MAX_CHUNK_BYTES: usize = 1600;
/// Once a chunk reaches this size, a blank line flushes it so chunks align to
/// natural paragraph/block breaks rather than arbitrary byte offsets.
const SOFT_FLUSH_BYTES: usize = MAX_CHUNK_BYTES / 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Byte offset of the chunk's first character within the file.
    pub start_byte: usize,
    /// Byte offset one past the chunk's last character.
    pub end_byte: usize,
    /// 1-based line number of the chunk's first line.
    pub start_line: usize,
    pub text: String,
}

/// Split `text` into chunks. Every returned chunk satisfies the invariant
/// `text[chunk.start_byte..chunk.end_byte] == chunk.text`; whitespace-only
/// spans are dropped.
pub fn chunk_text(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_start_byte = 0;
    let mut current_start_line = 1;
    let mut byte = 0;
    let mut line = 1;

    for raw_line in text.split_inclusive('\n') {
        // A single line larger than the budget (minified JS, embedded data) is
        // hard-split so no chunk blows past the embedding context.
        if raw_line.len() > MAX_CHUNK_BYTES {
            if !current.is_empty() {
                push_chunk(&mut chunks, &current, current_start_byte, byte, current_start_line);
                current.clear();
            }
            split_long_line(&mut chunks, raw_line, byte, line);
            byte += raw_line.len();
            line += 1;
            current_start_byte = byte;
            current_start_line = line;
            continue;
        }

        if !current.is_empty() && current.len() + raw_line.len() > MAX_CHUNK_BYTES {
            push_chunk(&mut chunks, &current, current_start_byte, byte, current_start_line);
            current.clear();
        }
        if current.is_empty() {
            current_start_byte = byte;
            current_start_line = line;
        }
        current.push_str(raw_line);
        byte += raw_line.len();
        line += 1;

        if raw_line.trim().is_empty() && current.len() >= SOFT_FLUSH_BYTES {
            push_chunk(&mut chunks, &current, current_start_byte, byte, current_start_line);
            current.clear();
        }
    }

    if !current.is_empty() {
        push_chunk(&mut chunks, &current, current_start_byte, byte, current_start_line);
    }
    chunks
}

fn push_chunk(chunks: &mut Vec<Chunk>, text: &str, start_byte: usize, end_byte: usize, start_line: usize) {
    if !text.trim().is_empty() {
        chunks.push(Chunk {
            start_byte,
            end_byte,
            start_line,
            text: text.to_string(),
        });
    }
}

fn split_long_line(chunks: &mut Vec<Chunk>, raw_line: &str, start_byte: usize, line: usize) {
    let mut offset = 0;
    while offset < raw_line.len() {
        let mut end = (offset + MAX_CHUNK_BYTES).min(raw_line.len());
        while end < raw_line.len() && !raw_line.is_char_boundary(end) {
            end -= 1;
        }
        if end <= offset {
            end = raw_line.len();
        }
        push_chunk(chunks, &raw_line[offset..end], start_byte + offset, start_byte + end, line);
        offset = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every chunk's byte range must reslice to exactly its text.
    fn assert_byte_ranges(text: &str, chunks: &[Chunk]) {
        for chunk in chunks {
            assert_eq!(
                &text[chunk.start_byte..chunk.end_byte],
                chunk.text,
                "byte range {}..{} did not match chunk text",
                chunk.start_byte,
                chunk.end_byte
            );
        }
    }

    #[test]
    fn empty_and_whitespace_yield_no_chunks() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("   \n\n  \n").is_empty());
    }

    #[test]
    fn small_text_is_a_single_chunk_from_line_one() {
        let text = "fn main() {\n    println!(\"hi\");\n}\n";
        let chunks = chunk_text(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].start_byte, 0);
        assert_byte_ranges(text, &chunks);
    }

    #[test]
    fn large_input_splits_and_preserves_offsets() {
        // Many paragraphs separated by blank lines, each well past the soft
        // flush threshold in aggregate.
        let paragraph = "x".repeat(200);
        let text = (0..20)
            .map(|_| paragraph.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk_text(&text);
        assert!(chunks.len() > 1, "expected multiple chunks, got {}", chunks.len());
        assert_byte_ranges(&text, &chunks);
        // start_line is monotonically non-decreasing across chunks.
        for pair in chunks.windows(2) {
            assert!(pair[1].start_line >= pair[0].start_line);
        }
    }

    #[test]
    fn overlong_single_line_is_hard_split() {
        let text = format!("{}\n", "a".repeat(MAX_CHUNK_BYTES * 3 + 7));
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 3);
        assert_byte_ranges(&text, &chunks);
        assert!(chunks.iter().all(|chunk| chunk.text.len() <= MAX_CHUNK_BYTES));
    }
}
