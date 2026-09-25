import Foundation
import NaturalLanguage

/// T1 input: NLEmbedding's English sentence embedding (512 dimensions). Rust
/// stores one vector per entry and runs kNN over them.
struct Embedding {
    private var model: NLEmbedding?
    private var loaded = false

    var isAvailable: Bool {
        mutating get { load() != nil }
    }

    var dimension: Int {
        mutating get { load()?.dimension ?? 0 }
    }

    var revision: Int {
        NLEmbedding.currentSentenceEmbeddingRevision(for: .english)
    }

    mutating func embed(_ texts: [String]) -> EmbedResult {
        guard let model = load() else {
            return EmbedResult(vectors: texts.map { _ in nil }, dimension: 0)
        }
        let vectors: [[Float]?] = texts.map { text in
            model.vector(for: text).map { $0.map(Float.init) }
        }
        return EmbedResult(vectors: vectors, dimension: model.dimension)
    }

    private mutating func load() -> NLEmbedding? {
        if !loaded {
            model = NLEmbedding.sentenceEmbedding(for: .english)
            loaded = true
        }
        return model
    }
}
