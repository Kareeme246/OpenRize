import Foundation

// One JSON object per line each way: `{"id": 7, "m": "classify", "p": {...}}` in, `{"id": 7,
// "r": {...}}` or `{"id": 7, "e": "message"}` out. Stateless apart from model caches; Rust
// (src-tauri/src/ai/sidecar.rs) owns all user data.

struct Envelope: Decodable {
    let id: Int
    let m: String
}

struct WithParams<P: Decodable>: Decodable {
    let p: P
}

struct Reply<R: Encodable>: Encodable {
    let id: Int
    let r: R
}

struct Failure: Encodable {
    let id: Int
    let e: String
}

struct SidecarError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

struct CapabilitiesResult: Encodable {
    /// `available`, or why the Foundation Model can't be used:
    /// `appleIntelligenceNotEnabled`, `modelNotReady`, `deviceNotEligible`,
    /// `unsupportedOS` (macOS below 26), or `unknown`.
    let llm: String
    let embed: Bool
    let embedDimension: Int
    let embedRevision: Int
    let personal: Bool
    let os: String
    let version: String
}

struct EmbedParams: Decodable {
    let texts: [String]
}

struct EmbedResult: Encodable {
    /// One vector per input text, `null` where NLEmbedding had no vector.
    let vectors: [[Float]?]
    let dimension: Int
}

struct Choice: Decodable {
    let id: String
    let name: String
    let description: String?
    /// Category `ai_prompt` or project `ai_hints`.
    let hint: String?
}

struct Example: Decodable {
    let features: String
    let category: String
    let project: String?
}

struct ClassifyParams: Decodable {
    let features: String
    let categories: [Choice]
    let projects: [Choice]?
    let examples: [Example]?
    let customInstructions: String?
    let samples: Int?
    /// Unset for the first (greedy) call; set for extra disagreement samples.
    let temperature: Double?
}

struct ClassifyResult: Encodable {
    /// Category id -> number of samples that picked it.
    let categoryVotes: [String: Int]
    /// Project id (or `none`) -> number of samples that picked it.
    let projectVotes: [String: Int]?
    let description: String?
    let samples: Int
    let latencyMs: Int
}

struct TrainRow: Decodable {
    let text: String
    let label: String
    let holdout: Bool
}

struct TrainParams: Decodable {
    let dataset: String
    let out: String
}

struct TrainResult: Encodable {
    let path: String
    let trainExamples: Int
    let holdoutExamples: Int
    /// Share of holdout rows the new model labels correctly; null when there
    /// was no holdout to score against.
    let holdoutAccuracy: Double?
    let labels: [String]
    let trainMs: Int
}

struct PredictParams: Decodable {
    let model: String
    let texts: [String]
}

struct PredictResult: Encodable {
    let hypotheses: [[String: Double]]
}

struct Empty: Encodable {}
